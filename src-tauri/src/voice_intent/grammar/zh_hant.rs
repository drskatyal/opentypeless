use super::{CommandMatch, SearchMatch};
use crate::voice_intent::normalize::{trim_command_payload, NormalizedUtterance};
use crate::voice_intent::SearchProvider;

pub(super) fn match_draft(view: &NormalizedUtterance<'_>) -> CommandMatch<String> {
    for prefix in ["寫一封", "幫我寫", "回覆說", "寫個", "起草"] {
        if !view.starts_with_prefix(prefix, false) {
            continue;
        }
        return view
            .payload_after_prefix(prefix)
            .map(CommandMatch::Matched)
            .unwrap_or(CommandMatch::MissingPayload);
    }
    CommandMatch::NoMatch
}

pub(super) fn matches_rewrite(view: &NormalizedUtterance<'_>) -> bool {
    [
        "改寫這段",
        "潤色這段",
        "把這段寫得",
        "精簡這段",
        "擴寫這段",
        "修正這段",
        "把這段改成",
    ]
    .iter()
    .any(|prefix| view.starts_with_prefix(prefix, false))
}

pub(super) fn matches_translation(view: &NormalizedUtterance<'_>) -> bool {
    ["把這段翻譯成", "翻譯這段到", "將選取文字翻譯成"]
        .iter()
        .any(|prefix| {
            view.starts_with_prefix(prefix, false) && view.payload_after_prefix(prefix).is_some()
        })
}

pub(super) fn matches_informational(view: &NormalizedUtterance<'_>) -> bool {
    [
        "總結這段",
        "解釋這段",
        "比較這段",
        "這段是什麼意思",
        "為什麼",
        "怎麼",
        "什麼",
        "誰",
        "何時",
        "哪裡",
    ]
    .iter()
    .any(|prefix| view.starts_with_prefix(prefix, false))
}

pub(super) fn match_search(view: &NormalizedUtterance<'_>) -> CommandMatch<SearchMatch> {
    let text = view.match_text();
    for (name, provider) in provider_names() {
        for verb in ["搜尋", "搜"] {
            let leading = format!("在 {name} {verb}");
            if view.starts_with_prefix(&leading, false) {
                return view
                    .payload_after_prefix(&leading)
                    .map(|query| CommandMatch::Matched(SearchMatch { provider, query }))
                    .unwrap_or(CommandMatch::MissingPayload);
            }

            let start = format!("{verb} ");
            let suffix = format!(" 在 {name}");
            if text.starts_with(&start) && text.ends_with(&suffix) {
                let query_start = start.len();
                let query_end = text.len() - suffix.len();
                return view
                    .original_for_match_range(query_start, query_end)
                    .as_deref()
                    .and_then(trim_command_payload)
                    .map(|query| {
                        CommandMatch::Matched(SearchMatch {
                            provider,
                            query: query.to_string(),
                        })
                    })
                    .unwrap_or(CommandMatch::MissingPayload);
            }
        }
    }
    CommandMatch::NoMatch
}

fn provider_names() -> [(&'static str, SearchProvider); 4] {
    [
        ("google", SearchProvider::Google),
        ("youtube", SearchProvider::YouTube),
        ("amazon", SearchProvider::Amazon),
        ("github", SearchProvider::GitHub),
    ]
}
