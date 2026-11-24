use crate::message::ChatMessage;

// Fixed estimate of the role markers that the chat template adds to each message
pub(crate) const MESSAGE_OVERHEAD_CHARACTERS: usize = 16;

const MINIMUM_TOKEN_SCALE: f32 = 0.5;
const MAXIMUM_TOKEN_SCALE: f32 = 2.0;

pub(crate) fn message_characters(message: &ChatMessage) -> usize {
    let visible = match message {
        ChatMessage::System { content }
        | ChatMessage::User { content }
        | ChatMessage::Tool { content, .. } => content.chars().count(),
        ChatMessage::Assistant {
            content,
            tool_calls,
        } => {
            content.chars().count()
                + tool_calls
                    .iter()
                    .map(|call| call.name.chars().count() + call.arguments.chars().count())
                    .sum::<usize>()
        }
    };
    visible + MESSAGE_OVERHEAD_CHARACTERS
}

pub(crate) fn estimate_tokens(characters: usize, chars_per_token: f32, scale: f32) -> u32 {
    (characters as f64 / f64::from(chars_per_token) * f64::from(scale)).ceil() as u32
}

// prompt_tokens is 0 when the server reported no usage, and the scale then stays
pub fn calibrate_token_scale(scale: f32, unscaled_tokens: u32, prompt_tokens: u32) -> f32 {
    if unscaled_tokens == 0 || prompt_tokens == 0 {
        return scale;
    }
    (prompt_tokens as f32 / unscaled_tokens as f32).clamp(MINIMUM_TOKEN_SCALE, MAXIMUM_TOKEN_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ChatToolCall;

    #[test]
    fn estimate_rounds_up() {
        assert_eq!(estimate_tokens(0, 3.5, 1.0), 0);
        assert_eq!(estimate_tokens(35, 3.5, 1.0), 10);
        assert_eq!(estimate_tokens(36, 3.5, 1.0), 11);
        assert_eq!(estimate_tokens(1, 4.0, 1.0), 1);
    }

    #[test]
    fn estimate_applies_the_scale() {
        assert_eq!(estimate_tokens(35, 3.5, 1.5), 15);
        assert_eq!(estimate_tokens(35, 3.5, 0.5), 5);
        assert_eq!(estimate_tokens(36, 3.5, 2.0), 21);
    }

    #[test]
    fn message_characters_count_the_content_and_the_overhead() {
        let message = ChatMessage::User {
            content: "aé日🦀".to_owned(),
        };
        assert_eq!(message_characters(&message), 20);
    }

    #[test]
    fn message_characters_count_the_name_and_the_arguments_of_a_tool_call() {
        let message = ChatMessage::Assistant {
            content: "ok".to_owned(),
            tool_calls: vec![ChatToolCall {
                id: "an id that the model does not see".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"a"}"#.to_owned(),
            }],
        };
        assert_eq!(message_characters(&message), 34);
    }

    #[test]
    fn calibration_gives_the_ratio_of_reported_to_estimated_tokens() {
        assert_eq!(calibrate_token_scale(1.0, 1000, 1250), 1.25);
        assert_eq!(calibrate_token_scale(1.7, 1000, 750), 0.75);
    }

    #[test]
    fn calibration_clamps_the_scale() {
        assert_eq!(calibrate_token_scale(1.0, 1000, 5000), 2.0);
        assert_eq!(calibrate_token_scale(1.0, 1000, 100), 0.5);
    }

    #[test]
    fn a_reported_value_of_zero_keeps_the_scale() {
        assert_eq!(calibrate_token_scale(1.3, 1000, 0), 1.3);
    }

    #[test]
    fn an_estimate_of_zero_keeps_the_scale() {
        assert_eq!(calibrate_token_scale(1.3, 0, 900), 1.3);
    }
}
