use protocol::{Event, EventBody};

use crate::conversation::{Entry, View, append_block, assistant_text, conversation, summary_block};
use crate::estimate::{estimate_tokens, message_characters};
use crate::message::ChatMessage;
use crate::render::{PromptInput, compose};

const KEPT_EVENTS: usize = 6;
const MINIMUM_EVENTS: usize = 8;

const SUMMARY_INSTRUCTION: &str = "\
You write a handoff note for another model that continues this coding session. Use only facts from the transcript. Write plain text, under 300 words, with these parts:
Goal: what the user wants.
Decisions and facts: what was decided and what was found.
Files read or changed: each path, with what was done to it.
Current state: what is done and what is open.
Next steps: what to do next.
Do not call tools.";

const DROPPED_NOTE: &str = "[Earlier transcript entries were dropped to fit the context.]";
const SUMMARY_REQUEST: &str = "Write the handoff note now.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionPlan {
    None,
    Elide {
        through: u32,
    },
    Summarize {
        through: u32,
        messages: Vec<ChatMessage>,
    },
}

// The budget is a soft ceiling because the server context is larger, so a session with fewer than 8 events after the latest summary gets no compaction
pub fn plan_compaction(input: &PromptInput) -> CompactionPlan {
    let view = View::of(input.events);
    let ceiling = input
        .profile
        .context_budget
        .saturating_sub(input.profile.reply_reserve);
    if compose(input, &view).prompt.report.total <= ceiling {
        return CompactionPlan::None;
    }
    let turn_events: Vec<&Event> = input
        .events
        .iter()
        .filter(|event| event.seq > view.hidden_through && is_turn_event(&event.body))
        .collect();
    if turn_events.len() < MINIMUM_EVENTS {
        return CompactionPlan::None;
    }
    let kept_start = turn_events.len() - KEPT_EVENTS;

    let elide_through = turn_events[kept_start - 1].seq;
    let elided = View {
        elide_through: view.elide_through.max(elide_through),
        ..view
    };
    // The margin stops a compaction on each turn, which would break the prompt cache each time
    let elided_total = compose(input, &elided).prompt.report.total;
    if u64::from(elided_total) * 10 <= u64::from(ceiling) * 7 {
        return CompactionPlan::Elide {
            through: elide_through,
        };
    }

    let Some(through) = summary_through(&turn_events, kept_start) else {
        return CompactionPlan::None;
    };
    CompactionPlan::Summarize {
        through,
        messages: summary_request(input, &view, through, ceiling),
    }
}

// Compaction and model switch events add no message, so they do not count as kept events
const fn is_turn_event(body: &EventBody) -> bool {
    match body {
        EventBody::User { .. } | EventBody::Assistant { .. } | EventBody::ToolResult { .. } => true,
        EventBody::Compaction { .. } | EventBody::ModelSwitch { .. } => false,
    }
}

// The kept tail grows backward to the assistant turn of a tool result at its start, so the
// latest results stay verbatim next to their calls
fn summary_through(turn_events: &[&Event], kept_start: usize) -> Option<u32> {
    let turn_start = (0..=kept_start)
        .rev()
        .find(|&index| !matches!(turn_events[index].body, EventBody::ToolResult { .. }))?;
    let last_summarized = turn_start.checked_sub(1)?;
    Some(turn_events[last_summarized].seq)
}

fn summary_request(
    input: &PromptInput,
    view: &View,
    through: u32,
    ceiling: u32,
) -> Vec<ChatMessage> {
    let summarized = &input.events[..input.events.partition_point(|event| event.seq <= through)];
    let transcript_view = View {
        hidden_through: view.hidden_through,
        summary: None,
        elide_through: through,
    };
    let entries: Vec<String> = conversation(summarized, &transcript_view)
        .entries
        .iter()
        .map(transcript_entry)
        .collect();

    let mut dropped = 0;
    loop {
        let messages = vec![
            ChatMessage::System {
                content: SUMMARY_INSTRUCTION.to_owned(),
            },
            ChatMessage::User {
                content: transcript(view.summary, &entries[dropped..], dropped > 0),
            },
        ];
        let tokens = estimate_tokens(
            messages.iter().map(message_characters).sum(),
            input.profile.chars_per_token,
            input.token_scale,
        );
        if tokens <= ceiling || dropped == entries.len() {
            return messages;
        }
        dropped += 1;
    }
}

