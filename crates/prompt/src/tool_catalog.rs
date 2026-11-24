use protocol::{Mode, ToolName};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::system_prompt::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParameterKind {
    String { non_empty: bool },
    Integer { minimum: i64, maximum: i64 },
    Boolean,
}

impl ParameterKind {
    const fn json_type(self) -> &'static str {
        match self {
            Self::String { .. } => "string",
            Self::Integer { .. } => "integer",
            Self::Boolean => "boolean",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub name: &'static str,
    pub kind: ParameterKind,
    pub required: bool,
}

// The single source of the text tool list, the native tool schema, and the argument checks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolSpecification {
    pub description: &'static str,
    pub parameters: &'static [Parameter],
}

const FREE_TEXT: ParameterKind = ParameterKind::String { non_empty: false };
const NON_EMPTY_TEXT: ParameterKind = ParameterKind::String { non_empty: true };

const fn required(name: &'static str, kind: ParameterKind) -> Parameter {
    Parameter {
        name,
        kind,
        required: true,
    }
}

const fn optional(name: &'static str, kind: ParameterKind) -> Parameter {
    Parameter {
        name,
        kind,
        required: false,
    }
}

const READ: ToolSpecification = ToolSpecification {
    description: "Read a file with line numbers, 200 lines per call.",
    parameters: &[
        required("path", NON_EMPTY_TEXT),
        optional(
            "offset",
            ParameterKind::Integer {
                minimum: 1,
                maximum: u32::MAX as i64,
            },
        ),
        optional(
            "limit",
            ParameterKind::Integer {
                minimum: 1,
                maximum: 400,
            },
        ),
    ],
};

const GLOB: ToolSpecification = ToolSpecification {
    description: "List the file paths that match a glob pattern, for example src/**/*.rs.",
    parameters: &[required("pattern", NON_EMPTY_TEXT)],
};

const GREP: ToolSpecification = ToolSpecification {
    description: "Search file content by regular expression; path limits it to a file or directory, glob to file names.",
    parameters: &[
        required("pattern", NON_EMPTY_TEXT),
        optional("path", NON_EMPTY_TEXT),
        optional("glob", FREE_TEXT),
    ],
};

const EDIT: ToolSpecification = ToolSpecification {
    description: "Replace the exact text old with new; copy old without line numbers; all=true replaces each match.",
    parameters: &[
        required("path", NON_EMPTY_TEXT),
        required("old", FREE_TEXT),
        required("new", FREE_TEXT),
        optional("all", ParameterKind::Boolean),
    ],
};

const WRITE: ToolSpecification = ToolSpecification {
    description: "Create a file or replace its whole content.",
    parameters: &[
        required("path", NON_EMPTY_TEXT),
        required("content", FREE_TEXT),
    ],
};

const DELEGATE: ToolSpecification = ToolSpecification {
    description: "Give one self-contained task to a sub-agent with a fresh context and get its final answer.",
    parameters: &[required("task", FREE_TEXT), optional("model", FREE_TEXT)],
};

pub(crate) const fn specification(tool: ToolName) -> &'static ToolSpecification {
    match tool {
        ToolName::Read => &READ,
        ToolName::Glob => &GLOB,
        ToolName::Grep => &GREP,
        ToolName::Edit => &EDIT,
        ToolName::Write => &WRITE,
        ToolName::Delegate => &DELEGATE,
    }
}

pub(crate) fn parameter_kind(tool: ToolName, name: &str) -> Option<ParameterKind> {
    specification(tool)
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .map(|parameter| parameter.kind)
}

// Delegate needs no client capability because the engine executes it
pub fn offered_tools(
    mode: Mode,
    capabilities: &[ToolName],
    agent_kind: AgentKind,
) -> Vec<ToolName> {
    mode.tools()
        .iter()
        .copied()
        .filter(|tool| match tool {
            ToolName::Delegate => agent_kind == AgentKind::Primary,
            ToolName::Read | ToolName::Glob | ToolName::Grep | ToolName::Edit | ToolName::Write => {
                capabilities.contains(tool)
            }
        })
        .collect()
}

pub(crate) fn tool_line(tool: ToolName) -> String {
    let specification = specification(tool);
    let parameters: Vec<String> = specification
        .parameters
        .iter()
        .map(|parameter| {
            if parameter.required {
                parameter.name.to_owned()
            } else {
                format!("{}?", parameter.name)
            }
        })
        .collect();
    format!(
        "{}({}): {}",
        tool.as_str(),
        parameters.join(", "),
        specification.description
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDeclaration(pub ToolName);

impl Serialize for ToolDeclaration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let specification = specification(self.0);
        let mut declaration = serializer.serialize_struct("ToolDeclaration", 2)?;
        declaration.serialize_field("type", "function")?;
        declaration.serialize_field(
            "function",
            &FunctionSchema {
                name: self.0.as_str(),
                description: specification.description,
                parameters: ObjectSchema {
                    kind: "object",
                    properties: Properties(specification.parameters),
                    required: specification
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.required)
                        .map(|parameter| parameter.name)
                        .collect(),
                },
            },
        )?;
        declaration.end()
    }
}

