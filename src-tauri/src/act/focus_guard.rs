//! Focus guard — never send keystrokes into a window that isn't the intended app.
//!
//! When an Act command launches or switches to an app and then types or presses
//! keys, there is a race: the OS may not have moved focus to the target app yet
//! (a browser cold-start, a splash screen), so a `type_text` / `key_combo` lands
//! in whatever window was foreground before — e.g. the terminal that started the
//! app. That is how "open Chrome and go to youtube.com" once typed into (and
//! closed) a PowerShell window.
//!
//! The guard is deterministic and fail-closed: before any keystroke-emitting
//! action, verify the foreground app matches the command's *intended* target
//! (the app it just launched/focused). On a mismatch it actively focuses the
//! target and re-checks a bounded number of times; if it still can't confirm the
//! right window is in front (UAC prompt, splash, focus-stealing prevention), it
//! ABORTS the action rather than typing into the wrong place.
//!
//! Crucially the target is the app the command deliberately acted on — never
//! "whatever is foreground right now" — so a stale foreground window can never
//! bless itself as the target.

use std::sync::Arc;
use std::time::Duration;

use super::backend::AccessibilityBackend;
use super::killswitch::KillSwitch;

/// Extra focus attempts after the first check (so up to `RETRIES + 1` snapshots).
const RETRIES: u32 = 2;
/// Bounded settle wait after a focus attempt before re-checking the foreground.
const SETTLE: Duration = Duration::from_millis(350);

/// Normalize an app name for fuzzy comparison: lowercase, drop `.exe`, and strip
/// whitespace so "Google Chrome", "Chrome", and "chrome.exe" all compare equal.
pub fn normalize_app_name(s: &str) -> String {
    s.to_lowercase()
        .replace(".exe", "")
        .split_whitespace()
        .collect::<String>()
}

/// Whether a foreground app name and an expected app name refer to the same app.
/// Substring both ways after normalization, because the launch target ("Google
/// Chrome") and the reported foreground ("Chrome" / "chrome.exe") often differ.
pub fn apps_match(foreground: &str, expected: &str) -> bool {
    let a = normalize_app_name(foreground);
    let b = normalize_app_name(expected);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// Ensure `expected` is the foreground app before keystroke input is sent.
///
/// Snapshot → match? done. Else focus the target, settle, re-check — up to a
/// bounded number of retries. Returns `Ok(())` when the target is confirmed
/// foreground, or `Err(reason)` when it can't be (the caller MUST then abort the
/// action instead of sending input). Every await is raced against the kill
/// switch so an abort is honored mid-guard.
pub async fn ensure_target_focused(
    backend: &Arc<dyn AccessibilityBackend>,
    kill: &KillSwitch,
    expected: &str,
) -> Result<(), String> {
    let mut last_seen = String::new();
    for attempt in 0..=RETRIES {
        let snapshot = tokio::select! {
            biased;
            _ = kill.wait_tripped() => return Err("aborted during focus guard".into()),
            snap = backend.snapshot() => snap.map_err(|e| format!("snapshot failed: {e}"))?,
        };
        last_seen = snapshot.app.clone();
        if apps_match(&snapshot.app, expected) {
            return Ok(());
        }
        if attempt == RETRIES {
            break;
        }
        // Actively bring the intended target forward, then let focus settle.
        tokio::select! {
            biased;
            _ = kill.wait_tripped() => return Err("aborted during focus guard".into()),
            _ = backend.focus_app(expected) => {}
        }
        tokio::select! {
            biased;
            _ = kill.wait_tripped() => return Err("aborted during focus guard".into()),
            _ = tokio::time::sleep(SETTLE) => {}
        }
    }
    Err(format!(
        "target app \"{expected}\" is not in the foreground (saw \"{last_seen}\"); \
         refused to send keystrokes to the wrong window"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_covers_app_name_variants() {
        assert!(apps_match("chrome", "Google Chrome"));
        assert!(apps_match("Google Chrome", "chrome.exe"));
        assert!(apps_match("CHROME.EXE", "google chrome"));
        assert!(apps_match("Notepad", "notepad"));
        assert!(!apps_match("powershell", "chrome"));
        assert!(!apps_match("Windows PowerShell", "Google Chrome"));
    }

    #[test]
    fn empty_names_never_match() {
        assert!(!apps_match("", "chrome"));
        assert!(!apps_match("chrome", ""));
        assert!(!apps_match("", ""));
    }
}
