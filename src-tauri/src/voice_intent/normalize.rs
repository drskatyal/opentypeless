use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct NormalizedUtterance<'a> {
    original: &'a str,
    match_text: String,
    original_ranges: Vec<Range<usize>>,
}

impl<'a> NormalizedUtterance<'a> {
    pub(crate) fn new(original: &'a str) -> Self {
        let mut match_text = String::new();
        let mut original_ranges = Vec::<Range<usize>>::new();
        let mut pending_whitespace: Option<Range<usize>> = None;

        let mut chars = original.char_indices().peekable();
        while let Some((start, character)) = chars.next() {
            let end = chars
                .peek()
                .map(|(index, _)| *index)
                .unwrap_or(original.len());
            if character.is_whitespace() {
                if !match_text.is_empty() {
                    match pending_whitespace.as_mut() {
                        Some(range) => range.end = end,
                        None => pending_whitespace = Some(start..end),
                    }
                }
                continue;
            }

            if let Some(range) = pending_whitespace.take() {
                match_text.push(' ');
                original_ranges.push(range);
            }
            for normalized in character.to_lowercase() {
                match_text.push(normalized);
                original_ranges.push(start..end);
            }
        }

        Self {
            original,
            match_text,
            original_ranges,
        }
    }

    pub(crate) fn match_text(&self) -> &str {
        &self.match_text
    }

    pub(crate) fn starts_with_prefix(&self, prefix: &str, require_boundary: bool) -> bool {
        let Some(remainder) = self.match_text.strip_prefix(prefix) else {
            return false;
        };
        if !require_boundary || remainder.is_empty() {
            return true;
        }
        remainder.chars().next().is_some_and(is_command_boundary)
    }

    pub(crate) fn payload_after_prefix(&self, prefix: &str) -> Option<String> {
        if !self.starts_with_prefix(prefix, prefix.is_ascii()) {
            return None;
        }
        let prefix_chars = prefix.chars().count();
        let original_end = self.original_ranges.get(prefix_chars.checked_sub(1)?)?.end;
        trim_command_payload(self.original.get(original_end..)?).map(ToString::to_string)
    }

    pub(crate) fn original_for_match_range(&self, start: usize, end: usize) -> Option<String> {
        if start >= end || end > self.match_text.len() {
            return None;
        }
        let start_character = self.match_text[..start].chars().count();
        let end_character = self.match_text[..end].chars().count();
        let original_start = self.original_ranges.get(start_character)?.start;
        let original_end = self.original_ranges.get(end_character.checked_sub(1)?)?.end;
        trim_command_payload(self.original.get(original_start..original_end)?)
            .map(ToString::to_string)
    }
}

fn is_command_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            ':' | '：' | ',' | '，' | '.' | '。' | '!' | '！' | '?' | '？' | '-' | '—'
        )
}

pub(crate) fn trim_command_payload(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    let trimmed = trimmed
        .trim_start_matches(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    ':' | '：' | ',' | '，' | '.' | '。' | '!' | '！' | '?' | '？' | '-' | '—'
                )
        })
        .trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_intent_grammar_normalizes_unicode_whitespace_and_maps_original_ranges() {
        let view = NormalizedUtterance::new(" \nDrAfT\u{3000}:  Project X — Tomorrow!  ");
        assert_eq!(view.match_text(), "draft : project x — tomorrow!");
        assert_eq!(
            view.payload_after_prefix("draft"),
            Some("Project X — Tomorrow!".to_string())
        );
    }

    #[test]
    fn voice_intent_grammar_requires_token_boundary_for_english_prefixes() {
        let view = NormalizedUtterance::new("draftsmanship matters");
        assert!(!view.starts_with_prefix("draft", true));
        let view = NormalizedUtterance::new("draft: a reply");
        assert!(view.starts_with_prefix("draft", true));
    }
}
