use protocol::{ToolCall, ToolCodec, ToolName};
use serde_json::{Map, Value};

use crate::message::ChatToolCall;
use crate::text_codec::{TOOL_CALL_CLOSE, TOOL_CALL_OPEN};
use crate::tool_catalog::{ParameterKind, parameter_kind};
use crate::validation::ToolCallError;

const ARG_KEY_OPEN: &str = "<arg_key>";
const ARG_KEY_CLOSE: &str = "</arg_key>";
const ARG_VALUE_OPEN: &str = "<arg_value>";
const ARG_VALUE_CLOSE: &str = "</arg_value>";

const INVALID_JSON: &str = r#"error: tool call is not valid JSON. Write <tool_call>{"name": ..., "arguments": {...}}</tool_call>"#;
const MISSING_NAME: &str = r#"error: tool call has no name. Write <tool_call>{"name": ..., "arguments": {...}}</tool_call>"#;

// Small models write these keys in place of each other
const ARGUMENT_KEYS: [&str; 3] = ["arguments", "parameters", "args"];

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedReply {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub problems: Vec<String>,
}

// text is the stream text without think blocks, and the text codec ignores native_calls and numbers its call ids from 0 after call_id_prefix
pub fn parse_reply(
    codec: ToolCodec,
    text: &str,
    native_calls: &[ChatToolCall],
    call_id_prefix: &str,
) -> ParsedReply {
    match codec {
        ToolCodec::Native => parse_native_reply(text, native_calls),
        ToolCodec::Text => parse_text_reply(text, call_id_prefix),
    }
}

fn parse_native_reply(text: &str, native_calls: &[ChatToolCall]) -> ParsedReply {
    let mut reply = ParsedReply {
        text: text.trim().to_owned(),
        tool_calls: Vec::new(),
        problems: Vec::new(),
    };
    for call in native_calls {
        let raw = call.arguments.trim();
        let arguments = if raw.is_empty() {
            Ok(Value::Object(Map::new()))
        } else {
            serde_json::from_str(raw)
        };
        match arguments {
            Ok(arguments) => reply.tool_calls.push(ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments,
            }),
            Err(_) => reply.problems.push(
                ToolCallError::InvalidArguments {
                    name: call.name.clone(),
                    reason: "not valid JSON".to_owned(),
                }
                .to_string(),
            ),
        }
    }
    reply
}

fn parse_text_reply(text: &str, call_id_prefix: &str) -> ParsedReply {
    let Some((visible, blocks)) = text.split_once(TOOL_CALL_OPEN) else {
        return parse_untagged_reply(text, call_id_prefix);
    };
    let mut reply = ParsedReply {
        text: visible.trim().to_owned(),
        tool_calls: Vec::new(),
        problems: Vec::new(),
    };
    for block in blocks.split(TOOL_CALL_OPEN) {
        // A block without a closing tag ends at the next block or at the end of the text
        let payload = block
            .split_once(TOOL_CALL_CLOSE)
            .map_or(block, |(payload, _)| payload);
        match parse_call_block(strip_code_fence(payload)) {
            Ok((name, arguments)) => reply.tool_calls.push(ToolCall {
                id: format!("{call_id_prefix}{}", reply.tool_calls.len()),
                name,
                arguments,
            }),
            Err(problem) => reply.problems.push(problem.to_owned()),
        }
    }
    reply
}

// Without tags, only a whole reply that is one JSON object with a name and an arguments key
// is a tool call, because other JSON is an answer to the user
fn parse_untagged_reply(text: &str, call_id_prefix: &str) -> ParsedReply {
    let text_only = || ParsedReply {
        text: text.trim().to_owned(),
        tool_calls: Vec::new(),
        problems: Vec::new(),
    };
    let Ok(value) = serde_json::from_str::<Value>(strip_code_fence(text)) else {
        return text_only();
    };
    let has_call_shape = value.get("name").is_some_and(Value::is_string)
        && ARGUMENT_KEYS.iter().any(|key| value.get(key).is_some());
    if !has_call_shape {
        return text_only();
    }
    match decode_call(value) {
        Ok((name, arguments)) => ParsedReply {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: format!("{call_id_prefix}0"),
                name,
                arguments,
            }],
            problems: Vec::new(),
        },
        Err(problem) => ParsedReply {
            text: String::new(),
            tool_calls: Vec::new(),
            problems: vec![problem.to_owned()],
        },
    }
}

