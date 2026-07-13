use super::{CommandLocale, RouteFallbackReason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuardReason {
    CodeOrIdentifier,
    QuotedOrReported,
    Negated,
}

impl From<GuardReason> for RouteFallbackReason {
    fn from(value: GuardReason) -> Self {
        match value {
            GuardReason::CodeOrIdentifier => Self::CodeOrIdentifier,
            GuardReason::QuotedOrReported => Self::QuotedOrReported,
            GuardReason::Negated => Self::Negated,
        }
    }
}

pub(crate) fn guard_reason(locale: CommandLocale, raw: &str) -> Option<GuardReason> {
    if has_code_or_identifier_command(locale, raw) {
        return Some(GuardReason::CodeOrIdentifier);
    }
    if has_quoted_or_reported_command(locale, raw) {
        return Some(GuardReason::QuotedOrReported);
    }
    if has_negated_command(locale, raw) {
        return Some(GuardReason::Negated);
    }
    None
}

pub(crate) fn has_command_signal(locale: CommandLocale, raw: &str) -> bool {
    let normalized = raw.to_lowercase();
    if contains_ascii_command_signal(&normalized) {
        return true;
    }
    match locale {
        CommandLocale::En => false,
        CommandLocale::ZhHans => [
            "写一封",
            "起草",
            "帮我写",
            "回复说",
            "写个",
            "改写这段",
            "润色这段",
            "把这段写得",
            "精简这段",
            "扩写这段",
            "修正这段",
            "把这段改成",
            "翻译",
            "把这段翻译成",
            "翻译这段到",
            "将选中文字翻译成",
            "总结这段",
            "解释这段",
            "比较这段",
            "这段是什么意思",
            "搜索",
            "搜",
        ]
        .iter()
        .any(|marker| normalized.contains(marker)),
        CommandLocale::ZhHant => [
            "寫一封",
            "起草",
            "幫我寫",
            "回覆說",
            "寫個",
            "改寫這段",
            "潤色這段",
            "把這段寫得",
            "精簡這段",
            "擴寫這段",
            "修正這段",
            "把這段改成",
            "翻譯",
            "把這段翻譯成",
            "翻譯這段到",
            "將選取文字翻譯成",
            "總結這段",
            "解釋這段",
            "比較這段",
            "這段是什麼意思",
            "搜尋",
            "搜",
        ]
        .iter()
        .any(|marker| normalized.contains(marker)),
    }
}

pub(crate) fn has_any_supported_command_signal(raw: &str) -> bool {
    [
        CommandLocale::En,
        CommandLocale::ZhHans,
        CommandLocale::ZhHant,
    ]
    .iter()
    .any(|locale| has_command_signal(*locale, raw))
}

fn contains_ascii_command_signal(normalized: &str) -> bool {
    [
        "draft",
        "write",
        "compose",
        "reply",
        "reply with",
        "rewrite",
        "rephrase",
        "translate",
        "fix",
        "make",
        "format",
        "turn",
        "make this",
        "fix the",
        "format this",
        "turn this",
        "translate this",
        "translate the selection",
        "summarize this",
        "explain this",
        "compare this",
        "search",
        "find",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn has_code_or_identifier_command(locale: CommandLocale, raw: &str) -> bool {
    if !has_command_signal(locale, raw) {
        return false;
    }
    if raw.contains('_') || raw.contains('(') || raw.contains(')') {
        return true;
    }

    let normalized = raw.to_ascii_lowercase();
    for marker in [
        "draft",
        "write",
        "compose",
        "reply",
        "rewrite",
        "rephrase",
        "translate",
        "search",
        "find",
        "format",
        "turn",
        "fix",
        "make",
    ] {
        for separator in ['.', '-', '?'] {
            let pattern = format!("{marker}{separator}");
            if normalized.find(&pattern).is_some_and(|index| {
                normalized[index + pattern.len()..]
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_alphanumeric())
            }) {
                return true;
            }
        }
    }

    for marker in [
        "draft",
        "write",
        "compose",
        "reply",
        "rewrite",
        "translate",
        "search",
        "format",
        "turn",
        "fix",
        "make",
    ] {
        if raw.match_indices(marker).any(|(index, value)| {
            raw[index + value.len()..]
                .chars()
                .next()
                .is_some_and(char::is_uppercase)
        }) {
            return true;
        }
    }

    false
}

pub(crate) fn has_quoted_or_reported_command(locale: CommandLocale, raw: &str) -> bool {
    if !has_command_signal(locale, raw) {
        return false;
    }
    let normalized = raw.to_lowercase();
    let reported = match locale {
        CommandLocale::En => [
            " said ",
            " says ",
            "asked me to",
            "the phrase",
            "the word",
            "quote:",
            "quoted command",
        ]
        .iter()
        .any(|marker| normalized.contains(marker)),
        CommandLocale::ZhHans => [
            "他说",
            "她说",
            "他们说",
            "原文是",
            "这句话是",
            "引用",
            "文档里写着",
            "有人说",
        ]
        .iter()
        .any(|marker| normalized.contains(marker)),
        CommandLocale::ZhHant => [
            "他說",
            "她說",
            "他們說",
            "原文是",
            "這句話是",
            "引用",
            "文件裡寫著",
            "有人說",
        ]
        .iter()
        .any(|marker| normalized.contains(marker)),
    };
    reported || has_balanced_command_quotes(raw)
}

fn has_balanced_command_quotes(raw: &str) -> bool {
    let ascii_double_quotes = raw.chars().filter(|character| *character == '"').count() >= 2;
    ascii_double_quotes
        || (raw.contains('“') && raw.contains('”'))
        || (raw.contains('「') && raw.contains('」'))
        || (raw.contains('『') && raw.contains('』'))
}

fn has_negated_command(locale: CommandLocale, raw: &str) -> bool {
    if !has_command_signal(locale, raw) {
        return false;
    }
    let normalized = raw.trim().to_lowercase();
    match locale {
        CommandLocale::En => [
            "do not ",
            "don't ",
            "never ",
            "not yet",
            "please do not ",
            "please don't ",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix)),
        CommandLocale::ZhHans => [
            "不要帮我",
            "不要",
            "别",
            "不用",
            "先别",
            "暂时不要",
            "不需要",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix)),
        CommandLocale::ZhHant => [
            "不要幫我",
            "不要",
            "別",
            "不用",
            "先別",
            "暫時不要",
            "不需要",
        ]
        .iter()
        .any(|prefix| normalized.starts_with(prefix)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_intent::CommandLocale;

    #[test]
    fn voice_intent_grammar_guard_order_prefers_identifier_then_report_then_negation() {
        assert_eq!(
            guard_reason(CommandLocale::En, "compose.yaml says do not draft"),
            Some(GuardReason::CodeOrIdentifier)
        );
        assert_eq!(
            guard_reason(CommandLocale::En, "she said do not draft a reply"),
            Some(GuardReason::QuotedOrReported)
        );
        assert_eq!(
            guard_reason(CommandLocale::En, "do not draft a reply"),
            Some(GuardReason::Negated)
        );
    }

    #[test]
    fn voice_intent_grammar_detects_balanced_command_quotes_but_not_apostrophes() {
        assert!(has_quoted_or_reported_command(
            CommandLocale::En,
            "she said \"draft a reply\""
        ));
        assert!(!has_quoted_or_reported_command(
            CommandLocale::En,
            "draft tomorrow's reply"
        ));
    }
}
