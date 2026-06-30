use crate::commands;
use crate::pipeline;
use crate::storage;
use crate::AskHotkeyCache;
use crate::HotkeyModeCache;
use crate::SessionTokenStore;
use tauri::Emitter;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

pub fn default_shortcut() -> Shortcut {
    let default_hotkey = storage::AppConfig::default().hotkey;
    let fallback = {
        #[cfg(target_os = "macos")]
        {
            Shortcut::new(Some(Modifiers::ALT), Code::Slash)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Shortcut::new(Some(Modifiers::CONTROL), Code::Slash)
        }
    };
    parse_hotkey(&default_hotkey).unwrap_or(fallback)
}

pub fn default_ask_shortcut() -> Shortcut {
    let default_hotkey = storage::AppConfig::default().ask_hotkey;
    let fallback = {
        #[cfg(target_os = "macos")]
        {
            Shortcut::new(Some(Modifiers::SUPER), Code::Period)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Shortcut::new(Some(Modifiers::CONTROL), Code::Period)
        }
    };
    parse_hotkey(&default_hotkey).unwrap_or(fallback)
}

fn shortcuts_match(a: &Shortcut, b: &Shortcut) -> bool {
    a.mods == b.mods && a.key == b.key
}

fn is_ask_shortcut(handle: &tauri::AppHandle, shortcut: &Shortcut) -> bool {
    let ask_hotkey = handle
        .state::<AskHotkeyCache>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    parse_hotkey(&ask_hotkey)
        .map(|configured| shortcuts_match(&configured, shortcut))
        .unwrap_or(false)
}

fn show_ask_result_window(handle: &tauri::AppHandle, result: &commands::ask::AskDictationResult) {
    handle
        .state::<commands::ask::AskDictationState>()
        .set_pending_result(result.clone());
    match crate::show_ask_popup_window(handle) {
        Ok(window) => {
            let _ = window.emit("ask:result", result);
        }
        Err(error) => {
            tracing::error!("Failed to show Ask result window: {}", error);
        }
    }
}