fn transcript_entry(entry: &Entry) -> String {
    match entry {
        Entry::User(content) => format!("User:\n{content}"),
        Entry::Assistant { text, tool_calls } => {
            format!("Assistant:\n{}", assistant_text(text, tool_calls))
        }
        Entry::ToolResult { call, content } => format!("Result of {}:\n{content}", call.name),
    }
}

fn transcript(earlier_summary: Option<&str>, entries: &[String], dropped: bool) -> String {
    let mut content = String::new();
    if let Some(text) = earlier_summary {
        content.push_str(&summary_block(text));
    }
    if dropped {
        append_block(&mut content, DROPPED_NOTE);
    }
    for entry in entries {
        append_block(&mut content, entry);
    }
    append_block(&mut content, SUMMARY_REQUEST);
    content
}

#[cfg(test)]
mod tests {
    use protocol::ToolCall;
    use serde_json::json;

    use super::*;
    use crate::fixtures::{
        assistant, call, elide, summary, tool_result, toolless_input, unit_profile, user,
        user_message,
    };

    const INSTRUCTION: &str = "\
You write a handoff note for another model that continues this coding session. Use only facts from the transcript. Write plain text, under 300 words, with these parts:
Goal: what the user wants.
Decisions and facts: what was decided and what was found.
Files read or changed: each path, with what was done to it.
Current state: what is done and what is open.
Next steps: what to do next.
Do not call tools.";

    fn read_call(id: &str) -> ToolCall {
        call(id, "read", json!({ "path": "src/a.rs" }))
    }

    fn summary_request_with(transcript: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage::System {
                content: INSTRUCTION.to_owned(),
            },
            user_message(transcript),
        ]
    }

    // With the system prompt of 612 characters, this session has 7255 characters, and 1451 characters after an elide through seq 5
    fn session_with_two_long_results() -> Vec<Event> {
        let long_output = "x".repeat(3000);
        vec![
            user(1, "task"),
            assistant(2, "", vec![read_call("c1")]),
            tool_result(3, "c1", "read", &long_output),
            assistant(4, "", vec![read_call("c2")]),
            tool_result(5, "c2", "read", &long_output),
            assistant(6, "", vec![read_call("c3")]),
            tool_result(7, "c3", "read", "1|short"),
            assistant(8, "", vec![read_call("c4")]),
            tool_result(9, "c4", "read", "1|short"),
            assistant(10, "done part", Vec::new()),
            user(11, "next"),
        ]
    }

    const TWO_LONG_RESULTS_TRANSCRIPT: &str = "\
User:
task

