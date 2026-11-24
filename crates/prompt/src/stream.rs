use protocol::{DeltaChannel, ToolCodec};

use crate::text_codec::TOOL_CALL_OPEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSegment {
    pub channel: DeltaChannel,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitStream {
    pub text: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    ThinkOpen,
    ThinkClose,
    ToolCallOpen,
}

impl Tag {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ThinkOpen => "<think>",
            Self::ThinkClose => "</think>",
            Self::ToolCallOpen => TOOL_CALL_OPEN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Text,
    Reasoning,
    // From the first tool call tag to the end, kept for the parser and not forwarded
    ToolCalls,
}

#[derive(Debug, Clone)]
pub struct StreamSplitter {
    codec: ToolCodec,
    region: Region,
    // Received text that can still become a tag
    held: String,
    text: String,
    reasoning: String,
}

impl StreamSplitter {
    pub const fn new(codec: ToolCodec) -> Self {
        Self {
            codec,
            region: Region::Text,
            held: String::new(),
            text: String::new(),
            reasoning: String::new(),
        }
    }

    pub fn push(&mut self, delta: &str) -> Vec<StreamSegment> {
        self.held.push_str(delta);
        let mut segments = Vec::new();
        loop {
            let tags = self.expected_tags();
            let first_tag = tags
                .iter()
                .filter_map(|tag| self.held.find(tag.as_str()).map(|start| (start, *tag)))
                .min_by_key(|(start, _)| *start);
            let Some((start, tag)) = first_tag else {
                let ready = partial_tag_start(&self.held, tags);
                self.release(ready, &mut segments);
                return segments;
            };
            self.release(start, &mut segments);
            match tag {
                Tag::ThinkOpen => {
                    self.held.drain(..tag.as_str().len());
                    self.region = Region::Reasoning;
                }
                Tag::ThinkClose => {
                    self.held.drain(..tag.as_str().len());
                    self.region = Region::Text;
                }
                Tag::ToolCallOpen => self.region = Region::ToolCalls,
            }
        }
    }

    pub fn finish(mut self) -> SplitStream {
        match self.region {
            Region::Text | Region::ToolCalls => self.text.push_str(&self.held),
            Region::Reasoning => self.reasoning.push_str(&self.held),
        }
        SplitStream {
            text: self.text,
            reasoning: self.reasoning,
        }
    }

    const fn expected_tags(&self) -> &'static [Tag] {
        match (self.region, self.codec) {
            (Region::Text, ToolCodec::Text) => &[Tag::ThinkOpen, Tag::ToolCallOpen],
            (Region::Text, ToolCodec::Native) => &[Tag::ThinkOpen],
            (Region::Reasoning, ToolCodec::Text | ToolCodec::Native) => &[Tag::ThinkClose],
            (Region::ToolCalls, ToolCodec::Text | ToolCodec::Native) => &[],
        }
    }

    fn release(&mut self, length: usize, segments: &mut Vec<StreamSegment>) {
        let released: String = self.held.drain(..length).collect();
        if released.is_empty() {
            return;
        }
        let channel = match self.region {
            Region::Text => {
                self.text.push_str(&released);
                DeltaChannel::Text
            }
            Region::Reasoning => {
                self.reasoning.push_str(&released);
                DeltaChannel::Reasoning
            }
            Region::ToolCalls => {
                self.text.push_str(&released);
                return;
            }
        };
        match segments.last_mut() {
            Some(last) if last.channel == channel => last.text.push_str(&released),
            Some(_) | None => segments.push(StreamSegment {
                channel,
                text: released,
            }),
        }
    }
}

