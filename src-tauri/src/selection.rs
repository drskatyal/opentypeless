#[cfg(not(target_os = "macos"))]
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};

const CLIPBOARD_COPY_SETTLE_MS: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectedTextCommandIntent {
    Ask,
    Explain,
    Summarize,
    Translate,
    Rewrite,
    FixGrammar,
    Shorten,
    Expand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectedTextCommandOutput {
    PopupAnswer,
    ReplaceSelection,
    InsertAtCursor,
    CopyToClipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedTextCommandRoute {
    pub intent: SelectedTextCommandIntent,
    pub output: SelectedTextCommandOutput,
}

fn selected_text_has_content(selected_text: Option<&str>) -> bool {
    selected_text.is_some_and(|text| !text.trim().is_empty())
}

fn contains_any_marker(normalized: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| normalized.contains(marker))
}

pub fn route_selected_text_command(
    raw_instruction: &str,
    selected_text: Option<&str>,
) -> SelectedTextCommandRoute {
    if !selected_text_has_content(selected_text) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Ask,
            output: SelectedTextCommandOutput::InsertAtCursor,
        };
    }

    let normalized = raw_instruction.trim().to_lowercase();

    if contains_any_marker(&normalized, &["translate", "translation", "翻译"]) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Translate,
            output: SelectedTextCommandOutput::ReplaceSelection,
        };
    }

    if contains_any_marker(
        &normalized,
        &[
            "fix", "correct", "grammar", "typo", "修正", "纠错", "改错", "语法",
        ],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::FixGrammar,
            output: SelectedTextCommandOutput::ReplaceSelection,
        };
    }

    if contains_any_marker(
        &normalized,
        &[
            "shorten",
            "make this shorter",
            "make shorter",
            "concise",
            "缩短",
            "变短",
            "精简",
        ],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Shorten,
            output: SelectedTextCommandOutput::ReplaceSelection,
        };
    }

    if contains_any_marker(
        &normalized,
        &["expand", "elaborate", "make longer", "扩写", "展开"],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Expand,
            output: SelectedTextCommandOutput::ReplaceSelection,
        };
    }

    if contains_any_marker(
        &normalized,
        &[
            "rewrite",
            "rephrase",
            "polish",
            "improve",
            "make better",
            "change tone",
            "润色",
            "改写",
            "重写",
            "优化",
            "换种说法",
        ],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Rewrite,
            output: SelectedTextCommandOutput::ReplaceSelection,
        };
    }

    if contains_any_marker(
        &normalized,
        &["summarize", "summary", "tl;dr", "总结", "概括", "摘要"],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Summarize,
            output: SelectedTextCommandOutput::PopupAnswer,
        };
    }

    if contains_any_marker(
        &normalized,
        &[
            "explain",
            "what does this mean",
            "what does it mean",
            "meaning",
            "解释",
            "什么意思",
            "这段什么意思",
            "解读",
        ],
    ) {
        return SelectedTextCommandRoute {
            intent: SelectedTextCommandIntent::Explain,
            output: SelectedTextCommandOutput::PopupAnswer,
        };
    }

    SelectedTextCommandRoute {
        intent: SelectedTextCommandIntent::Ask,
        output: SelectedTextCommandOutput::PopupAnswer,
    }
}

pub fn selected_text_from_clipboard_result(
    selected: Option<String>,
    sentinel: &str,
) -> Option<String> {
    match selected {
        Some(text) if !text.trim().is_empty() && text != sentinel => Some(text),
        _ => None,
    }
}

fn clipboard_copy_sentinel() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!(
        "__opentypeless_copy_sentinel_{}_{}__",
        std::process::id(),
        nanos
    )
}

#[cfg(target_os = "macos")]
fn copy_selected_text_to_clipboard() -> bool {
    match std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "c" using command down"#,
        ])
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            tracing::warn!(
                "macOS selected-text copy failed with exit code: {:?}",
                status.code()
            );
            false
        }
        Err(e) => {
            tracing::warn!("Failed to run osascript for selected-text copy: {}", e);
            false
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn copy_selected_text_to_clipboard() -> bool {
    let Ok(mut enigo) = Enigo::new(&EnigoSettings::default()) else {
        return false;
    };

    let pressed = enigo.key(Key::Control, Direction::Press).is_ok();
    if pressed {
        let _ = enigo.key(Key::Unicode('c'), Direction::Click);
        let _ = enigo.key(Key::Control, Direction::Release);
    }
    pressed
}

pub fn capture_selected_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let backup = clipboard.get_text().ok();
    let sentinel = clipboard_copy_sentinel();
    let _ = clipboard.set_text(&sentinel);

    if !copy_selected_text_to_clipboard() {
        tracing::debug!("Selected text copy shortcut could not be sent");
    }

    std::thread::sleep(std::time::Duration::from_millis(CLIPBOARD_COPY_SETTLE_MS));

    let selected = clipboard.get_text().ok();

    if let Some(ref b) = backup {
        let _ = clipboard.set_text(b);
    } else {
        let _ = clipboard.set_text("");
    }

    tracing::info!(
        "Selected text capture: backup_len={}, selected_len={}",
        backup.as_deref().map(|s| s.len()).unwrap_or(0),
        selected.as_deref().map(|s| s.len()).unwrap_or(0)
    );

    let result = selected_text_from_clipboard_result(selected, &sentinel);
    if result.is_none() {
        tracing::debug!("Selected text capture did not produce fresh clipboard text");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_text_rejects_copy_sentinel_when_clipboard_was_unchanged() {
        assert_eq!(
            selected_text_from_clipboard_result(Some("__sentinel__".to_string()), "__sentinel__"),
            None
        );
    }

    #[test]
    fn selected_text_accepts_text_that_matches_previous_clipboard_backup() {
        assert_eq!(
            selected_text_from_clipboard_result(Some("selected text".to_string()), "__sentinel__"),
            Some("selected text".to_string())
        );
    }

    #[test]
    fn selected_text_rejects_whitespace_only_clipboard() {
        assert_eq!(
            selected_text_from_clipboard_result(Some(" \n\t ".to_string()), "__sentinel__"),
            None
        );
    }

    #[test]
    fn selected_text_command_routes_explain_and_summarize_to_popup_answer() {
        assert_eq!(
            route_selected_text_command("这段什么意思", Some("selected text")).output,
            SelectedTextCommandOutput::PopupAnswer
        );
        assert_eq!(
            route_selected_text_command("summarize this", Some("selected text")).output,
            SelectedTextCommandOutput::PopupAnswer
        );
    }

    #[test]
    fn selected_text_command_routes_explicit_edits_to_replace_selection() {
        assert_eq!(
            route_selected_text_command("润色这段", Some("selected text")).intent,
            SelectedTextCommandIntent::Rewrite
        );
        assert_eq!(
            route_selected_text_command("translate this to English", Some("selected text")).output,
            SelectedTextCommandOutput::ReplaceSelection
        );
        assert_eq!(
            route_selected_text_command("fix the grammar", Some("selected text")).intent,
            SelectedTextCommandIntent::FixGrammar
        );
    }

    #[test]
    fn selected_text_command_without_selected_text_keeps_cursor_output() {
        assert_eq!(
            route_selected_text_command("summarize this", None).output,
            SelectedTextCommandOutput::InsertAtCursor
        );
    }
}