Assistant:
<tool_call>
{\"name\": \"read\", \"arguments\": {\"path\": \"src/a.rs\"}}
</tool_call>

Result of read:
[elided result of read {\"path\":\"src/a.rs\"} (3000 characters). Call the tool again if you need it.]

Assistant:
<tool_call>
{\"name\": \"read\", \"arguments\": {\"path\": \"src/a.rs\"}}
</tool_call>

Result of read:
[elided result of read {\"path\":\"src/a.rs\"} (3000 characters). Call the tool again if you need it.]

Write the handoff note now.";

    fn session_with_an_edit() -> Vec<Event> {
        let parse_path = json!({ "path": "src/parse.rs" });
        vec![
            user(1, "Fix the parser."),
            assistant(2, "I look.", vec![call("c1", "read", parse_path.clone())]),
            tool_result(3, "c1", "read", &"p".repeat(900)),
            assistant(
                4,
                "The bug is in line 7.",
                vec![call(
                    "c2",
                    "edit",
                    json!({ "new": "b", "old": "a", "path": "src/parse.rs" }),
                )],
            ),
            tool_result(5, "c2", "edit", "edited src/parse.rs: 1 replacement(s)"),
            user(6, "Now add a test."),
            assistant(7, &"z".repeat(400), Vec::new()),
            user(8, &"u".repeat(400)),
            assistant(9, "", vec![call("c3", "read", parse_path)]),
            tool_result(10, "c3", "read", &"r".repeat(300)),
            assistant(11, &"w".repeat(200), Vec::new()),
            user(12, "go on"),
        ]
    }

    #[test]
    fn summary_instruction_fits_600_characters() {
        assert_eq!(SUMMARY_INSTRUCTION, INSTRUCTION);
        assert_eq!(SUMMARY_INSTRUCTION.chars().count(), 406);
    }

    #[test]
    fn a_session_within_the_budget_needs_no_compaction() {
        let events = session_with_two_long_results();
        let at_the_ceiling = unit_profile(9255, 2000);
        let over_the_ceiling = unit_profile(9254, 2000);
        assert_eq!(
            plan_compaction(&toolless_input(&at_the_ceiling, &events)),
            CompactionPlan::None
        );
        assert_ne!(
            plan_compaction(&toolless_input(&over_the_ceiling, &events)),
            CompactionPlan::None
        );
    }

    #[test]
    fn fewer_than_8_events_give_no_compaction_even_over_the_budget() {
        let events = &session_with_two_long_results()[..7];
        let profile = unit_profile(1000, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, events)),
            CompactionPlan::None
        );
        let eight_events = &session_with_two_long_results()[..8];
        assert_ne!(
            plan_compaction(&toolless_input(&profile, eight_events)),
            CompactionPlan::None
        );
    }

    #[test]
    fn elide_keeps_the_last_6_events_verbatim() {
        let events = session_with_two_long_results();
        let profile = unit_profile(8000, 3000);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Elide { through: 5 }
        );
    }

    #[test]
    fn elide_is_chosen_only_when_it_reaches_70_percent_of_the_ceiling() {
        let events = session_with_two_long_results();
        let reached = unit_profile(2073, 0);
        let missed = unit_profile(2072, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&reached, &events)),
            CompactionPlan::Elide { through: 5 }
        );
        assert_eq!(
            plan_compaction(&toolless_input(&missed, &events)),
            CompactionPlan::Summarize {
                through: 5,
                messages: summary_request_with(TWO_LONG_RESULTS_TRANSCRIPT),
            }
        );
    }

    #[test]
    fn the_token_scale_changes_the_plan() {
        let events = session_with_two_long_results();
        let profile = unit_profile(8000, 0);
        let input = toolless_input(&profile, &events);
        assert_eq!(plan_compaction(&input), CompactionPlan::None);
        let scaled = PromptInput {
            token_scale: 1.5,
            ..input
        };
        assert_eq!(
            plan_compaction(&scaled),
            CompactionPlan::Elide { through: 5 }
        );
    }

    #[test]
    fn an_elide_that_is_recorded_already_is_not_planned_again() {
        let mut events = session_with_two_long_results();
        events.push(elide(12, 5));
        let profile = unit_profile(1400, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 5,
                messages: summary_request_with(TWO_LONG_RESULTS_TRANSCRIPT),
            }
        );
    }

    #[test]
    fn summarize_gives_a_request_with_the_transcript_of_the_summarized_events() {
        let events = session_with_an_edit();
        let profile = unit_profile(2000, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 6,
                messages: summary_request_with(
                    "\
User:
Fix the parser.

Assistant:
I look.
<tool_call>
{\"name\": \"read\", \"arguments\": {\"path\": \"src/parse.rs\"}}
</tool_call>

Result of read:
[elided result of read {\"path\":\"src/parse.rs\"} (900 characters). Call the tool again if you need it.]

Assistant:
The bug is in line 7.
<tool_call>
{\"name\": \"edit\", \"arguments\": {\"new\": \"b\", \"old\": \"a\", \"path\": \"src/parse.rs\"}}
</tool_call>

Result of edit:
edited src/parse.rs: 1 replacement(s)

User:
Now add a test.

Write the handoff note now."
                ),
            }
        );
    }

    #[test]
    fn a_transcript_over_the_ceiling_drops_its_oldest_entries_and_says_so() {
        let events = session_with_an_edit();
        let profile = unit_profile(660, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 6,
                messages: summary_request_with(
                    "\
[Earlier transcript entries were dropped to fit the context.]

Result of edit:
edited src/parse.rs: 1 replacement(s)

User:
Now add a test.

Write the handoff note now."
                ),
            }
        );
    }

    #[test]
    fn the_kept_tail_moves_back_to_the_assistant_turn_of_its_first_tool_result() {
        let output = "r".repeat(300);
        let events = [
            user(1, "u1"),
            assistant(2, "a2", Vec::new()),
            user(3, "u3"),
            assistant(4, "a4", Vec::new()),
            user(5, "u5"),
            assistant(
                6,
                "",
                vec![read_call("c1"), read_call("c2"), read_call("c3")],
            ),
            tool_result(7, "c1", "read", &output),
            tool_result(8, "c2", "read", &output),
            tool_result(9, "c3", "read", &output),
            assistant(10, "", vec![read_call("c4")]),
            tool_result(11, "c4", "read", &output),
            assistant(12, "a12", Vec::new()),
        ];
        let profile = unit_profile(1000, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 5,
                messages: summary_request_with(
                    "User:\nu1\n\nAssistant:\na2\n\nUser:\nu3\n\nAssistant:\na4\n\nUser:\nu5\n\nWrite the handoff note now."
                ),
            }
        );
    }

    #[test]
    fn a_tail_that_is_one_assistant_turn_from_its_start_gives_no_compaction() {
        let output = "r".repeat(300);
        let calls: Vec<ToolCall> = (1..=7)
            .map(|index| read_call(&format!("c{index}")))
            .collect();
        let mut events = vec![
            user(1, "old question"),
            summary(2, 1, "Goal: read each file."),
            assistant(3, "", calls),
        ];
        events.extend(
            (1..=7).map(|index| tool_result(3 + index, &format!("c{index}"), "read", &output)),
        );
        let profile = unit_profile(1000, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::None
        );
    }

    fn session_after_a_summary() -> Vec<Event> {
        vec![
            user(1, "old question"),
            assistant(2, "old answer", Vec::new()),
            summary(3, 2, "Goal: fix the parser."),
            user(4, &"l".repeat(200)),
            assistant(5, "I look.", Vec::new()),
            user(6, "And the tests?"),
            assistant(7, &"a".repeat(300), Vec::new()),
            user(8, &"b".repeat(300)),
            assistant(9, &"c".repeat(300), Vec::new()),
            user(10, &"d".repeat(300)),
            assistant(11, &"e".repeat(300), Vec::new()),
            user(12, &"f".repeat(300)),
        ]
    }

    #[test]
    fn the_transcript_starts_from_the_latest_earlier_summary() {
        let events = session_after_a_summary();
        let profile = unit_profile(1500, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 6,
                messages: summary_request_with(&format!(
                    "\
Summary of the earlier session:
Goal: fix the parser.

User:
{}

Assistant:
I look.

User:
And the tests?

Write the handoff note now.",
                    "l".repeat(200)
                )),
            }
        );
    }

    #[test]
    fn the_earlier_summary_stays_when_transcript_entries_are_dropped() {
        let events = session_after_a_summary();
        let profile = unit_profile(628, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::Summarize {
                through: 6,
                messages: summary_request_with(
                    "\
Summary of the earlier session:
Goal: fix the parser.

[Earlier transcript entries were dropped to fit the context.]

Assistant:
I look.

User:
And the tests?

Write the handoff note now."
                ),
            }
        );
    }

    #[test]
    fn events_before_the_latest_summary_do_not_count_as_events_to_compact() {
        let mut events = vec![user(1, "one"), assistant(2, "two", Vec::new())];
        events.extend((3..=8).map(|seq| user(seq, &"x".repeat(200))));
        events.push(summary(9, 2, "Goal: g."));
        let profile = unit_profile(1000, 0);
        assert_eq!(
            plan_compaction(&toolless_input(&profile, &events)),
            CompactionPlan::None
        );
    }
}
