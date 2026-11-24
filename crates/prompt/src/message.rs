use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ChatToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    // JSON text, in a model reply it can be empty or malformed
    pub arguments: String,
}

impl Serialize for ChatToolCall {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Function<'a> {
            name: &'a str,
            arguments: &'a str,
        }

        let mut call = serializer.serialize_struct("ChatToolCall", 3)?;
        call.serialize_field("id", &self.id)?;
        call.serialize_field("type", "function")?;
        call.serialize_field(
            "function",
            &Function {
                name: &self.name,
                arguments: &self.arguments,
            },
        )?;
        call.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire_json(message: &ChatMessage) -> String {
        serde_json::to_string(message).expect("a chat message encodes")
    }

    #[test]
    fn system_message_wire_json() {
        let message = ChatMessage::System {
            content: "Be brief.".to_owned(),
        };
        assert_eq!(
            wire_json(&message),
            r#"{"role":"system","content":"Be brief."}"#
        );
    }

    #[test]
    fn user_message_wire_json() {
        let message = ChatMessage::User {
            content: "line 1\nline 2".to_owned(),
        };
        assert_eq!(
            wire_json(&message),
            r#"{"role":"user","content":"line 1\nline 2"}"#
        );
    }

    #[test]
    fn assistant_message_without_tool_calls_omits_the_field() {
        let message = ChatMessage::Assistant {
            content: "Done.".to_owned(),
            tool_calls: Vec::new(),
        };
        assert_eq!(
            wire_json(&message),
            r#"{"role":"assistant","content":"Done."}"#
        );
    }

    #[test]
    fn assistant_tool_call_arguments_are_a_json_encoded_string() {
        let message = ChatMessage::Assistant {
            content: String::new(),
            tool_calls: vec![ChatToolCall {
                id: "call_1".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"src/main.rs"}"#.to_owned(),
            }],
        };
        assert_eq!(
            wire_json(&message),
            r#"{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read","arguments":"{\"path\":\"src/main.rs\"}"}}]}"#
        );
    }

    #[test]
    fn tool_message_wire_json() {
        let message = ChatMessage::Tool {
            tool_call_id: "call_1".to_owned(),
            content: "1|fn main() {}".to_owned(),
        };
        assert_eq!(
            wire_json(&message),
            r#"{"role":"tool","tool_call_id":"call_1","content":"1|fn main() {}"}"#
        );
    }
}