fn parse_call_block(payload: &str) -> Result<(String, Value), &'static str> {
    if let Ok(value) = serde_json::from_str(payload) {
        return decode_call(value);
    }
    if let Some(call) = parse_argument_tag_call(payload) {
        return Ok(call);
    }
    parse_unkeyed_name_call(payload).map_or(Err(INVALID_JSON), decode_call)
}

// read<arg_key>path</arg_key><arg_value>a.rs</arg_value>, one text form some models write
fn parse_argument_tag_call(payload: &str) -> Option<(String, Value)> {
    let (name, mut rest) = payload.split_once(ARG_KEY_OPEN)?;
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) || name.contains('<') {
        return None;
    }
    let tool = ToolName::parse(name);
    let mut arguments = Map::new();
    loop {
        let (key, after_key) = rest.split_once(ARG_KEY_CLOSE)?;
        let (value, after_value) = after_key
            .trim_start()
            .strip_prefix(ARG_VALUE_OPEN)?
            .split_once(ARG_VALUE_CLOSE)?;
        let key = key.trim();
        let kind = tool.and_then(|tool| parameter_kind(tool, key));
        arguments.insert(key.to_owned(), typed_argument(kind, value));
        let after_value = after_value.trim_start();
        if after_value.is_empty() {
            return Some((name.to_owned(), Value::Object(arguments)));
        }
        rest = after_value.strip_prefix(ARG_KEY_OPEN)?;
    }
}

// A tag value is text, so only the declared kind of the parameter makes it a number or a boolean
fn typed_argument(kind: Option<ParameterKind>, text: &str) -> Value {
    let typed = match kind {
        Some(ParameterKind::Integer { .. }) => text.trim().parse::<i64>().ok().map(Value::from),
        Some(ParameterKind::Boolean) => text.trim().parse::<bool>().ok().map(Value::from),
        Some(ParameterKind::String { .. }) | None => None,
    };
    typed.unwrap_or_else(|| Value::from(text))
}

// {"read", "arguments": {...}}, one text form some models write, with the name key left out
fn parse_unkeyed_name_call(payload: &str) -> Option<Value> {
    let members = payload.strip_prefix('{')?.trim_start();
    let mut names = serde_json::Deserializer::from_str(members).into_iter::<String>();
    let name = names.next()?.ok()?;
    let rest = members[names.byte_offset()..]
        .trim_start()
        .strip_prefix(',')?;
    let mut value: Value = serde_json::from_str(&format!("{{{rest}")).ok()?;
    let object = value.as_object_mut()?;
    if object.contains_key("name") || !ARGUMENT_KEYS.iter().any(|key| object.contains_key(*key)) {
        return None;
    }
    object.insert("name".to_owned(), Value::from(name));
    Some(value)
}

fn strip_code_fence(text: &str) -> &str {
    let text = text.trim();
    let Some(fenced) = text.strip_prefix("```") else {
        return text;
    };
    fenced
        .strip_suffix("```")
        .unwrap_or(fenced)
        .trim_start_matches(|character: char| character.is_ascii_alphanumeric())
        .trim()
}

