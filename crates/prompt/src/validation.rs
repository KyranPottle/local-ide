use protocol::{ToolCall, ToolName};
use serde_json::Value;

use crate::tool_catalog::{Parameter, ParameterKind, specification};

// The display form is the error line that returns to the model
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolCallError {
    #[error("error: unknown tool: {name}")]
    UnknownTool { name: String },
    #[error("error: invalid arguments for {name}: {reason}")]
    InvalidArguments { name: String, reason: String },
}

// Path safety stays with the browser, which executes the workspace tools
pub fn validate_tool_call(
    call: &ToolCall,
    offered: &[ToolName],
) -> Result<ToolName, ToolCallError> {
    let Some(tool) = ToolName::parse(&call.name).filter(|tool| offered.contains(tool)) else {
        return Err(ToolCallError::UnknownTool {
            name: call.name.clone(),
        });
    };
    let invalid = |reason: String| ToolCallError::InvalidArguments {
        name: call.name.clone(),
        reason,
    };
    let Some(arguments) = call.arguments.as_object() else {
        return Err(invalid("arguments must be a JSON object".to_owned()));
    };
    for parameter in specification(tool).parameters {
        match arguments.get(parameter.name) {
            Some(value) => check_value(parameter, value).map_err(invalid)?,
            None if parameter.required => {
                return Err(invalid(format!("missing {}", parameter.name)));
            }
            None => {}
        }
    }
    Ok(tool)
}

fn check_value(parameter: &Parameter, value: &Value) -> Result<(), String> {
    let name = parameter.name;
    match parameter.kind {
        ParameterKind::String { non_empty } => {
            let Some(text) = value.as_str() else {
                return Err(format!("{name} must be a string"));
            };
            if non_empty && text.is_empty() {
                return Err(format!("{name} must not be empty"));
            }
        }
        ParameterKind::Integer { minimum, maximum } => {
            let Some(number) = value.as_i64() else {
                return Err(format!("{name} must be an integer"));
            };
            if number < minimum {
                return Err(format!("{name} must be at least {minimum}"));
            }
            if number > maximum {
                return Err(format!("{name} must be at most {maximum}"));
            }
        }
        ParameterKind::Boolean => {
            if !value.is_boolean() {
                return Err(format!("{name} must be a boolean"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::fixtures::call;

    fn error_line(name: &str, arguments: Value) -> String {
        validate_tool_call(&call("c1", name, arguments), &ToolName::ALL)
            .expect_err("the call is invalid")
            .to_string()
    }

    #[test]
    fn a_valid_call_gives_the_typed_tool_name() {
        let cases = [
            (
                "read",
                json!({ "path": "src/a.rs", "offset": 201, "limit": 400 }),
                ToolName::Read,
            ),
            ("glob", json!({ "pattern": "**/*.rs" }), ToolName::Glob),
            (
                "grep",
                json!({ "pattern": "fn main", "path": "src", "glob": "*.rs" }),
                ToolName::Grep,
            ),
            (
                "edit",
                json!({ "path": "a", "old": "x", "new": "", "all": true }),
                ToolName::Edit,
            ),
            (
                "write",
                json!({ "path": "a", "content": "" }),
                ToolName::Write,
            ),
            (
                "delegate",
                json!({ "task": "Find the parser", "model": "fast" }),
                ToolName::Delegate,
            ),
        ];
        for (name, arguments, expected) in cases {
            assert_eq!(
                validate_tool_call(&call("c1", name, arguments), &ToolName::ALL),
                Ok(expected),
                "{name}"
            );
        }
    }

    #[test]
    fn an_unknown_name_is_rejected() {
        assert_eq!(
            error_line("shell", json!({ "command": "ls" })),
            "error: unknown tool: shell"
        );
    }

    #[test]
    fn a_known_tool_that_is_not_offered_is_rejected() {
        let result = validate_tool_call(
            &call("c1", "write", json!({ "path": "a", "content": "b" })),
            &[ToolName::Read, ToolName::Glob, ToolName::Grep],
        );
        assert_eq!(
            result,
            Err(ToolCallError::UnknownTool {
                name: "write".to_owned()
            })
        );
        assert_eq!(
            result.expect_err("the tool is not offered").to_string(),
            "error: unknown tool: write"
        );
    }

    #[test]
    fn invalid_arguments_give_one_error_line_with_the_reason() {
        let cases = [
            (
                "read",
                json!({}),
                "error: invalid arguments for read: missing path",
            ),
            (
                "edit",
                json!({ "path": "a", "old": "x" }),
                "error: invalid arguments for edit: missing new",
            ),
            (
                "read",
                json!("src/a.rs"),
                "error: invalid arguments for read: arguments must be a JSON object",
            ),
            (
                "read",
                json!({ "path": 7 }),
                "error: invalid arguments for read: path must be a string",
            ),
            (
                "read",
                json!({ "path": "a", "limit": "50" }),
                "error: invalid arguments for read: limit must be an integer",
            ),
            (
                "read",
                json!({ "path": "a", "offset": 1.5 }),
                "error: invalid arguments for read: offset must be an integer",
            ),
            (
                "read",
                json!({ "path": "a", "offset": null }),
                "error: invalid arguments for read: offset must be an integer",
            ),
            (
                "edit",
                json!({ "path": "a", "old": "x", "new": "y", "all": "true" }),
                "error: invalid arguments for edit: all must be a boolean",
            ),
            (
                "read",
                json!({ "path": "a", "limit": 0 }),
                "error: invalid arguments for read: limit must be at least 1",
            ),
            (
                "read",
                json!({ "path": "a", "offset": 0 }),
                "error: invalid arguments for read: offset must be at least 1",
            ),
            (
                "read",
                json!({ "path": "" }),
                "error: invalid arguments for read: path must not be empty",
            ),
            (
                "glob",
                json!({ "pattern": "" }),
                "error: invalid arguments for glob: pattern must not be empty",
            ),
            (
                "grep",
                json!({ "pattern": "x", "path": "" }),
                "error: invalid arguments for grep: path must not be empty",
            ),
        ];
        for (name, arguments, expected) in cases {
            assert_eq!(error_line(name, arguments), expected);
        }
    }

    #[test]
    fn an_unsafe_path_passes_because_the_browser_owns_path_safety() {
        assert_eq!(
            validate_tool_call(
                &call("c1", "read", json!({ "path": "../secret" })),
                &ToolName::ALL
            ),
            Ok(ToolName::Read)
        );
    }
}