// A tag holds one `<`, at its start, so only the text from the last `<` can become a tag
fn partial_tag_start(text: &str, tags: &[Tag]) -> usize {
    match text.rfind('<') {
        Some(start)
            if tags
                .iter()
                .any(|tag| tag.as_str().starts_with(&text[start..])) =>
        {
            start
        }
        Some(_) | None => text.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Outcome {
        forwarded_text: String,
        forwarded_reasoning: String,
        stream: SplitStream,
    }

    fn split(codec: ToolCodec, chunks: &[&str]) -> Outcome {
        let mut splitter = StreamSplitter::new(codec);
        let mut forwarded_text = String::new();
        let mut forwarded_reasoning = String::new();
        for chunk in chunks {
            for segment in splitter.push(chunk) {
                match segment.channel {
                    DeltaChannel::Text => forwarded_text.push_str(&segment.text),
                    DeltaChannel::Reasoning => forwarded_reasoning.push_str(&segment.text),
                }
            }
        }
        Outcome {
            forwarded_text,
            forwarded_reasoning,
            stream: splitter.finish(),
        }
    }

    fn split_by_character(codec: ToolCodec, text: &str) -> Outcome {
        let characters: Vec<String> = text.chars().map(String::from).collect();
        let chunks: Vec<&str> = characters.iter().map(String::as_str).collect();
        split(codec, &chunks)
    }

    fn segment(channel: DeltaChannel, text: &str) -> StreamSegment {
        StreamSegment {
            channel,
            text: text.to_owned(),
        }
    }

    const FULL_REPLY: &str = "<think>Der Plan: größe prüfen 日本 🦀</think>\nIch lese die Datei größe.rs 🦀.\n<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"größe.rs\"}}\n</tool_call>\n<think>not reasoning</think>";

    fn full_reply_outcome() -> Outcome {
        Outcome {
            forwarded_text: "\nIch lese die Datei größe.rs 🦀.\n".to_owned(),
            forwarded_reasoning: "Der Plan: größe prüfen 日本 🦀".to_owned(),
            stream: SplitStream {
                text: "\nIch lese die Datei größe.rs 🦀.\n<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"größe.rs\"}}\n</tool_call>\n<think>not reasoning</think>".to_owned(),
                reasoning: "Der Plan: größe prüfen 日本 🦀".to_owned(),
            },
        }
    }

    #[test]
    fn one_chunk_splits_into_ordered_segments() {
        let mut splitter = StreamSplitter::new(ToolCodec::Text);
        assert_eq!(
            splitter.push("a<think>b</think>c<think>d</think><tool_call>e"),
            [
                segment(DeltaChannel::Text, "a"),
                segment(DeltaChannel::Reasoning, "b"),
                segment(DeltaChannel::Text, "c"),
                segment(DeltaChannel::Reasoning, "d"),
            ]
        );
        assert_eq!(splitter.push("f</tool_call>g"), []);
        assert_eq!(
            splitter.finish(),
            SplitStream {
                text: "ac<tool_call>ef</tool_call>g".to_owned(),
                reasoning: "bd".to_owned(),
            }
        );
    }

    #[test]
    fn one_chunk_gives_the_expected_result() {
        assert_eq!(split(ToolCodec::Text, &[FULL_REPLY]), full_reply_outcome());
    }

    #[test]
    fn one_character_at_a_time_gives_the_same_result_as_one_chunk() {
        assert_eq!(
            split_by_character(ToolCodec::Text, FULL_REPLY),
            full_reply_outcome()
        );
    }

    #[test]
    fn each_two_chunk_cut_on_a_character_boundary_gives_the_same_result() {
        for (cut, _) in FULL_REPLY.char_indices() {
            let (first, second) = FULL_REPLY.split_at(cut);
            assert_eq!(
                split(ToolCodec::Text, &[first, second]),
                full_reply_outcome(),
                "cut at byte {cut}"
            );
        }
    }

    #[test]
    fn only_the_shortest_suffix_that_can_become_a_tag_is_held_back() {
        let mut splitter = StreamSplitter::new(ToolCodec::Text);
        assert_eq!(
            splitter.push("x < y <to"),
            [segment(DeltaChannel::Text, "x < y ")]
        );
        assert_eq!(
            splitter.push("p> z <"),
            [segment(DeltaChannel::Text, "<top> z ")]
        );
        assert_eq!(splitter.push("<thi"), [segment(DeltaChannel::Text, "<")]);
        assert_eq!(
            splitter.push("nk>r</thin"),
            [segment(DeltaChannel::Reasoning, "r")]
        );
        assert_eq!(
            splitter.push("g"),
            [segment(DeltaChannel::Reasoning, "</thing")]
        );
        assert_eq!(
            splitter.finish(),
            SplitStream {
                text: "x < y <top> z <".to_owned(),
                reasoning: "r</thing".to_owned(),
            }
        );
    }

    #[test]
    fn a_partial_tag_at_the_end_of_the_stream_is_text() {
        let mut splitter = StreamSplitter::new(ToolCodec::Text);
        assert_eq!(
            splitter.push("a <tool_c"),
            [segment(DeltaChannel::Text, "a ")]
        );
        assert_eq!(
            splitter.finish(),
            SplitStream {
                text: "a <tool_c".to_owned(),
                reasoning: String::new(),
            }
        );
    }

    #[test]
    fn an_open_think_block_at_the_end_of_the_stream_is_reasoning() {
        let outcome = split(ToolCodec::Text, &["a<think>b", "c</thi"]);
        assert_eq!(
            outcome,
            Outcome {
                forwarded_text: "a".to_owned(),
                forwarded_reasoning: "bc".to_owned(),
                stream: SplitStream {
                    text: "a".to_owned(),
                    reasoning: "bc</thi".to_owned(),
                },
            }
        );
    }

    #[test]
    fn the_native_codec_forwards_the_tool_call_tag_as_text() {
        let text = "<think>p</think>a <tool_call>b</tool_call> c";
        let expected = Outcome {
            forwarded_text: "a <tool_call>b</tool_call> c".to_owned(),
            forwarded_reasoning: "p".to_owned(),
            stream: SplitStream {
                text: "a <tool_call>b</tool_call> c".to_owned(),
                reasoning: "p".to_owned(),
            },
        };
        assert_eq!(split(ToolCodec::Native, &[text]), expected);
        assert_eq!(split_by_character(ToolCodec::Native, text), expected);
    }

    #[test]
    fn an_argument_tag_call_stays_out_of_the_text_channel() {
        let text = "I read it.\n<tool_call>read\n<arg_key>path</arg_key>\n<arg_value>README.md</arg_value>\n</tool_call>";
        assert_eq!(
            split_by_character(ToolCodec::Text, text),
            Outcome {
                forwarded_text: "I read it.\n".to_owned(),
                forwarded_reasoning: String::new(),
                stream: SplitStream {
                    text: "I read it.\n<tool_call>read\n<arg_key>path</arg_key>\n<arg_value>README.md</arg_value>\n</tool_call>".to_owned(),
                    reasoning: String::new(),
                },
            }
        );
    }

    #[test]
    fn an_unkeyed_name_call_stays_out_of_the_text_channel() {
        let text =
            "<tool_call>\n{\"read\", \"arguments\": {\"path\": \"README.md\"}}\n</tool_call>";
        assert_eq!(
            split_by_character(ToolCodec::Text, text),
            Outcome {
                forwarded_text: String::new(),
                forwarded_reasoning: String::new(),
                stream: SplitStream {
                    text: "<tool_call>\n{\"read\", \"arguments\": {\"path\": \"README.md\"}}\n</tool_call>".to_owned(),
                    reasoning: String::new(),
                },
            }
        );
    }

    #[test]
    fn an_empty_delta_gives_no_segment() {
        let mut splitter = StreamSplitter::new(ToolCodec::Text);
        assert_eq!(splitter.push(""), []);
    }
}
