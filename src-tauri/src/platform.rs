use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub os: String,
    pub session_type: String,
    pub global_hotkey_reliable: bool,
    pub keyboard_output_reliable: bool,
    pub clipboard_auto_paste_reliable: bool,
}

pub fn current_os() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown"
    }
}

pub fn current_session_type() -> String {
    #[cfg(target_os = "linux")]
    {
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        return normalize_session_type(&session).to_string();
    }

    #[cfg(not(target_os = "linux"))]
    {
        "unknown".to_string()
    }
}

pub fn is_wayland_session() -> bool {
    current_session_type() == "wayland"
}

pub fn capabilities() -> PlatformCapabilities {
    capabilities_for(current_os(), &current_session_type())
}

fn normalize_session_type(session_type: &str) -> &'static str {
    if session_type.eq_ignore_ascii_case("wayland") {
        "wayland"
    } else if session_type.eq_ignore_ascii_case("x11") {
        "x11"
    } else {
        "unknown"
    }
}

fn capabilities_for(os: &str, session_type: &str) -> PlatformCapabilities {
    let normalized_session = normalize_session_type(session_type);
    let is_linux_wayland = os == "linux" && normalized_session == "wayland";

    PlatformCapabilities {
        os: os.to_string(),
        session_type: normalized_session.to_string(),
        global_hotkey_reliable: !is_linux_wayland,
        keyboard_output_reliable: !is_linux_wayland,
        clipboard_auto_paste_reliable: !is_linux_wayland,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_linux_marks_input_automation_unreliable() {
        let caps = capabilities_for("linux", "wayland");

        assert_eq!(caps.session_type, "wayland");
        assert!(!caps.global_hotkey_reliable);
        assert!(!caps.keyboard_output_reliable);
        assert!(!caps.clipboard_auto_paste_reliable);
    }

    #[test]
    fn x11_linux_keeps_input_automation_reliable() {
        let caps = capabilities_for("linux", "x11");

        assert_eq!(caps.session_type, "x11");
        assert!(caps.global_hotkey_reliable);
        assert!(caps.keyboard_output_reliable);
        assert!(caps.clipboard_auto_paste_reliable);
    }

    #[test]
    fn unknown_session_normalizes_to_unknown() {
        let caps = capabilities_for("linux", "mir");

        assert_eq!(caps.session_type, "unknown");
        assert!(caps.global_hotkey_reliable);
    }
}
