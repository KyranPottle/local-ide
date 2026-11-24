use std::borrow::Cow;

// Inserts a backslash into each closing tag of tag in body, so that only the block's own closing tag ends the block
// A match ignores ASCII case and allows whitespace before `>`
pub(crate) fn neutralize_closing_tag<'a>(body: &'a str, tag: &str) -> Cow<'a, str> {
    let mut escaped = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = body[search..].find("</") {
        let start = search + offset;
        search = start + 2;
        if !is_closing_tag(&body[search..], tag) {
            continue;
        }
        escaped.push_str(&body[copied..start]);
        escaped.push_str("<\\/");
        copied = search;
    }
    if copied == 0 {
        return Cow::Borrowed(body);
    }
    escaped.push_str(&body[copied..]);
    Cow::Owned(escaped)
}

fn is_closing_tag(rest: &str, tag: &str) -> bool {
    let Some(name) = rest.get(..tag.len()) else {
        return false;
    };
    name.eq_ignore_ascii_case(tag)
        && rest[tag.len()..]
            .trim_start_matches(|character: char| character.is_ascii_whitespace())
            .starts_with('>')
}

pub(crate) fn attribute_value(value: &str) -> Cow<'_, str> {
    if !value
        .chars()
        .any(|character| character == '"' || character.is_control())
    {
        return Cow::Borrowed(value);
    }
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("&quot;"),
            character if character.is_control() => escaped.push(' '),
            character => escaped.push(character),
        }
    }
    Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_tag_gets_a_backslash_in_each_case_and_spacing_variant() {
        assert_eq!(
            neutralize_closing_tag("a</file>b</FILE >c</File\t>d", "file"),
            "a<\\/file>b<\\/FILE >c<\\/File\t>d"
        );
    }

    #[test]
    fn other_tags_and_partial_names_stay_byte_identical() {
        let body = "</tool_response> </files> </fil> </ file> </file";
        assert!(matches!(
            neutralize_closing_tag(body, "file"),
            Cow::Borrowed(text) if text == body
        ));
    }

    #[test]
    fn attribute_value_replaces_quotes_and_control_characters() {
        assert_eq!(attribute_value("a\"b\nc\r\td.rs"), "a&quot;b c  d.rs");
    }
}
