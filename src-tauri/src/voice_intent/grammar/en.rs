use super::{CommandMatch, SearchMatch};
use crate::voice_intent::normalize::{trim_command_payload, NormalizedUtterance};
use crate::voice_intent::SearchProvider;

pub(super) fn match_draft(view: &NormalizedUtterance<'_>) -> CommandMatch<String> {
    for prefix in ["reply with", "compose", "draft", "write"] {
        if !view.starts_with_prefix(prefix, true) {
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
        "rewrite this",
        "rephrase this",
        "make this shorter",
        "make this longer",
        "make this warmer",
        "make this friendlier",
        "make this more formal",
        "make this more concise",
        "fix the grammar",
        "fix the spelling",
        "format this as",
        "turn this into",
    ]
    .iter()
    .any(|prefix| view.starts_with_prefix(prefix, true))
}

pub(super) fn matches_translation(view: &NormalizedUtterance<'_>) -> bool {
    [
        "translate this to",
        "translate this into",
        "translate the selection to",
        "translate the selection into",
    ]
    .iter()
    .any(|prefix| {
        view.starts_with_prefix(prefix, true) && view.payload_after_prefix(prefix).is_some()
    })
}

pub(super) fn matches_informational(view: &NormalizedUtterance<'_>) -> bool {
    [
        "summarize this",
        "explain this",
        "compare this",
        "what ",
        "why ",
        "how ",
        "who ",
        "when ",
        "where ",
    ]
    .iter()
    .any(|prefix| view.match_text().starts_with(prefix))
}

pub(super) fn match_search(view: &NormalizedUtterance<'_>) -> CommandMatch<SearchMatch> {
    for command in ["search", "find"] {
        let Some(rest) = view.payload_after_prefix(command) else {
            if view.starts_with_prefix(command, true) {
                return CommandMatch::MissingPayload;
            }
            continue;
        };

        for (name, provider) in provider_names() {
            let normalized = rest.to_ascii_lowercase();
            let suffix = format!(" on {name}");
            if normalized.ends_with(&suffix) {
                let query_end = rest.len() - suffix.len();
                return trim_command_payload(&rest[..query_end])
                    .map(|query| {
                        CommandMatch::Matched(SearchMatch {
                            provider,
                            query: query.to_string(),
                        })
                    })
                    .unwrap_or(CommandMatch::MissingPayload);
            }

            if command == "search" {
                let prefix = format!("{name} for ");
                if normalized.starts_with(&prefix) {
                    return trim_command_payload(&rest[prefix.len()..])
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