fn show_ask_error_window(handle: &tauri::AppHandle, message: String) {
    handle
        .state::<commands::ask::AskDictationState>()
        .set_pending_error(message.clone());
    match crate::show_ask_popup_window(handle) {
        Ok(window) => {
            let _ = window.emit("ask:error", message);
        }
        Err(error) => {
            tracing::error!("Failed to show Ask error window: {}", error);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AskShortcutAction {
    Start,
    Stop,
    StopAfterStart,
    Ignore,
}

fn ask_shortcut_action(
    hotkey_mode: &str,
    event_state: ShortcutState,
    is_recording: bool,
    is_starting: bool,
    is_busy: bool,
) -> AskShortcutAction {
    let is_toggle_mode = hotkey_mode == "toggle";

    match (is_toggle_mode, event_state) {
        (true, ShortcutState::Released) => AskShortcutAction::Ignore,
        (true, ShortcutState::Pressed) => {
            if is_recording {
                AskShortcutAction::Stop
            } else if is_busy {
                AskShortcutAction::Ignore
            } else {
                AskShortcutAction::Start
            }
        }
        (false, ShortcutState::Pressed) => {
            if is_busy {
                AskShortcutAction::Ignore
            } else {
                AskShortcutAction::Start
            }
        }
        (false, ShortcutState::Released) => {
            if is_recording {
                AskShortcutAction::Stop
            } else if is_starting {
                AskShortcutAction::StopAfterStart
            } else {
                AskShortcutAction::Ignore
            }
        }
    }
}

async fn stop_ask_shortcut(handle: tauri::AppHandle) {
    if !handle
        .state::<commands::ask::AskDictationState>()
        .is_recording()
    {
        return;
    }

    let ask_state = handle.state::<commands::ask::AskDictationState>();
    let config_state = handle.state::<storage::ConfigManager>();
    let token_store = handle.state::<SessionTokenStore>();
    let client = handle.state::<reqwest::Client>();

    match commands::ask::stop_ask_dictation(
        handle.clone(),
        ask_state,
        config_state,
        token_store,
        client,
    )
    .await
    {
        Ok(result) => show_ask_result_window(&handle, &result),
        Err(message) if message == "Ask dictation is not recording" => {}
        Err(message) => show_ask_error_window(&handle, message),
    }
}

fn start_ask_shortcut(handle: tauri::AppHandle) {
    let did_reserve_start = {
        let ask_state = handle.state::<commands::ask::AskDictationState>();
        ask_state.try_begin_starting()
    };
    if !did_reserve_start {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let ask_state = handle.state::<commands::ask::AskDictationState>();
        let config_state = handle.state::<storage::ConfigManager>();
        let token_store = handle.state::<SessionTokenStore>();
        let client = handle.state::<reqwest::Client>();

        if let Err(message) = commands::ask::start_reserved_ask_dictation(
            handle.clone(),
            ask_state,
            config_state,
            token_store,
            client,
        )
        .await
        {
            show_ask_error_window(&handle, message);
            return;
        }

        if handle
            .state::<commands::ask::AskDictationState>()
            .take_stop_after_start()
        {
            stop_ask_shortcut(handle).await;
        }
    });
}

fn handle_ask_shortcut(handle: tauri::AppHandle, action: AskShortcutAction) {
    match action {
        AskShortcutAction::Start => start_ask_shortcut(handle),
        AskShortcutAction::Stop => {
            tauri::async_runtime::spawn(stop_ask_shortcut(handle));
        }
        AskShortcutAction::StopAfterStart => {
            let _ = handle
                .state::<commands::ask::AskDictationState>()
                .request_stop_after_start();
        }
        AskShortcutAction::Ignore => {}
    }
}

pub fn build_shortcut_handler(
    app_handle: tauri::AppHandle,
) -> impl Fn(&tauri::AppHandle, &Shortcut, tauri_plugin_global_shortcut::ShortcutEvent)
       + Send
       + Sync
       + 'static {
    move |_app, shortcut, event| {
        let handle = app_handle.clone();
        if is_ask_shortcut(&handle, shortcut) {
            let hotkey_mode = handle
                .state::<HotkeyModeCache>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let ask_state = handle.state::<commands::ask::AskDictationState>();
            let action = ask_shortcut_action(
                &hotkey_mode,
                event.state,
                ask_state.is_recording(),
                ask_state.is_starting(),
                ask_state.is_busy(),
            );
            handle_ask_shortcut(handle, action);
            return;
        }

        match event.state {
            ShortcutState::Pressed => {
                let hotkey_mode = handle
                    .state::<HotkeyModeCache>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                tauri::async_runtime::spawn(async move {
                    if handle.state::<commands::ask::AskDictationState>().is_busy() {
                        return;
                    }

                    let pipeline = handle.state::<pipeline::PipelineHandle>();

                    if hotkey_mode == "toggle" {
                        if pipeline.current_state() == pipeline::PipelineState::Idle {
                            if let Err(e) = pipeline.start().await {
                                tracing::error!("Failed to start recording: {}", e);
                                let _ = handle.emit("pipeline:error", e.to_string());
                            }
                        } else if let Err(e) = pipeline.stop().await {
                            tracing::error!("Failed to stop recording: {}", e);
                            let _ = handle.emit("pipeline:error", e.to_string());
                        }
                    } else if let Err(e) = pipeline.start().await {
                        tracing::error!("Failed to start recording: {}", e);
                        let _ = handle.emit("pipeline:error", e.to_string());
                    }
                });
            }
            ShortcutState::Released => {
                let hotkey_mode = handle
                    .state::<HotkeyModeCache>()
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if hotkey_mode != "toggle" {
                    tauri::async_runtime::spawn(async move {
                        if handle.state::<commands::ask::AskDictationState>().is_busy() {
                            return;
                        }

                        let pipeline = handle.state::<pipeline::PipelineHandle>();
                        if let Err(e) = pipeline.stop().await {
                            tracing::error!("Failed to stop recording: {}", e);
                            let _ = handle.emit("pipeline:error", e.to_string());
                        }
                    });
                }
            }
        }
    }
}

pub fn parse_hotkey(s: &str) -> Option<Shortcut> {
    let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = Modifiers::empty();
    let key_str = parts.last()?;

    for &part in &parts[..parts.len() - 1] {
        match part.to_lowercase().as_str() {
            "alt" | "option" => modifiers |= Modifiers::ALT,
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "meta" | "super" | "win" | "cmd" | "command" => modifiers |= Modifiers::SUPER,
            _ => return None,
        }
    }

    let code = match key_str.to_lowercase().as_str() {
        "space" => Code::Space,
        "tab" => Code::Tab,
        "enter" | "return" => Code::Enter,
        "backspace" => Code::Backspace,
        "escape" | "esc" => Code::Escape,
        "delete" => Code::Delete,
        "insert" => Code::Insert,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "arrowup" | "up" => Code::ArrowUp,
        "arrowdown" | "down" => Code::ArrowDown,
        "arrowleft" | "left" => Code::ArrowLeft,
        "arrowright" | "right" => Code::ArrowRight,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "/" | "slash" => Code::Slash,
        "\\" | "backslash" => Code::Backslash,
        "." | "period" | "。" => Code::Period,
        "," | "comma" => Code::Comma,
        ";" | "semicolon" => Code::Semicolon,
        "'" | "quote" => Code::Quote,
        "`" | "backquote" => Code::Backquote,
        "-" | "minus" => Code::Minus,
        "=" | "equal" => Code::Equal,
        "[" | "bracketleft" => Code::BracketLeft,
        "]" | "bracketright" => Code::BracketRight,
        _ => return None,
    };

    let mods = if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    };
    Some(Shortcut::new(mods, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hotkey_ctrl_slash() {
        let s = parse_hotkey("Ctrl+/");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL);
        assert_eq!(s.key, Code::Slash);
    }

    #[test]
    fn test_parse_hotkey_ctrl_shift_a() {
        let s = parse_hotkey("Ctrl+Shift+A");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(s.key, Code::KeyA);
    }

    #[test]
    fn test_parse_hotkey_case_insensitive() {
        let s = parse_hotkey("cTrL+/");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL);
        assert_eq!(s.key, Code::Slash);
    }

    #[test]
    fn test_parse_hotkey_option_slash() {
        let s = parse_hotkey("Option+/");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::ALT);
        assert_eq!(s.key, Code::Slash);
    }

    #[test]
    fn test_parse_hotkey_command_period() {
        for hotkey in ["Command+.", "Command+。"] {
            let s = parse_hotkey(hotkey);
            assert!(s.is_some(), "Failed to parse {hotkey}");
            let s = s.unwrap();
            assert_eq!(s.mods, Modifiers::SUPER);
            assert_eq!(s.key, Code::Period);
        }
    }

    #[test]
    fn test_parse_hotkey_ctrl_period() {
        let s = parse_hotkey("Ctrl+.");
        assert!(s.is_some());
        let s = s.unwrap();
        assert_eq!(s.mods, Modifiers::CONTROL);
        assert_eq!(s.key, Code::Period);
    }

    #[test]
    fn ask_shortcut_follows_hold_mode_press_and_release() {
        assert_eq!(
            ask_shortcut_action("hold", ShortcutState::Pressed, false, false, false),
            AskShortcutAction::Start
        );
        assert_eq!(
            ask_shortcut_action("hold", ShortcutState::Released, true, false, true),
            AskShortcutAction::Stop
        );
        assert_eq!(
            ask_shortcut_action("hold", ShortcutState::Released, false, true, true),
            AskShortcutAction::StopAfterStart
        );
    }

    #[test]
    fn ask_shortcut_follows_toggle_mode_press_only() {
        assert_eq!(
            ask_shortcut_action("toggle", ShortcutState::Pressed, false, false, false),
            AskShortcutAction::Start
        );
        assert_eq!(
            ask_shortcut_action("toggle", ShortcutState::Pressed, true, false, true),
            AskShortcutAction::Stop
        );
        assert_eq!(
            ask_shortcut_action("toggle", ShortcutState::Released, true, false, true),
            AskShortcutAction::Ignore
        );
        assert_eq!(
            ask_shortcut_action("toggle", ShortcutState::Pressed, false, false, true),
            AskShortcutAction::Ignore
        );
    }

    #[test]
    fn test_parse_hotkey_f_keys() {
        for (key, expected) in [("F1", Code::F1), ("F12", Code::F12)] {
            let s = parse_hotkey(&format!("Ctrl+{}", key));
            assert!(s.is_some(), "Failed to parse Ctrl+{}", key);
            assert_eq!(s.unwrap().key, expected);
        }
    }

    #[test]
    fn test_parse_hotkey_meta_modifier() {
        for name in ["Meta", "Super", "Win", "Cmd", "Command"] {
            let s = parse_hotkey(&format!("{}+A", name));
            assert!(s.is_some(), "Failed to parse {}+A", name);
            assert_eq!(s.unwrap().mods, Modifiers::SUPER);
        }
    }

    #[test]
    fn test_parse_hotkey_no_modifier() {
        let s = parse_hotkey("A");
        assert!(s.is_some());
        assert_eq!(s.unwrap().mods, Modifiers::empty());
    }

    #[test]
    fn test_parse_hotkey_invalid_key() {
        let s = parse_hotkey("Alt+InvalidKey");
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_hotkey_empty_string() {
        let s = parse_hotkey("");
        assert!(s.is_none());
    }

    #[test]
    fn test_parse_hotkey_digits() {
        let s = parse_hotkey("Ctrl+0");
        assert!(s.is_some());
        assert_eq!(s.unwrap().key, Code::Digit0);

        let s = parse_hotkey("Ctrl+9");
        assert!(s.is_some());
        assert_eq!(s.unwrap().key, Code::Digit9);
    }

    #[test]
    fn test_parse_hotkey_navigation_keys() {
        for (key, expected) in [
            ("Enter", Code::Enter),
            ("Tab", Code::Tab),
            ("Escape", Code::Escape),
            ("Backspace", Code::Backspace),
            ("Delete", Code::Delete),
            ("Up", Code::ArrowUp),
            ("Down", Code::ArrowDown),
        ] {
            let s = parse_hotkey(&format!("Alt+{}", key));
            assert!(s.is_some(), "Failed to parse Alt+{}", key);
            assert_eq!(s.unwrap().key, expected);
        }
    }
}