#[derive(Serialize)]
struct FunctionSchema {
    name: &'static str,
    description: &'static str,
    parameters: ObjectSchema,
}

#[derive(Serialize)]
struct ObjectSchema {
    #[serde(rename = "type")]
    kind: &'static str,
    properties: Properties,
    required: Vec<&'static str>,
}

// A typed map keeps the parameter order of the specification, a serde_json map can sort its keys
struct Properties(&'static [Parameter]);

impl Serialize for Properties {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Property {
            #[serde(rename = "type")]
            kind: &'static str,
        }

        let mut properties = serializer.serialize_map(Some(self.0.len()))?;
        for parameter in self.0 {
            properties.serialize_entry(
                parameter.name,
                &Property {
                    kind: parameter.kind.json_type(),
                },
            )?;
        }
        properties.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::WORKSPACE_TOOLS;

    #[test]
    fn agent_mode_offers_the_capabilities_and_delegate() {
        assert_eq!(
            offered_tools(Mode::Agent, &WORKSPACE_TOOLS, AgentKind::Primary),
            [
                ToolName::Read,
                ToolName::Glob,
                ToolName::Grep,
                ToolName::Edit,
                ToolName::Write,
                ToolName::Delegate,
            ]
        );
    }

    #[test]
    fn a_tool_outside_the_capabilities_is_not_offered() {
        assert_eq!(
            offered_tools(
                Mode::Agent,
                &[ToolName::Write, ToolName::Read],
                AgentKind::Primary
            ),
            [ToolName::Read, ToolName::Write, ToolName::Delegate]
        );
        assert_eq!(
            offered_tools(Mode::Agent, &[], AgentKind::Primary),
            [ToolName::Delegate]
        );
    }

    #[test]
    fn ask_mode_offers_the_read_only_tools() {
        assert_eq!(
            offered_tools(Mode::Ask, &WORKSPACE_TOOLS, AgentKind::Primary),
            [ToolName::Read, ToolName::Glob, ToolName::Grep]
        );
        assert_eq!(
            offered_tools(Mode::Ask, &[ToolName::Delegate], AgentKind::Primary),
            []
        );
    }

    #[test]
    fn a_delegate_child_never_gets_delegate() {
        assert_eq!(
            offered_tools(Mode::Agent, &ToolName::ALL, AgentKind::DelegateChild),
            WORKSPACE_TOOLS
        );
    }

    #[test]
    fn tool_lines_are_a_signature_and_one_sentence() {
        let lines: Vec<String> = ToolName::ALL.into_iter().map(tool_line).collect();
        assert_eq!(
            lines,
            [
                "read(path, offset?, limit?): Read a file with line numbers, 200 lines per call.",
                "glob(pattern): List the file paths that match a glob pattern, for example src/**/*.rs.",
                "grep(pattern, path?, glob?): Search file content by regular expression; path limits it to a file or directory, glob to file names.",
                "edit(path, old, new, all?): Replace the exact text old with new; copy old without line numbers; all=true replaces each match.",
                "write(path, content): Create a file or replace its whole content.",
                "delegate(task, model?): Give one self-contained task to a sub-agent with a fresh context and get its final answer.",
            ]
        );
    }

    #[test]
    fn each_tool_line_is_one_line_under_140_characters() {
        for tool in ToolName::ALL {
            let line = tool_line(tool);
            assert!(line.chars().count() < 140, "{line}");
            assert!(!line.contains('\n'), "{line}");
        }
    }

    #[test]
    fn native_declaration_wire_json() {
        assert_eq!(
            serde_json::to_string(&ToolDeclaration(ToolName::Read)).expect("a declaration encodes"),
            r#"{"type":"function","function":{"name":"read","description":"Read a file with line numbers, 200 lines per call.","parameters":{"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer"},"limit":{"type":"integer"}},"required":["path"]}}}"#
        );
        assert_eq!(
            serde_json::to_string(&ToolDeclaration(ToolName::Edit)).expect("a declaration encodes"),
            r#"{"type":"function","function":{"name":"edit","description":"Replace the exact text old with new; copy old without line numbers; all=true replaces each match.","parameters":{"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"},"all":{"type":"boolean"}},"required":["path","old","new"]}}}"#
        );
    }

    #[test]
    fn native_tools_json_with_all_six_tools_fits_2400_characters() {
        let declarations: Vec<ToolDeclaration> =
            ToolName::ALL.into_iter().map(ToolDeclaration).collect();
        let size = serde_json::to_string(&declarations)
            .expect("the declarations encode")
            .chars()
            .count();
        assert_eq!(size, 1654);
        assert!(size <= 2400);
    }
}
