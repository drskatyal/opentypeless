mod en;
mod zh_hans;
mod zh_hant;

use super::normalize::NormalizedUtterance;
use super::{CommandLocale, SearchProvider};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CommandMatch<T> {
    NoMatch,
    MissingPayload,
    Matched(T),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchMatch {
    pub provider: SearchProvider,
    pub query: String,
}

pub(crate) fn match_draft(
    locale: CommandLocale,
    view: &NormalizedUtterance<'_>,
) -> CommandMatch<String> {
    match locale {
        CommandLocale::En => en::match_draft(view),
        CommandLocale::ZhHans => zh_hans::match_draft(view),
        CommandLocale::ZhHant => zh_hant::match_draft(view),
    }
}

pub(crate) fn matches_rewrite(locale: CommandLocale, view: &NormalizedUtterance<'_>) -> bool {
    match locale {
        CommandLocale::En => en::matches_rewrite(view),
        CommandLocale::ZhHans => zh_hans::matches_rewrite(view),
        CommandLocale::ZhHant => zh_hant::matches_rewrite(view),
    }
}

pub(crate) fn matches_translation(locale: CommandLocale, view: &NormalizedUtterance<'_>) -> bool {
    match locale {
        CommandLocale::En => en::matches_translation(view),
        CommandLocale::ZhHans => zh_hans::matches_translation(view),
        CommandLocale::ZhHant => zh_hant::matches_translation(view),
    }
}

pub(crate) fn matches_informational(locale: CommandLocale, view: &NormalizedUtterance<'_>) -> bool {
    match locale {
        CommandLocale::En => en::matches_informational(view),
        CommandLocale::ZhHans => zh_hans::matches_informational(view),
        CommandLocale::ZhHant => zh_hant::matches_informational(view),
    }
}

pub(crate) fn match_search(
    locale: CommandLocale,
    view: &NormalizedUtterance<'_>,
) -> CommandMatch<SearchMatch> {
    match locale {
        CommandLocale::En => en::match_search(view),
        CommandLocale::ZhHans => zh_hans::match_search(view),
        CommandLocale::ZhHant => zh_hant::match_search(view),
    }
}

pub(crate) fn exact_confidence(view: &NormalizedUtterance<'_>) -> f32 {
    if view
        .match_text()
        .chars()
        .last()
        .is_some_and(|character| matches!(character, '.' | '。' | '!' | '！' | '?' | '？'))
    {
        0.9
    } else {
        1.0
    }
}
