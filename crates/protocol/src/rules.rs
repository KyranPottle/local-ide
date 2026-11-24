pub const RULES_CHARACTER_LIMIT: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulesTooLong;

pub fn rules_text(text: &str) -> Result<Option<String>, RulesTooLong> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > RULES_CHARACTER_LIMIT {
        return Err(RulesTooLong);
    }
    Ok(Some(trimmed.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_is_2000_characters() {
        assert_eq!(RULES_CHARACTER_LIMIT, 2_000);
    }

    #[test]
    fn blank_text_is_none() {
        assert_eq!(rules_text(" \n\t").unwrap(), None);
    }

    #[test]
    fn text_at_the_cap_is_kept() {
        let text = "é".repeat(RULES_CHARACTER_LIMIT);
        assert_eq!(
            rules_text(&format!(" {text} ")).unwrap().as_deref(),
            Some(text.as_str())
        );
    }

    #[test]
    fn text_past_the_cap_is_rejected() {
        let text = "x".repeat(RULES_CHARACTER_LIMIT + 1);
        assert_eq!(rules_text(&text), Err(RulesTooLong));
    }
}
