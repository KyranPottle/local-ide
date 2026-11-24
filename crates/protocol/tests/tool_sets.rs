use protocol::{Mode, ToolName};

const WIRE_NAMES: [(ToolName, &str); 6] = [
    (ToolName::Read, "read"),
    (ToolName::Glob, "glob"),
    (ToolName::Grep, "grep"),
    (ToolName::Edit, "edit"),
    (ToolName::Write, "write"),
    (ToolName::Delegate, "delegate"),
];

#[test]
fn tool_name_agrees_with_its_wire_name() {
    for (tool, name) in WIRE_NAMES {
        assert_eq!(tool.as_str(), name);
        assert_eq!(ToolName::parse(name), Some(tool));
        assert_eq!(
            serde_json::to_value(tool).expect("tool name encodes"),
            serde_json::Value::from(name)
        );
    }
}

#[test]
fn parse_rejects_a_name_outside_the_tool_set() {
    assert_eq!(ToolName::parse("shell"), None);
    assert_eq!(ToolName::parse("Read"), None);
    assert_eq!(ToolName::parse(""), None);
}

#[test]
fn only_delegate_runs_in_the_engine() {
    let engine_tools: Vec<ToolName> = ToolName::ALL
        .into_iter()
        .filter(|tool| !tool.is_workspace_tool())
        .collect();

    assert_eq!(engine_tools, [ToolName::Delegate]);
}

#[test]
fn only_edit_and_write_mutate_the_workspace() {
    let mutating_tools: Vec<ToolName> = ToolName::ALL
        .into_iter()
        .filter(|tool| tool.mutates_workspace())
        .collect();

    assert_eq!(mutating_tools, [ToolName::Edit, ToolName::Write]);
}

#[test]
fn ask_mode_offers_exactly_the_read_only_tools() {
    assert_eq!(
        Mode::Ask.tools(),
        [ToolName::Read, ToolName::Glob, ToolName::Grep]
    );
}

#[test]
fn agent_mode_offers_each_tool() {
    assert_eq!(
        Mode::Agent.tools(),
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
fn mode_wire_names() {
    assert_eq!(serde_json::to_value(Mode::Agent).unwrap(), "agent");
    assert_eq!(serde_json::to_value(Mode::Ask).unwrap(), "ask");
    assert_eq!(serde_json::from_str::<Mode>(r#""ask""#).unwrap(), Mode::Ask);
}
