pub(crate) fn char_prefix(text: &str, limit: usize) -> &str {
    text.char_indices()
        .nth(limit)
        .map_or(text, |(end, _)| &text[..end])
}

// The same cap applies to a tool output and to an attachment content
pub fn cap_output(output: &str, limit: usize) -> String {
    let kept = char_prefix(output, limit);
    if kept.len() == output.len() {
        return output.to_owned();
    }
    format!("{kept}\n[output truncated at {limit} characters]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_at_the_limit_stays_unchanged() {
        assert_eq!(cap_output("abcdef", 6), "abcdef");
        assert_eq!(cap_output("", 0), "");
    }

    #[test]
    fn output_over_the_limit_is_cut_and_marked() {
        assert_eq!(
            cap_output("abcdefg", 6),
            "abcdef\n[output truncated at 6 characters]"
        );
    }

    #[test]
    fn the_limit_counts_characters_and_the_cut_is_on_a_character_boundary() {
        let cases = [
            (0, "\n[output truncated at 0 characters]"),
            (1, "a\n[output truncated at 1 characters]"),
            (2, "aé\n[output truncated at 2 characters]"),
            (3, "aé日\n[output truncated at 3 characters]"),
            (4, "aé日🦀\n[output truncated at 4 characters]"),
            (5, "aé日🦀z"),
            (6, "aé日🦀z"),
        ];
        for (limit, expected) in cases {
            assert_eq!(cap_output("aé日🦀z", limit), expected, "limit {limit}");
        }
    }

    #[test]
    fn char_prefix_cuts_between_characters() {
        let cases = [(0, ""), (1, "é"), (2, "é日"), (3, "é日🦀"), (4, "é日🦀")];
        for (limit, expected) in cases {
            assert_eq!(char_prefix("é日🦀", limit), expected, "limit {limit}");
        }
    }
}