fn decode_call(mut value: Value) -> Result<(String, Value), &'static str> {
    let Some(Value::String(name)) = value.get_mut("name").map(Value::take) else {
        return Err(MISSING_NAME);
    };
    let arguments = ARGUMENT_KEYS
        .iter()
        .find_map(|key| value.get_mut(key).map(Value::take))
        .unwrap_or_else(|| Value::Object(Map::new()));
    match arguments {
        Value::String(encoded) => serde_json::from_str(&encoded)
            .map(|arguments| (name, arguments))
            .map_err(|_| INVALID_JSON),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            Ok((name, arguments))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::fixtures::call;
    use crate::text_codec::tool_call_block;

    fn parse_text(text: &str) -> ParsedReply {
        parse_reply(ToolCodec::Text, text, &[], "s3-")
    }

    fn native_call(id: &str, name: &str, arguments: &str) -> ChatToolCall {
        ChatToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }
    }

    fn one_call(text: &str, name: &str, arguments: Value) -> ParsedReply {
        ParsedReply {
            text: text.to_owned(),
            tool_calls: vec![call("s3-0", name, arguments)],
            problems: Vec::new(),
        }
    }

    #[test]
    fn text_without_a_tool_call_is_the_visible_text() {
        assert_eq!(
            parse_text("  The parser is in src/parse.rs.\n"),
            ParsedReply {
                text: "The parser is in src/parse.rs.".to_owned(),
                tool_calls: Vec::new(),
                problems: Vec::new(),
            }
        );
    }

    #[test]
    fn the_canonical_block_gives_one_call() {
        let text = "I read the file.\n<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"src/main.rs\"}}\n</tool_call>";
        assert_eq!(
            parse_text(text),
            one_call("I read the file.", "read", json!({ "path": "src/main.rs" }))
        );
    }

    #[test]
    fn several_blocks_give_calls_with_an_index_in_the_id() {
        let text = "<tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"*.rs\"}}</tool_call>\n<tool_call>{\"name\":\"read\",\"arguments\":{\"path\":\"a.rs\"}}</tool_call>";
        assert_eq!(
            parse_text(text),
            ParsedReply {
                text: String::new(),
                tool_calls: vec![
                    call("s3-0", "glob", json!({ "pattern": "*.rs" })),
                    call("s3-1", "read", json!({ "path": "a.rs" })),
                ],
                problems: Vec::new(),
            }
        );
    }

    #[test]
    fn a_missing_closing_tag_at_the_end_is_tolerated() {
        let text =
            "Next.\n<tool_call>\n{\"name\": \"glob\", \"arguments\": {\"pattern\": \"**/*.md\"}}\n";
        assert_eq!(
            parse_text(text),
            one_call("Next.", "glob", json!({ "pattern": "**/*.md" }))
        );
    }

    #[test]
    fn a_missing_closing_tag_before_the_next_block_is_tolerated() {
        let text = "<tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"a\"}}\n<tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"b\"}}";
        assert_eq!(
            parse_text(text).tool_calls,
            [
                call("s3-0", "glob", json!({ "pattern": "a" })),
                call("s3-1", "glob", json!({ "pattern": "b" })),
            ]
        );
    }

    #[test]
    fn a_code_fence_around_the_json_is_tolerated() {
        let with_language = "<tool_call>\n```json\n{\"name\": \"read\", \"arguments\": {\"path\": \"a.rs\"}}\n```\n</tool_call>";
        let without_language = "<tool_call>```{\"name\": \"read\", \"arguments\": {\"path\": \"a.rs\"}}```</tool_call>";
        let expected = one_call("", "read", json!({ "path": "a.rs" }));
        assert_eq!(parse_text(with_language), expected);
        assert_eq!(parse_text(without_language), expected);
    }

    #[test]
    fn parameters_and_args_stand_in_for_arguments() {
        let expected = one_call("", "read", json!({ "path": "a.rs" }));
        assert_eq!(
            parse_text(
                "<tool_call>{\"name\": \"read\", \"parameters\": {\"path\": \"a.rs\"}}</tool_call>"
            ),
            expected
        );
        assert_eq!(
            parse_text(
                "<tool_call>{\"name\": \"read\", \"args\": {\"path\": \"a.rs\"}}</tool_call>"
            ),
            expected
        );
    }

    #[test]
    fn arguments_as_a_json_encoded_string_are_decoded() {
        let text = r#"<tool_call>{"name": "read", "arguments": "{\"path\": \"a.rs\", \"limit\": 50}"}</tool_call>"#;
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "a.rs", "limit": 50 }))
        );
    }

    #[test]
    fn whitespace_and_newlines_anywhere_are_tolerated() {
        let text = "\n\n<tool_call>\n\n  {\n  \"name\" :\n \"read\" ,\n\t\"arguments\" : {\n \"path\" : \"a.rs\"\n }\n }\n\n </tool_call>\n\n";
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "a.rs" }))
        );
    }

    #[test]
    fn a_bare_json_object_as_the_whole_reply_is_a_call() {
        let expected = one_call("", "read", json!({ "path": "a.rs" }));
        assert_eq!(
            parse_text("\n{\"name\": \"read\", \"arguments\": {\"path\": \"a.rs\"}}\n"),
            expected
        );
        assert_eq!(
            parse_text("```json\n{\"name\": \"read\", \"arguments\": {\"path\": \"a.rs\"}}\n```"),
            expected
        );
    }

    #[test]
    fn json_without_the_call_shape_is_visible_text() {
        let cases = [
            r#"{"name": "local-ide", "version": "0.1.0"}"#,
            r#"{"arguments": {"path": "a.rs"}}"#,
            r#"Use {"name": "read", "arguments": {"path": "a.rs"}} to read."#,
            r#"["read"]"#,
        ];
        for text in cases {
            assert_eq!(
                parse_text(text),
                ParsedReply {
                    text: text.to_owned(),
                    tool_calls: Vec::new(),
                    problems: Vec::new(),
                }
            );
        }
    }

    #[test]
    fn a_block_without_arguments_gives_an_empty_object() {
        assert_eq!(
            parse_text("<tool_call>{\"name\": \"glob\"}</tool_call>"),
            one_call("", "glob", json!({}))
        );
    }

    #[test]
    fn json_that_does_not_parse_gives_a_problem_line() {
        let cut_off = "I write the file.\n<tool_call>\n{\"name\": \"write\", \"arguments\": {\"path\": \"a.rs\", \"content\": \"fn ma";
        assert_eq!(
            parse_text(cut_off),
            ParsedReply {
                text: "I write the file.".to_owned(),
                tool_calls: Vec::new(),
                problems: vec![
                    "error: tool call is not valid JSON. Write <tool_call>{\"name\": ..., \"arguments\": {...}}</tool_call>"
                        .to_owned()
                ],
            }
        );
    }

    #[test]
    fn an_encoded_argument_string_that_does_not_parse_gives_a_problem_line() {
        assert_eq!(
            parse_text("<tool_call>{\"name\": \"read\", \"arguments\": \"path=a.rs\"}</tool_call>")
                .problems,
            [
                "error: tool call is not valid JSON. Write <tool_call>{\"name\": ..., \"arguments\": {...}}</tool_call>"
            ]
        );
    }

    #[test]
    fn a_block_without_a_name_gives_a_problem_line() {
        assert_eq!(
            parse_text("<tool_call>{\"path\": \"a.rs\"}</tool_call>"),
            ParsedReply {
                text: String::new(),
                tool_calls: Vec::new(),
                problems: vec![
                    "error: tool call has no name. Write <tool_call>{\"name\": ..., \"arguments\": {...}}</tool_call>"
                        .to_owned()
                ],
            }
        );
    }

    #[test]
    fn a_valid_block_after_an_invalid_block_keeps_a_gapless_id() {
        let reply = parse_text(
            "<tool_call>{broken</tool_call><tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"a\"}}</tool_call>",
        );
        assert_eq!(
            reply.tool_calls,
            [call("s3-0", "glob", json!({ "pattern": "a" }))]
        );
        assert_eq!(reply.problems.len(), 1);
    }

    #[test]
    fn text_after_a_block_is_dropped() {
        let text = "Before.\n<tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"a\"}}</tool_call>\nThe result is 3 files.";
        assert_eq!(
            parse_text(text),
            one_call("Before.", "glob", json!({ "pattern": "a" }))
        );
    }

    #[test]
    fn multi_byte_text_around_a_block_is_kept() {
        let text = "Ich lese die Datei größe.rs 日本 🦀\n<tool_call>{\"name\":\"read\",\"arguments\":{\"path\":\"größe.rs\"}}</tool_call>";
        assert_eq!(
            parse_text(text),
            one_call(
                "Ich lese die Datei größe.rs 日本 🦀",
                "read",
                json!({ "path": "größe.rs" })
            )
        );
    }

    #[test]
    fn the_argument_tag_form_with_newlines_gives_the_json_call() {
        let text = "<tool_call>read\n<arg_key>path</arg_key>\n<arg_value>README.md</arg_value>\n</tool_call>";
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "README.md" }))
        );
    }

    #[test]
    fn the_argument_tag_form_on_one_line_gives_the_json_call() {
        let text =
            "<tool_call>read<arg_key>path</arg_key><arg_value>README.md</arg_value></tool_call>";
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "README.md" }))
        );
    }

    #[test]
    fn an_argument_tag_call_writes_back_as_the_json_block() {
        let reply = parse_text(
            "<tool_call>read<arg_key>path</arg_key><arg_value>README.md</arg_value></tool_call>",
        );
        assert_eq!(
            tool_call_block(&reply.tool_calls[0]),
            "<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"README.md\"}}\n</tool_call>"
        );
    }

    #[test]
    fn argument_tag_values_take_the_declared_parameter_kind() {
        let read = "<tool_call>read<arg_key>path</arg_key><arg_value>2026</arg_value><arg_key>limit</arg_key><arg_value> 50 </arg_value></tool_call>";
        assert_eq!(
            parse_text(read),
            one_call("", "read", json!({ "path": "2026", "limit": 50 }))
        );
        let edit = "<tool_call>edit\n<arg_key>path</arg_key>\n<arg_value>a.rs</arg_value>\n<arg_key>old</arg_key>\n<arg_value> x\n</arg_value>\n<arg_key>new</arg_key>\n<arg_value>true</arg_value>\n<arg_key>all</arg_key>\n<arg_value>true</arg_value>\n</tool_call>";
        assert_eq!(
            parse_text(edit),
            one_call(
                "",
                "edit",
                json!({ "path": "a.rs", "old": " x\n", "new": "true", "all": true })
            )
        );
    }

    #[test]
    fn argument_tag_values_outside_the_declared_kind_stay_text() {
        let text = "<tool_call>read<arg_key>path</arg_key><arg_value>a.rs</arg_value><arg_key>limit</arg_key><arg_value>all</arg_value></tool_call>";
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "a.rs", "limit": "all" }))
        );
        let unknown =
            "<tool_call>shell<arg_key>count</arg_key><arg_value>3</arg_value></tool_call>";
        assert_eq!(
            parse_text(unknown),
            one_call("", "shell", json!({ "count": "3" }))
        );
    }

    #[test]
    fn a_broken_argument_tag_form_gives_a_problem_line() {
        let cases = [
            "<tool_call>read<arg_key>path</arg_key>README.md</tool_call>",
            "<tool_call>read<arg_key>path</arg_key><arg_value>README.md</tool_call>",
            "<tool_call>read<arg_key>path</arg_key><arg_value>a</arg_value> extra</tool_call>",
            "<tool_call><arg_key>path</arg_key><arg_value>a</arg_value></tool_call>",
            "<tool_call>read the file<arg_key>path</arg_key><arg_value>a</arg_value></tool_call>",
        ];
        for text in cases {
            assert_eq!(
                parse_text(text).problems,
                [
                    "error: tool call is not valid JSON. Write <tool_call>{\"name\": ..., \"arguments\": {...}}</tool_call>"
                ],
                "{text}"
            );
        }
    }

    #[test]
    fn the_unkeyed_name_form_gives_the_json_call() {
        let text =
            "<tool_call>\n{\"read\", \"arguments\": {\"path\": \"README.md\"}}\n</tool_call>";
        assert_eq!(
            parse_text(text),
            one_call("", "read", json!({ "path": "README.md" }))
        );
    }

    #[test]
    fn an_unkeyed_name_without_an_argument_key_gives_a_problem_line() {
        let cases = [
            "<tool_call>{\"read\", \"path\": \"README.md\"}</tool_call>",
            "<tool_call>{\"read\", \"name\": \"glob\", \"arguments\": {}}</tool_call>",
            "<tool_call>{\"read\" \"arguments\": {}}</tool_call>",
        ];
        for text in cases {
            assert_eq!(parse_text(text).problems.len(), 1, "{text}");
            assert_eq!(parse_text(text).tool_calls, [], "{text}");
        }
    }

    #[test]
    fn native_calls_keep_their_id_and_parse_their_arguments() {
        let reply = parse_reply(
            ToolCodec::Native,
            " I read two files. ",
            &[
                native_call("call_a", "read", r#"{"path": "a.rs"}"#),
                native_call("call_b", "read", "  {\"path\":\"b.rs\",\"limit\":10}\n"),
            ],
            "unused-",
        );
        assert_eq!(
            reply,
            ParsedReply {
                text: "I read two files.".to_owned(),
                tool_calls: vec![
                    call("call_a", "read", json!({ "path": "a.rs" })),
                    call("call_b", "read", json!({ "path": "b.rs", "limit": 10 })),
                ],
                problems: Vec::new(),
            }
        );
    }

    #[test]
    fn an_empty_native_argument_string_means_an_empty_object() {
        let reply = parse_reply(
            ToolCodec::Native,
            "",
            &[native_call("call_a", "glob", "")],
            "",
        );
        assert_eq!(reply.tool_calls, [call("call_a", "glob", json!({}))]);
        assert_eq!(reply.problems, Vec::<String>::new());
    }

    #[test]
    fn invalid_native_arguments_give_a_problem_line() {
        let reply = parse_reply(
            ToolCodec::Native,
            "",
            &[
                native_call("call_a", "read", r#"{"path": "a.rs"#),
                native_call("call_b", "glob", r#"{"pattern": "*"}"#),
            ],
            "",
        );
        assert_eq!(
            reply.tool_calls,
            [call("call_b", "glob", json!({ "pattern": "*" }))]
        );
        assert_eq!(
            reply.problems,
            ["error: invalid arguments for read: not valid JSON"]
        );
    }

    #[test]
    fn the_native_codec_treats_the_tag_as_ordinary_text() {
        let text = "<tool_call>{\"name\":\"glob\",\"arguments\":{\"pattern\":\"a\"}}</tool_call>";
        assert_eq!(
            parse_reply(ToolCodec::Native, text, &[], ""),
            ParsedReply {
                text: text.to_owned(),
                tool_calls: Vec::new(),
                problems: Vec::new(),
            }
        );
    }
}
