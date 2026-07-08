use crate::credentials::{migrate_legacy_config_secrets, SystemCredentialVault};
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri_plugin_store::StoreExt;

const CUSTOM_SCENES_MAX_COUNT: usize = 100;
const SCENE_ID_MAX_CHARS: usize = 120;
const SCENE_SOURCE_MAX_CHARS: usize = 24;
const SCENE_NAME_MAX_CHARS: usize = 80;
const SCENE_DESCRIPTION_MAX_CHARS: usize = 240;
pub(crate) const SCENE_PROMPT_MAX_CHARS: usize = 4000;
pub const DEFAULT_HISTORY_MAX_ENTRIES: u32 = 5000;
pub const MAX_HISTORY_RETENTION_DAYS: u32 = 3650;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct CustomScene {
    pub id: String,
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ActiveScene {
    pub id: String,
    pub source: String,
    pub name: String,
    pub prompt_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ShortcutBinding {
    pub primary: String,
    pub modifiers: Vec<String>,
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        binding_from_hotkey_string(default_dictation_hotkey()).unwrap_or_else(|| Self {
            primary: "/".to_string(),
            modifiers: vec!["Ctrl".to_string()],
        })
    }
}

impl ShortcutBinding {
    pub fn from_hotkey(value: &str) -> Option<Self> {
        binding_from_hotkey_string(value)
    }

    pub fn to_hotkey_string(&self) -> Option<String> {
        let mut binding = self.clone();
        if !binding.normalize() {
            return None;
        }

        let mut parts = binding.modifiers;
        parts.push(binding.primary);
        Some(parts.join("+"))
    }

    pub fn normalize(&mut self) -> bool {
        let Some(primary) = normalize_hotkey_primary(&self.primary) else {
            return false;
        };

        let mut seen_semantic = HashSet::new();
        let mut modifiers = Vec::new();
        for modifier in &self.modifiers {
            let Some((semantic, canonical)) = normalize_hotkey_modifier(modifier) else {
                return false;
            };
            if !seen_semantic.insert(semantic) {
                return false;
            }
            modifiers.push(canonical.to_string());
        }

        modifiers.sort_by_key(|modifier| hotkey_modifier_rank(modifier));
        self.primary = primary;
        self.modifiers = modifiers;
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub dictation: ShortcutBinding,
    pub ask: Option<ShortcutBinding>,
    pub translate: Option<ShortcutBinding>,
    pub edit_selection: Option<ShortcutBinding>,
    pub switch_scene: Option<ShortcutBinding>,
    pub open_app: Option<ShortcutBinding>,
    pub dictation_mode: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        let mut config = Self::from_legacy(
            default_dictation_hotkey(),
            default_ask_hotkey(),
            default_dictation_hotkey_mode(),
        );
        config.translate = default_translate_hotkey().and_then(ShortcutBinding::from_hotkey);
        config
    }
}

impl HotkeyConfig {
    pub fn from_legacy(dictation_hotkey: &str, ask_hotkey: &str, dictation_mode: &str) -> Self {
        let dictation = ShortcutBinding::from_hotkey(dictation_hotkey)
            .unwrap_or_else(|| ShortcutBinding::from_hotkey(default_dictation_hotkey()).unwrap());
        let ask = if ask_hotkey.trim().is_empty() {
            None
        } else {
            ShortcutBinding::from_hotkey(ask_hotkey)
                .or_else(|| ShortcutBinding::from_hotkey(default_ask_hotkey()))
        };
        let dictation_mode = normalize_hotkey_mode(dictation_mode).to_string();

        Self {
            dictation,
            ask,
            translate: None,
            edit_selection: None,
            switch_scene: None,
            open_app: None,
            dictation_mode,
        }
    }

    pub fn normalize(&mut self) {
        if !self.dictation.normalize() {
            self.dictation = ShortcutBinding::from_hotkey(default_dictation_hotkey()).unwrap();
        }

        if let Some(ask) = self.ask.as_mut() {
            if !ask.normalize() {
                self.ask = ShortcutBinding::from_hotkey(default_ask_hotkey());
            }
        }

        normalize_optional_binding(&mut self.translate);
        normalize_optional_binding(&mut self.edit_selection);
        normalize_optional_binding(&mut self.switch_scene);
        normalize_optional_binding(&mut self.open_app);
        self.dictation_mode = normalize_hotkey_mode(&self.dictation_mode).to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub stt_provider: String,
    pub stt_api_key: String,
    pub stt_custom_api_key: String,
    pub stt_language: String,
    pub stt_custom_preset: String,
    pub stt_custom_base_url: String,
    pub stt_custom_model: String,
    pub stt_volcengine_resource_id: String,
    pub llm_provider: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub llm_base_url: String,
    pub polish_enabled: bool,
    pub polish_style: String,
    pub polish_custom_prompt: String,
    pub polish_chinese_script: String,
    pub custom_scenes: Vec<CustomScene>,
    pub active_scene: Option<ActiveScene>,
    pub translate_enabled: bool,
    pub target_lang: String,
    pub hotkey: String,
    pub ask_hotkey: String,
    pub hotkey_mode: String,
    pub hotkeys: HotkeyConfig,
    pub output_mode: String,
    pub insertion_strategy: String,
    pub restore_clipboard_after_paste: bool,
    pub paste_shortcut: String,
    pub windows_sendinput_newline_mode: String,
    pub streaming_insert_enabled: bool,
    pub selected_text_enabled: bool,
    pub theme: String,
    pub auto_start: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub max_recording_seconds: u32,
    pub history_enabled: bool,
    pub history_retention_days: u32,
    pub history_max_entries: u32,
    pub ui_language: String,
    pub capsule_auto_hide: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            stt_provider: "glm-asr".to_string(),
            stt_api_key: String::new(),
            stt_custom_api_key: String::new(),
            stt_language: "multi".to_string(),
            stt_custom_preset: crate::stt::config::CUSTOM_WHISPER_PRESET_SPEACHES.to_string(),
            stt_custom_base_url: crate::stt::config::DEFAULT_CUSTOM_WHISPER_BASE_URL.to_string(),
            stt_custom_model: crate::stt::config::DEFAULT_CUSTOM_WHISPER_MODEL.to_string(),
            stt_volcengine_resource_id: crate::stt::volcengine::VOLCENGINE_SEEDASR_RESOURCE_ID
                .to_string(),
            llm_provider: "openrouter".to_string(),
            llm_api_key: String::new(),
            llm_model: "google/gemini-2.5-flash".to_string(),
            llm_base_url: "https://openrouter.ai/api/v1".to_string(),
            polish_enabled: true,
            polish_style: "clean".to_string(),
            polish_custom_prompt: String::new(),
            polish_chinese_script: "preserve".to_string(),
            custom_scenes: Vec::new(),
            active_scene: None,
            translate_enabled: false,
            target_lang: "en".to_string(),
            hotkey: default_dictation_hotkey().to_string(),
            ask_hotkey: default_ask_hotkey().to_string(),
            hotkey_mode: default_dictation_hotkey_mode().to_string(),
            hotkeys: HotkeyConfig::default(),
            output_mode: "keyboard".to_string(),
            insertion_strategy: "auto".to_string(),
            restore_clipboard_after_paste: true,
            paste_shortcut: "ctrlV".to_string(),
            windows_sendinput_newline_mode: "enter".to_string(),
            streaming_insert_enabled: false,
            selected_text_enabled: false,
            theme: "system".to_string(),
            auto_start: true,
            close_to_tray: true,
            start_minimized: false,
            max_recording_seconds: 30,
            history_enabled: true,
            history_retention_days: 0,
            history_max_entries: DEFAULT_HISTORY_MAX_ENTRIES,
            ui_language: "en".to_string(),
            capsule_auto_hide: false,
        }
    }
}

impl AppConfig {
    pub fn new_install_default() -> Self {
        Self {
            capsule_auto_hide: true,
            ..Self::default()
        }
    }

    fn normalize_platform_hotkey(&mut self) {
        #[cfg(target_os = "macos")]
        if self.hotkey == "Alt+/" {
            self.hotkey = "Option+/".to_string();
        }
        #[cfg(target_os = "macos")]
        if self.hotkey == "Option+/" && self.hotkey_mode == "hold" {
            self.hotkey = "Fn".to_string();
            self.hotkey_mode = "toggle".to_string();
        }
        #[cfg(target_os = "windows")]
        if self.hotkey == "Ctrl+/" && self.hotkey_mode == "hold" {
            self.hotkey = "RightAlt".to_string();
            self.hotkey_mode = "toggle".to_string();
        }
        #[cfg(target_os = "macos")]
        if self.ask_hotkey == "Alt+Shift+/"
            || self.ask_hotkey == "Option+Shift+/"
            || self.ask_hotkey == "Command+Shift+/"
            || self.ask_hotkey == "Command+/"
            || self.ask_hotkey == "Command+。"
            || self.ask_hotkey == "Command+."
        {
            self.ask_hotkey = default_ask_hotkey().to_string();
        }
        #[cfg(not(target_os = "macos"))]
        if self.ask_hotkey == "Ctrl+Shift+/"
            || self.ask_hotkey == "Control+Shift+/"
            || self.ask_hotkey == "Ctrl+/"
            || self.ask_hotkey == "Control+/"
            || self.ask_hotkey == "Ctrl+."
            || self.ask_hotkey == "Control+."
        {
            self.ask_hotkey = default_ask_hotkey().to_string();
        }
    }

    fn normalize_hotkey_settings(&mut self) {
        self.normalize_platform_hotkey();
        self.hotkey_mode = normalize_hotkey_mode(&self.hotkey_mode).to_string();
        let translate = self.hotkeys.translate.clone();
        let edit_selection = self.hotkeys.edit_selection.clone();
        let switch_scene = self.hotkeys.switch_scene.clone();
        let open_app = self.hotkeys.open_app.clone();
        self.hotkeys = HotkeyConfig::from_legacy(&self.hotkey, &self.ask_hotkey, &self.hotkey_mode);
        self.hotkeys.translate = translate;
        self.hotkeys.edit_selection = edit_selection;
        self.hotkeys.switch_scene = switch_scene;
        self.hotkeys.open_app = open_app;
        self.sync_legacy_hotkey_fields_from_typed();
    }

    fn sync_legacy_hotkey_fields_from_typed(&mut self) {
        self.hotkeys.normalize();
        self.hotkey = self
            .hotkeys
            .dictation
            .to_hotkey_string()
            .unwrap_or_else(|| default_dictation_hotkey().to_string());
        self.ask_hotkey = self
            .hotkeys
            .ask
            .as_ref()
            .and_then(ShortcutBinding::to_hotkey_string)
            .unwrap_or_default();
        self.hotkey_mode = self.hotkeys.dictation_mode.clone();
    }

    pub(crate) fn normalize_values(&mut self) {
        self.polish_style = normalize_polish_style(&self.polish_style).to_string();
        self.polish_custom_prompt = sanitize_polish_custom_prompt(&self.polish_custom_prompt);
        self.polish_chinese_script = "preserve".to_string();
        sanitize_custom_scenes(&mut self.custom_scenes);
        sanitize_active_scene(&mut self.active_scene);
        self.normalize_insertion_strategy();
        self.normalize_paste_shortcut();
        self.normalize_windows_sendinput_newline_mode();
        self.normalize_hotkey_settings();
        self.normalize_history_settings();
    }

    fn normalize_insertion_strategy(&mut self) {
        if !matches!(
            self.insertion_strategy.as_str(),
            "auto" | "keyboard" | "clipboardPaste" | "clipboardCopyOnly" | "windowsSendInput"
        ) {
            self.insertion_strategy = "auto".to_string();
        }

        self.output_mode =
            legacy_output_mode_for_insertion_strategy(&self.insertion_strategy).to_string();
    }

    fn normalize_paste_shortcut(&mut self) {
        if !matches!(
            self.paste_shortcut.as_str(),
            "ctrlV" | "ctrlShiftV" | "shiftInsert"
        ) {
            self.paste_shortcut = "ctrlV".to_string();
        }
    }

    fn normalize_windows_sendinput_newline_mode(&mut self) {
        if !matches!(
            self.windows_sendinput_newline_mode.as_str(),
            "enter" | "shiftEnter" | "crlf"
        ) {
            self.windows_sendinput_newline_mode = "enter".to_string();
        }
    }

    fn normalize_history_settings(&mut self) {
        self.history_retention_days = self.history_retention_days.min(MAX_HISTORY_RETENTION_DAYS);
        self.history_max_entries = self
            .history_max_entries
            .clamp(1, DEFAULT_HISTORY_MAX_ENTRIES);
    }

    pub fn history_retention_policy(&self) -> HistoryRetentionPolicy {
        HistoryRetentionPolicy {
            enabled: self.history_enabled,
            max_entries: self.history_max_entries,
            retention_days: self.history_retention_days,
        }
    }

    pub fn from_stored_value(value: serde_json::Value) -> Result<Self, serde_json::Error> {
        let has_capsule_auto_hide = value
            .as_object()
            .is_some_and(|object| object.contains_key("capsule_auto_hide"));
        let has_insertion_strategy = value
            .as_object()
            .is_some_and(|object| object.contains_key("insertion_strategy"));
        let has_hotkeys = value
            .as_object()
            .is_some_and(|object| object.contains_key("hotkeys"));
        let mut config: Self = serde_json::from_value(value)?;
        if has_hotkeys {
            config.sync_legacy_hotkey_fields_from_typed();
        }
        if !has_capsule_auto_hide {
            config.capsule_auto_hide = false;
        }
        if !has_insertion_strategy {
            config.insertion_strategy =
                insertion_strategy_from_legacy_output_mode(&config.output_mode).to_string();
        }
        config.normalize_values();
        Ok(config)
    }
}

fn default_dictation_hotkey() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Fn"
    }
    #[cfg(target_os = "windows")]
    {
        "RightAlt"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "Ctrl+/"
    }
}

fn default_dictation_hotkey_mode() -> &'static str {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        "toggle"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "hold"
    }
}

fn default_ask_hotkey() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Fn+Space"
    }
    #[cfg(target_os = "windows")]
    {
        "RightAlt+Space"
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        "Ctrl+."
    }
}

fn default_translate_hotkey() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        Some("Fn+LeftShift")
    }
    #[cfg(target_os = "windows")]
    {
        Some("RightAlt+LeftShift")
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        None
    }
}

fn normalize_hotkey_mode(value: &str) -> &'static str {
    if value == "toggle" {
        "toggle"
    } else {
        "hold"
    }
}

fn binding_from_hotkey_string(value: &str) -> Option<ShortcutBinding> {
    let parts: Vec<&str> = value
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let primary = normalize_hotkey_primary(parts.last()?)?;
    let mut modifiers = Vec::new();
    let mut seen_semantic = HashSet::new();
    for part in &parts[..parts.len() - 1] {
        let (semantic, canonical) = normalize_hotkey_modifier(part)?;
        if !seen_semantic.insert(semantic) {
            return None;
        }
        modifiers.push(canonical.to_string());
    }

    modifiers.sort_by_key(|modifier| hotkey_modifier_rank(modifier));
    Some(ShortcutBinding { primary, modifiers })
}

fn normalize_optional_binding(binding: &mut Option<ShortcutBinding>) {
    if let Some(value) = binding.as_mut() {
        if !value.normalize() {
            *binding = None;
        }
    }
}

fn normalize_hotkey_modifier(value: &str) -> Option<(&'static str, &'static str)> {
    match value.trim().to_lowercase().as_str() {
        "fn" | "function" => Some(("fn", "Fn")),
        "rightalt" | "right_alt" | "right-alt" | "altright" | "alt_right" | "alt-right" => {
            Some(("rightalt", "RightAlt"))
        }
        "ctrl" | "control" => Some(("ctrl", "Ctrl")),
        "shift" => Some(("shift", "Shift")),
        "alt" => Some(("alt", "Alt")),
        "option" => Some(("alt", "Option")),
        "meta" | "super" | "win" => Some(("super", "Super")),
        "cmd" | "command" => Some(("super", "Command")),
        _ => None,
    }
}

fn hotkey_modifier_rank(value: &str) -> u8 {
    match value {
        "Fn" | "RightAlt" => 0,
        "Command" | "Super" => 0,
        "Ctrl" => 1,
        "Option" | "Alt" => 2,
        "Shift" => 3,
        _ => 9,
    }
}

fn normalize_hotkey_primary(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = match trimmed.to_lowercase().as_str() {
        "space" => "Space".to_string(),
        "tab" => "Tab".to_string(),
        "enter" | "return" => "Enter".to_string(),
        "backspace" => "Backspace".to_string(),
        "escape" | "esc" => "Escape".to_string(),
        "delete" => "Delete".to_string(),
        "insert" => "Insert".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "leftshift" | "left_shift" | "left-shift" | "shiftleft" | "shift_left" | "shift-left" => {
            "LeftShift".to_string()
        }
        "pageup" => "PageUp".to_string(),
        "pagedown" => "PageDown".to_string(),
        "arrowup" | "up" => "Up".to_string(),
        "arrowdown" | "down" => "Down".to_string(),
        "arrowleft" | "left" => "Left".to_string(),
        "arrowright" | "right" => "Right".to_string(),
        "fn" | "function" => "Fn".to_string(),
        "rightalt" | "right_alt" | "right-alt" | "altright" | "alt_right" | "alt-right" => {
            "RightAlt".to_string()
        }
        "slash" | "/" => "/".to_string(),
        "backslash" | "\\" => "\\".to_string(),
        "period" | "." | "。" => ".".to_string(),
        "comma" | "," => ",".to_string(),
        "semicolon" | ";" => ";".to_string(),
        "quote" | "'" => "'".to_string(),
        "backquote" | "`" => "`".to_string(),
        "minus" | "-" => "-".to_string(),
        "equal" | "=" => "=".to_string(),
        "bracketleft" | "[" => "[".to_string(),
        "bracketright" | "]" => "]".to_string(),
        other
            if matches!(
                other,
                "f1" | "f2"
                    | "f3"
                    | "f4"
                    | "f5"
                    | "f6"
                    | "f7"
                    | "f8"
                    | "f9"
                    | "f10"
                    | "f11"
                    | "f12"
            ) =>
        {
            other.to_uppercase()
        }
        other if other.len() == 1 => {
            let ch = other.chars().next()?;
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase().to_string()
            } else {
                return None;
            }
        }
        _ => return None,
    };

    Some(normalized)
}

fn insertion_strategy_from_legacy_output_mode(output_mode: &str) -> &'static str {
    if output_mode == "clipboard" {
        "clipboardPaste"
    } else {
        "auto"
    }
}

fn legacy_output_mode_for_insertion_strategy(strategy: &str) -> &'static str {
    match strategy {
        "clipboardPaste" | "clipboardCopyOnly" => "clipboard",
        _ => "keyboard",
    }
}

const POLISH_CUSTOM_PROMPT_MAX_CHARS: usize = 2000;

fn normalize_polish_style(value: &str) -> &'static str {
    match value.trim() {
        "minimal" => "minimal",
        "clean" => "clean",
        "structured" => "structured",
        "professional" => "professional",
        _ => "clean",
    }
}

fn sanitize_polish_custom_prompt(value: &str) -> String {
    value
        .replace('\0', "")
        .trim()
        .chars()
        .take(POLISH_CUSTOM_PROMPT_MAX_CHARS)
        .collect()
}

fn sanitize_scene_string(value: &str, max_chars: usize) -> String {
    value
        .replace('\0', "")
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

fn sanitize_custom_scenes(scenes: &mut Vec<CustomScene>) {
    for scene in scenes.iter_mut() {
        scene.id = sanitize_scene_string(&scene.id, SCENE_ID_MAX_CHARS);
        scene.name = sanitize_scene_string(&scene.name, SCENE_NAME_MAX_CHARS);
        scene.description = sanitize_scene_string(&scene.description, SCENE_DESCRIPTION_MAX_CHARS);
        scene.prompt_template =
            sanitize_scene_string(&scene.prompt_template, SCENE_PROMPT_MAX_CHARS);
        scene.created_at = sanitize_scene_string(&scene.created_at, SCENE_ID_MAX_CHARS);
        scene.updated_at = sanitize_scene_string(&scene.updated_at, SCENE_ID_MAX_CHARS);
    }
    scenes.retain(|scene| {
        !scene.id.is_empty() && !scene.name.is_empty() && !scene.prompt_template.is_empty()
    });
    scenes.truncate(CUSTOM_SCENES_MAX_COUNT);
}

fn sanitize_active_scene(active_scene: &mut Option<ActiveScene>) {
    if let Some(scene) = active_scene.as_mut() {
        scene.id = sanitize_scene_string(&scene.id, SCENE_ID_MAX_CHARS);
        scene.source = sanitize_scene_string(&scene.source, SCENE_SOURCE_MAX_CHARS);
        scene.name = sanitize_scene_string(&scene.name, SCENE_NAME_MAX_CHARS);
        scene.prompt_template =
            sanitize_scene_string(&scene.prompt_template, SCENE_PROMPT_MAX_CHARS);

        if scene.id.is_empty()
            || scene.source.is_empty()
            || scene.name.is_empty()
            || scene.prompt_template.is_empty()
        {
            *active_scene = None;
        }
    }
}

// ─── ConfigManager (tauri-plugin-store backed) ───

pub struct ConfigManager {
    app_handle: tauri::AppHandle,
    cache: Mutex<Option<AppConfig>>,
}

impl ConfigManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            cache: Mutex::new(None),
        }
    }

    pub async fn load(&self) -> Result<AppConfig> {
        if let Some(config) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(config);
        }

        let mut config = match self.app_handle.store("settings.json") {
            Ok(store) => match store.get("app_config") {
                Some(val) => AppConfig::from_stored_value(val.clone())
                    .unwrap_or_else(|_| AppConfig::new_install_default()),
                None => AppConfig::new_install_default(),
            },
            Err(_) => AppConfig::new_install_default(),
        };

        self.migrate_legacy_config_secrets_on_load(&mut config);

        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());
        Ok(config)
    }

    pub async fn save(&self, config: &AppConfig) -> Result<()> {
        let mut config = config.clone();
        config.normalize_values();
        let report = migrate_legacy_config_secrets(&mut config, &SystemCredentialVault)?;
        if !report.migrated.is_empty() {
            tracing::info!(
                migrated_credentials = report.migrated.len(),
                "Migrated legacy config credentials before saving settings"
            );
        }
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());

        self.persist_config(&config)?;

        Ok(())
    }

    fn migrate_legacy_config_secrets_on_load(&self, config: &mut AppConfig) {
        match migrate_legacy_config_secrets(config, &SystemCredentialVault) {
            Ok(report) if !report.migrated.is_empty() => {
                tracing::info!(
                    migrated_credentials = report.migrated.len(),
                    "Migrated legacy config credentials to system credential vault"
                );
                if let Err(error) = self.persist_config(config) {
                    tracing::warn!("Failed to persist sanitized settings after migration: {error}");
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    "Failed to migrate legacy config credentials; keeping plaintext settings until a later retry: {error}"
                );
            }
        }
    }

    fn persist_config(&self, config: &AppConfig) -> Result<()> {
        let store = self
            .app_handle
            .store("settings.json")
            .map_err(|e| anyhow::anyhow!("Failed to open store: {}", e))?;
        let val = serde_json::to_value(config)?;
        store.set("app_config", val);
        store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(())
    }
}

// ─── HistoryStore (SQLite backed) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub app_name: String,
    pub app_type: String,
    pub raw_text: String,
    pub polished_text: String,
    pub language: Option<String>,
    pub duration_ms: Option<i64>,
    pub active_scene_id: Option<String>,
    pub active_scene_source: Option<String>,
    pub active_scene_name: Option<String>,
    pub active_scene_prompt_chars: Option<i64>,
    pub active_scene_prompt_truncated: bool,
    pub output_status: Option<String>,
    pub output_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRetentionPolicy {
    pub enabled: bool,
    pub max_entries: u32,
    /// 0 means keep indefinitely by age.
    pub retention_days: u32,
}

impl Default for HistoryRetentionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: DEFAULT_HISTORY_MAX_ENTRIES,
            retention_days: 0,
        }
    }
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                app_type TEXT NOT NULL DEFAULT '',
                raw_text TEXT NOT NULL DEFAULT '',
                polished_text TEXT NOT NULL DEFAULT '',
                language TEXT,
                duration_ms INTEGER,
                active_scene_id TEXT,
                active_scene_source TEXT,
                active_scene_name TEXT,
                active_scene_prompt_chars INTEGER,
                active_scene_prompt_truncated INTEGER NOT NULL DEFAULT 0,
                output_status TEXT,
                output_error TEXT
            );",
        )?;
        ensure_history_optional_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn add(&self, entry: HistoryEntry) -> Result<()> {
        self.add_with_policy(entry, &HistoryRetentionPolicy::default())
            .await
    }

    pub async fn add_with_policy(
        &self,
        entry: HistoryEntry,
        policy: &HistoryRetentionPolicy,
    ) -> Result<()> {
        if !policy.enabled {
            return Ok(());
        }

        let now_iso = entry.created_at.clone();
        {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            conn.execute(
                "INSERT INTO history (
                    created_at,
                    app_name,
                    app_type,
                    raw_text,
                    polished_text,
                    language,
                    duration_ms,
                    active_scene_id,
                    active_scene_source,
                    active_scene_name,
                    active_scene_prompt_chars,
                    active_scene_prompt_truncated,
                    output_status,
                    output_error
                )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                rusqlite::params![
                    entry.created_at,
                    entry.app_name,
                    entry.app_type,
                    entry.raw_text,
                    entry.polished_text,
                    entry.language,
                    entry.duration_ms,
                    entry.active_scene_id,
                    entry.active_scene_source,
                    entry.active_scene_name,
                    entry.active_scene_prompt_chars,
                    entry.active_scene_prompt_truncated,
                    entry.output_status,
                    entry.output_error,
                ],
            )?;
        }

        self.prune_with_policy(policy, &now_iso).await?;
        Ok(())
    }

    pub async fn prune_with_policy(
        &self,
        policy: &HistoryRetentionPolicy,
        now_iso: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if !policy.enabled {
            conn.execute("DELETE FROM history", [])?;
            return Ok(());
        }

        let max_entries = policy.max_entries.clamp(1, DEFAULT_HISTORY_MAX_ENTRIES);
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
            rusqlite::params![max_entries],
        )?;

        if policy.retention_days > 0 {
            if let Ok(now) = chrono::NaiveDateTime::parse_from_str(now_iso, "%Y-%m-%dT%H:%M:%S") {
                let cutoff = now - chrono::Duration::days(policy.retention_days as i64);
                let cutoff_iso = cutoff.format("%Y-%m-%dT%H:%M:%S").to_string();
                conn.execute(
                    "DELETE FROM history WHERE created_at < ?1",
                    rusqlite::params![cutoff_iso],
                )?;
            }
        }

        Ok(())
    }

    pub async fn list(&self, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT
                id,
                created_at,
                app_name,
                app_type,
                raw_text,
                polished_text,
                language,
                duration_ms,
                active_scene_id,
                active_scene_source,
                active_scene_name,
                active_scene_prompt_chars,
                active_scene_prompt_truncated,
                output_status,
                output_error
             FROM history ORDER BY id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                created_at: row.get(1)?,
                app_name: row.get(2)?,
                app_type: row.get(3)?,
                raw_text: row.get(4)?,
                polished_text: row.get(5)?,
                language: row.get(6)?,
                duration_ms: row.get(7)?,
                active_scene_id: row.get(8)?,
                active_scene_source: row.get(9)?,
                active_scene_name: row.get(10)?,
                active_scene_prompt_chars: row.get(11)?,
                active_scene_prompt_truncated: row.get(12)?,
                output_status: row.get(13)?,
                output_error: row.get(14)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

fn ensure_history_optional_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(history)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;

    for (name, ddl) in [
        ("active_scene_id", "ALTER TABLE history ADD COLUMN active_scene_id TEXT"),
        (
            "active_scene_source",
            "ALTER TABLE history ADD COLUMN active_scene_source TEXT",
        ),
        (
            "active_scene_name",
            "ALTER TABLE history ADD COLUMN active_scene_name TEXT",
        ),
        (
            "active_scene_prompt_chars",
            "ALTER TABLE history ADD COLUMN active_scene_prompt_chars INTEGER",
        ),
        (
            "active_scene_prompt_truncated",
            "ALTER TABLE history ADD COLUMN active_scene_prompt_truncated INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "output_status",
            "ALTER TABLE history ADD COLUMN output_status TEXT",
        ),
        (
            "output_error",
            "ALTER TABLE history ADD COLUMN output_error TEXT",
        ),
    ] {
        if !columns.contains(name) {
            conn.execute(ddl, [])?;
        }
    }

    Ok(())
}

// ─── DictionaryStore (SQLite backed) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: i64,
    pub word: String,
    pub pronunciation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorrectionRule {
    pub id: i64,
    pub pattern: String,
    pub replacement: String,
    pub enabled: bool,
}

pub struct DictionaryStore {
    conn: Mutex<Connection>,
}

impl DictionaryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dictionary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL,
                pronunciation TEXT
            );
            CREATE TABLE IF NOT EXISTS correction_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern TEXT NOT NULL,
                replacement TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn add(&self, word: &str, pronunciation: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO dictionary (word, pronunciation) VALUES (?1, ?2)",
            rusqlite::params![word, pronunciation],
        )?;
        Ok(())
    }

    pub async fn remove(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM dictionary WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare("SELECT id, word, pronunciation FROM dictionary")?;
        let rows = stmt.query_map([], |row| {
            Ok(DictionaryEntry {
                id: row.get(0)?,
                word: row.get(1)?,
                pronunciation: row.get(2)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn words(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = match conn.prepare("SELECT word FROM dictionary") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub async fn add_correction(&self, pattern: &str, replacement: &str) -> Result<()> {
        let pattern = sanitize_correction_text(pattern);
        let replacement = sanitize_correction_text(replacement);
        if pattern.is_empty() || replacement.is_empty() {
            anyhow::bail!("Correction rule cannot be empty");
        }

        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO correction_rules (pattern, replacement, enabled) VALUES (?1, ?2, 1)",
            rusqlite::params![pattern, replacement],
        )?;
        Ok(())
    }

    pub async fn remove_correction(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM correction_rules WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub async fn set_correction_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "UPDATE correction_rules SET enabled = ?2 WHERE id = ?1",
            rusqlite::params![id, if enabled { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub async fn correction_rules(&self) -> Result<Vec<CorrectionRule>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, pattern, replacement, enabled FROM correction_rules ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CorrectionRule {
                id: row.get(0)?,
                pattern: row.get(1)?,
                replacement: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn enabled_correction_rules(&self) -> Vec<CorrectionRule> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = match conn.prepare(
            "SELECT id, pattern, replacement, enabled FROM correction_rules WHERE enabled = 1 ORDER BY id ASC LIMIT 100",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| {
            Ok(CorrectionRule {
                id: row.get(0)?,
                pattern: row.get(1)?,
                replacement: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
            })
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }
}

fn sanitize_correction_text(value: &str) -> String {
    value.replace('\0', "").trim().chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_defaults_missing_custom_stt_api_key() {
        let value = serde_json::json!({
            "stt_provider": "deepgram",
            "stt_api_key": "hosted-secret"
        });

        let config: AppConfig = serde_json::from_value(value).unwrap();

        assert_eq!(config.stt_provider, "deepgram");
        assert_eq!(config.stt_api_key, "hosted-secret");
        assert_eq!(config.stt_custom_api_key, "");
        assert_eq!(
            config.stt_volcengine_resource_id,
            crate::stt::volcengine::VOLCENGINE_SEEDASR_RESOURCE_ID
        );
    }

    #[test]
    fn app_config_defaults_missing_polish_preferences() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "stt_api_key": "",
            "stt_language": "multi",
            "llm_provider": "openrouter",
            "llm_api_key": "",
            "llm_model": "google/gemini-2.5-flash",
            "llm_base_url": "https://openrouter.ai/api/v1",
            "polish_enabled": true,
            "translate_enabled": false,
            "target_lang": "en",
            "hotkey": "Ctrl+/",
            "hotkey_mode": "hold",
            "output_mode": "keyboard",
            "selected_text_enabled": false,
            "theme": "system",
            "auto_start": false,
            "close_to_tray": true,
            "start_minimized": false,
            "max_recording_seconds": 30,
            "ui_language": "en"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_custom_prompt, "");
        assert_eq!(config.polish_chinese_script, "preserve");
        assert_eq!(config.polish_style, "clean");
    }

    #[test]
    fn app_config_preserves_valid_polish_style() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["polish_style"] = serde_json::json!("structured");

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_style, "structured");
    }

    #[test]
    fn app_config_normalizes_invalid_polish_style_to_clean() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["polish_style"] = serde_json::json!("marketplace-pack");

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_style, "clean");
    }

    #[test]
    fn app_config_defaults_missing_clipboard_restore_preferences() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "output_mode": "clipboard"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(config.restore_clipboard_after_paste);
        assert_eq!(config.paste_shortcut, "ctrlV");
    }

    #[test]
    fn app_config_defaults_and_normalizes_history_privacy_settings() {
        let default_value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter"
        });
        let default_config = AppConfig::from_stored_value(default_value).unwrap();
        assert!(default_config.history_enabled);
        assert_eq!(default_config.history_retention_days, 0);
        assert_eq!(default_config.history_max_entries, 5000);

        let invalid_value = serde_json::json!({
            "history_enabled": true,
            "history_retention_days": 99999,
            "history_max_entries": 99999
        });
        let invalid_config = AppConfig::from_stored_value(invalid_value).unwrap();
        assert_eq!(invalid_config.history_retention_days, 3650);
        assert_eq!(invalid_config.history_max_entries, 5000);
    }

    #[test]
    fn shortcut_binding_accepts_native_single_key_triggers() {
        let fn_binding = ShortcutBinding::from_hotkey("Fn").expect("Fn parses");
        assert_eq!(fn_binding.primary, "Fn");
        assert!(fn_binding.modifiers.is_empty());
        assert_eq!(fn_binding.to_hotkey_string().as_deref(), Some("Fn"));

        let right_alt = ShortcutBinding::from_hotkey("RightAlt").expect("RightAlt parses");
        assert_eq!(right_alt.primary, "RightAlt");
        assert!(right_alt.modifiers.is_empty());
        assert_eq!(right_alt.to_hotkey_string().as_deref(), Some("RightAlt"));
    }

    #[test]
    fn shortcut_binding_accepts_native_mode_shortcuts() {
        let ask = ShortcutBinding::from_hotkey("Fn+Space").expect("Fn+Space parses");
        assert_eq!(ask.primary, "Space");
        assert_eq!(ask.modifiers, vec!["Fn".to_string()]);
        assert_eq!(ask.to_hotkey_string().as_deref(), Some("Fn+Space"));

        let translate =
            ShortcutBinding::from_hotkey("RightAlt+LeftShift").expect("RightAlt+LeftShift parses");
        assert_eq!(translate.primary, "LeftShift");
        assert_eq!(translate.modifiers, vec!["RightAlt".to_string()]);
        assert_eq!(
            translate.to_hotkey_string().as_deref(),
            Some("RightAlt+LeftShift")
        );
    }

    #[test]
    fn app_config_defaults_streaming_insert_to_disabled() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(!config.streaming_insert_enabled);
    }

    #[test]
    fn app_config_migrates_legacy_keyboard_output_to_auto_insertion_strategy() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "output_mode": "keyboard"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.insertion_strategy, "auto");
        assert_eq!(config.output_mode, "keyboard");
    }

    #[test]
    fn app_config_migrates_legacy_clipboard_output_to_clipboard_paste_strategy() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "output_mode": "clipboard"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.insertion_strategy, "clipboardPaste");
        assert_eq!(config.output_mode, "clipboard");
    }

    #[test]
    fn app_config_migrates_legacy_hotkeys_to_typed_config() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "hotkey": "Ctrl+Shift+;",
            "ask_hotkey": "Ctrl+.",
            "hotkey_mode": "toggle"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(
            config.hotkeys.dictation,
            ShortcutBinding {
                primary: ";".to_string(),
                modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            }
        );
        assert_eq!(
            config.hotkeys.ask,
            Some(ShortcutBinding {
                primary: ".".to_string(),
                modifiers: vec!["Ctrl".to_string()],
            })
        );
        assert_eq!(config.hotkeys.dictation_mode, "toggle");
        assert_eq!(config.hotkey, "Ctrl+Shift+;");
        assert_eq!(config.ask_hotkey, "Ctrl+.");
        assert_eq!(config.hotkey_mode, "toggle");
    }

    #[test]
    fn app_config_uses_typed_hotkeys_when_present() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "hotkey": "Ctrl+/",
            "ask_hotkey": "Ctrl+.",
            "hotkey_mode": "hold",
            "hotkeys": {
                "dictation": { "primary": "-", "modifiers": ["Ctrl", "Shift"] },
                "ask": { "primary": ".", "modifiers": ["Ctrl"] },
                "translate": null,
                "editSelection": null,
                "switchScene": null,
                "openApp": null,
                "dictationMode": "toggle"
            }
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Ctrl+Shift+-");
        assert_eq!(config.ask_hotkey, "Ctrl+.");
        assert_eq!(config.hotkey_mode, "toggle");
        assert_eq!(config.hotkeys.dictation.primary, "-");
        assert_eq!(config.hotkeys.dictation.modifiers, vec!["Ctrl", "Shift"]);
    }

    #[test]
    fn app_config_preserves_extended_typed_hotkey_roles_when_present() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "hotkey": "Ctrl+/",
            "ask_hotkey": "Ctrl+.",
            "hotkey_mode": "hold",
            "hotkeys": {
                "dictation": { "primary": "-", "modifiers": ["Ctrl", "Shift"] },
                "ask": null,
                "translate": { "primary": "T", "modifiers": ["Ctrl", "Shift"] },
                "editSelection": { "primary": "E", "modifiers": ["Ctrl", "Shift"] },
                "switchScene": { "primary": "S", "modifiers": ["Ctrl", "Shift"] },
                "openApp": { "primary": "O", "modifiers": ["Ctrl", "Shift"] },
                "dictationMode": "toggle"
            }
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Ctrl+Shift+-");
        assert_eq!(config.ask_hotkey, "");
        assert_eq!(config.hotkeys.ask, None);
        assert_eq!(
            config.hotkeys.translate,
            Some(ShortcutBinding {
                primary: "T".to_string(),
                modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            })
        );
        assert_eq!(
            config.hotkeys.edit_selection,
            Some(ShortcutBinding {
                primary: "E".to_string(),
                modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            })
        );
        assert_eq!(
            config.hotkeys.switch_scene,
            Some(ShortcutBinding {
                primary: "S".to_string(),
                modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            })
        );
        assert_eq!(
            config.hotkeys.open_app,
            Some(ShortcutBinding {
                primary: "O".to_string(),
                modifiers: vec!["Ctrl".to_string(), "Shift".to_string()],
            })
        );
    }

    #[test]
    fn app_config_preserves_explicit_copy_only_insertion_strategy() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "output_mode": "keyboard",
            "insertion_strategy": "clipboardCopyOnly"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.insertion_strategy, "clipboardCopyOnly");
        assert_eq!(config.output_mode, "clipboard");
    }

    #[test]
    fn app_config_defaults_and_normalizes_windows_sendinput_newline_mode() {
        let default_value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter"
        });
        let default_config = AppConfig::from_stored_value(default_value).unwrap();
        assert_eq!(default_config.windows_sendinput_newline_mode, "enter");

        let explicit_value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "windows_sendinput_newline_mode": "shiftEnter"
        });
        let explicit_config = AppConfig::from_stored_value(explicit_value).unwrap();
        assert_eq!(explicit_config.windows_sendinput_newline_mode, "shiftEnter");

        let invalid_value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter",
            "windows_sendinput_newline_mode": "invalid"
        });
        let invalid_config = AppConfig::from_stored_value(invalid_value).unwrap();
        assert_eq!(invalid_config.windows_sendinput_newline_mode, "enter");
    }

    #[test]
    fn app_config_defaults_missing_custom_scenes() {
        let value = serde_json::json!({
            "stt_provider": "glm-asr",
            "llm_provider": "openrouter"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(config.custom_scenes.is_empty());
        assert!(config.active_scene.is_none());
    }

    #[test]
    fn app_config_sanitizes_custom_scenes_and_active_scene() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["custom_scenes"] = serde_json::json!([
            {
                "id": "  custom_keep  ",
                "name": format!("  Scene\0{}  ", "x".repeat(100)),
                "description": format!("  Desc\0{}  ", "y".repeat(300)),
                "prompt_template": format!("  Prompt\0{}  ", "z".repeat(5000)),
                "created_at": "  2026-06-30T00:00:00.000Z  ",
                "updated_at": "  2026-06-30T00:00:00.000Z  "
            }
        ]);
        value["active_scene"] = serde_json::json!({
            "id": "  custom_keep  ",
            "source": "custom",
            "name": "  Active\0 Scene  ",
            "prompt_template": "  Use bullets.\0  "
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.custom_scenes.len(), 1);
        let scene = &config.custom_scenes[0];
        assert_eq!(scene.id, "custom_keep");
        assert_eq!(scene.name.chars().count(), 80);
        assert!(scene.name.starts_with("Scene"));
        assert!(!scene.name.contains('\0'));
        assert_eq!(scene.description.chars().count(), 240);
        assert_eq!(scene.prompt_template.chars().count(), 4000);

        let active_scene = config
            .active_scene
            .expect("active scene should remain valid");
        assert_eq!(active_scene.id, "custom_keep");
        assert_eq!(active_scene.name, "Active Scene");
        assert_eq!(active_scene.prompt_template, "Use bullets.");
    }

    #[test]
    fn app_config_clears_empty_active_scene() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["active_scene"] = serde_json::json!({
            "id": "custom_empty",
            "source": "custom",
            "name": "Empty",
            "prompt_template": "   \0  "
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(config.active_scene.is_none());
    }

    #[test]
    fn app_config_sanitizes_custom_polish_prompt_and_clears_chinese_script() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["polish_custom_prompt"] = serde_json::json!("  use formal tone\0  ");
        value["polish_chinese_script"] = serde_json::json!("traditional");

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.polish_custom_prompt, "use formal tone");
        assert_eq!(config.polish_chinese_script, "preserve");
    }

    #[test]
    fn app_config_new_install_defaults_capsule_auto_hide_true() {
        let config = AppConfig::new_install_default();
        assert!(config.capsule_auto_hide);
        assert!(config.auto_start);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_config_new_install_uses_fn_toggle_on_macos() {
        let config = AppConfig::new_install_default();
        assert_eq!(config.hotkey, "Fn");
        assert_eq!(config.hotkeys.dictation.primary, "Fn");
        assert_eq!(config.hotkeys.dictation.modifiers, Vec::<String>::new());
        assert_eq!(config.hotkey_mode, "toggle");
        assert_eq!(config.hotkeys.dictation_mode, "toggle");
        assert_eq!(config.ask_hotkey, "Fn+Space");
        assert_eq!(
            config
                .hotkeys
                .ask
                .as_ref()
                .and_then(ShortcutBinding::to_hotkey_string),
            Some("Fn+Space".to_string())
        );
        assert_eq!(
            config
                .hotkeys
                .translate
                .as_ref()
                .and_then(ShortcutBinding::to_hotkey_string),
            Some("Fn+LeftShift".to_string())
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn app_config_new_install_uses_right_alt_toggle_on_windows() {
        let config = AppConfig::new_install_default();
        assert_eq!(config.hotkey, "RightAlt");
        assert_eq!(config.hotkeys.dictation.primary, "RightAlt");
        assert_eq!(config.hotkeys.dictation.modifiers, Vec::<String>::new());
        assert_eq!(config.hotkey_mode, "toggle");
        assert_eq!(config.hotkeys.dictation_mode, "toggle");
        assert_eq!(config.ask_hotkey, "RightAlt+Space");
        assert_eq!(
            config
                .hotkeys
                .ask
                .as_ref()
                .and_then(ShortcutBinding::to_hotkey_string),
            Some("RightAlt+Space".to_string())
        );
        assert_eq!(
            config
                .hotkeys
                .translate
                .as_ref()
                .and_then(ShortcutBinding::to_hotkey_string),
            Some("RightAlt+LeftShift".to_string())
        );
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    #[test]
    fn app_config_new_install_keeps_ctrl_slash_on_linux() {
        let config = AppConfig::new_install_default();
        assert_eq!(config.hotkey, "Ctrl+/");
        assert_eq!(config.hotkey_mode, "hold");
        assert_eq!(config.ask_hotkey, "Ctrl+.");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_config_migrates_old_mac_default_hotkey_to_fn() {
        let value = serde_json::json!({
            "hotkey": "Option+/",
            "ask_hotkey": "Command+.",
            "hotkey_mode": "hold"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Fn");
        assert_eq!(config.hotkeys.dictation.primary, "Fn");
        assert_eq!(config.hotkeys.dictation.modifiers, Vec::<String>::new());
        assert_eq!(config.hotkey_mode, "toggle");
        assert_eq!(config.hotkeys.dictation_mode, "toggle");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn app_config_migrates_old_windows_default_hotkey_to_right_alt() {
        let value = serde_json::json!({
            "hotkey": "Ctrl+/",
            "ask_hotkey": "Ctrl+.",
            "hotkey_mode": "hold"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "RightAlt");
        assert_eq!(config.hotkeys.dictation.primary, "RightAlt");
        assert_eq!(config.hotkeys.dictation.modifiers, Vec::<String>::new());
        assert_eq!(config.hotkey_mode, "toggle");
        assert_eq!(config.hotkeys.dictation_mode, "toggle");
    }

    #[test]
    fn app_config_preserves_custom_hotkey_during_native_default_migration() {
        let value = serde_json::json!({
            "hotkey": "Ctrl+Shift+;",
            "ask_hotkey": "Ctrl+.",
            "hotkey_mode": "hold"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Ctrl+Shift+;");
        assert_eq!(config.hotkeys.dictation.primary, ";");
        assert_eq!(
            config.hotkeys.dictation.modifiers,
            vec!["Ctrl".to_string(), "Shift".to_string()]
        );
        assert_eq!(config.hotkey_mode, "hold");
        assert_eq!(config.hotkeys.dictation_mode, "hold");
    }

    #[test]
    fn app_config_existing_missing_capsule_auto_hide_defaults_false() {
        let value = serde_json::json!({
            "stt_provider": "deepgram",
            "stt_api_key": "hosted-secret"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.stt_provider, "deepgram");
        assert_eq!(config.stt_api_key, "hosted-secret");
        assert!(!config.capsule_auto_hide);
    }

    #[test]
    fn app_config_existing_explicit_capsule_auto_hide_is_preserved() {
        let value = serde_json::json!({
            "capsule_auto_hide": true
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert!(config.capsule_auto_hide);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_config_migrates_legacy_mac_alt_slash_label() {
        let value = serde_json::json!({
            "hotkey": "Alt+/"
        });

        let config = AppConfig::from_stored_value(value).unwrap();

        assert_eq!(config.hotkey, "Option+/");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_config_migrates_legacy_mac_ask_hotkey_defaults() {
        for legacy in [
            "Alt+Shift+/",
            "Option+Shift+/",
            "Command+Shift+/",
            "Command+/",
            "Command+。",
        ] {
            let value = serde_json::json!({
                "ask_hotkey": legacy
            });

            let config = AppConfig::from_stored_value(value).unwrap();

            assert_eq!(config.ask_hotkey, "Fn+Space");
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn app_config_migrates_legacy_non_macos_ask_hotkey_defaults() {
        for legacy in ["Ctrl+Shift+/", "Control+Shift+/", "Ctrl+/", "Control+/"] {
            let value = serde_json::json!({
                "ask_hotkey": legacy
            });

            let config = AppConfig::from_stored_value(value).unwrap();

            #[cfg(target_os = "windows")]
            assert_eq!(config.ask_hotkey, "RightAlt+Space");

            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            assert_eq!(config.ask_hotkey, "Ctrl+.");
        }
    }

    fn test_history_entry(id: i64, created_at: &str) -> HistoryEntry {
        HistoryEntry {
            id,
            created_at: created_at.to_string(),
            app_name: "Notes".to_string(),
            app_type: "General".to_string(),
            raw_text: format!("raw {id}"),
            polished_text: format!("polished {id}"),
            language: None,
            duration_ms: None,
            active_scene_id: None,
            active_scene_source: None,
            active_scene_name: None,
            active_scene_prompt_chars: None,
            active_scene_prompt_truncated: false,
            output_status: None,
            output_error: None,
        }
    }

    fn temp_history_store(name: &str) -> HistoryStore {
        let path = std::env::temp_dir().join(format!(
            "opentypeless-history-test-{}-{}.sqlite",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        HistoryStore::new(path).unwrap()
    }

    fn temp_dictionary_store(name: &str) -> DictionaryStore {
        let path = std::env::temp_dir().join(format!(
            "opentypeless-dictionary-test-{}-{}.sqlite",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        DictionaryStore::new(path).unwrap()
    }

    #[tokio::test]
    async fn history_store_respects_disabled_policy() {
        let store = temp_history_store("disabled");
        let policy = HistoryRetentionPolicy {
            enabled: false,
            max_entries: 5000,
            retention_days: 0,
        };

        store
            .add_with_policy(test_history_entry(1, "2026-07-01T00:00:00"), &policy)
            .await
            .unwrap();

        assert!(store.list(10, 0).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_store_prunes_by_max_entries_policy() {
        let store = temp_history_store("max");
        let policy = HistoryRetentionPolicy {
            enabled: true,
            max_entries: 2,
            retention_days: 0,
        };

        for id in 1..=3 {
            store
                .add_with_policy(
                    test_history_entry(id, &format!("2026-07-0{id}T00:00:00")),
                    &policy,
                )
                .await
                .unwrap();
        }

        let entries = store.list(10, 0).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].polished_text, "polished 3");
        assert_eq!(entries[1].polished_text, "polished 2");
    }

    #[tokio::test]
    async fn history_store_prunes_by_retention_days_policy() {
        let store = temp_history_store("days");
        let policy = HistoryRetentionPolicy {
            enabled: true,
            max_entries: 5000,
            retention_days: 7,
        };

        store
            .add_with_policy(test_history_entry(1, "2026-06-25T00:00:00"), &policy)
            .await
            .unwrap();
        store
            .add_with_policy(test_history_entry(2, "2026-07-01T00:00:00"), &policy)
            .await
            .unwrap();
        store
            .prune_with_policy(&policy, "2026-07-05T00:00:00")
            .await
            .unwrap();

        let entries = store.list(10, 0).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].polished_text, "polished 2");
    }

    #[tokio::test]
    async fn history_store_persists_active_scene_diagnostics() {
        let store = temp_history_store("scene-diagnostics");
        let mut entry = test_history_entry(1, "2026-07-01T00:00:00");
        entry.active_scene_id = Some("builtin_meeting_notes".to_string());
        entry.active_scene_source = Some("builtin".to_string());
        entry.active_scene_name = Some("Meeting Notes".to_string());
        entry.active_scene_prompt_chars = Some(128);
        entry.active_scene_prompt_truncated = false;

        store.add(entry).await.unwrap();

        let entries = store.list(10, 0).await.unwrap();
        assert_eq!(
            entries[0].active_scene_id.as_deref(),
            Some("builtin_meeting_notes")
        );
        assert_eq!(entries[0].active_scene_source.as_deref(), Some("builtin"));
        assert_eq!(
            entries[0].active_scene_name.as_deref(),
            Some("Meeting Notes")
        );
        assert_eq!(entries[0].active_scene_prompt_chars, Some(128));
        assert!(!entries[0].active_scene_prompt_truncated);
    }

    #[tokio::test]
    async fn history_store_persists_output_status_diagnostics() {
        let store = temp_history_store("output-diagnostics");
        let mut entry = test_history_entry(1, "2026-07-01T00:00:00");
        entry.output_status = Some("partial".to_string());
        entry.output_error = Some("LLM failed after partial streaming insert".to_string());

        store.add(entry).await.unwrap();

        let entries = store.list(10, 0).await.unwrap();
        assert_eq!(entries[0].output_status.as_deref(), Some("partial"));
        assert_eq!(
            entries[0].output_error.as_deref(),
            Some("LLM failed after partial streaming insert")
        );
    }

    #[tokio::test]
    async fn history_store_migrates_legacy_history_table_scene_columns() {
        let path = std::env::temp_dir().join(format!(
            "opentypeless-history-test-legacy-scene-{}.sqlite",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    created_at TEXT NOT NULL,
                    app_name TEXT NOT NULL DEFAULT '',
                    app_type TEXT NOT NULL DEFAULT '',
                    raw_text TEXT NOT NULL DEFAULT '',
                    polished_text TEXT NOT NULL DEFAULT '',
                    language TEXT,
                    duration_ms INTEGER
                );",
            )
            .unwrap();
        }

        let store = HistoryStore::new(path).unwrap();
        let mut entry = test_history_entry(1, "2026-07-01T00:00:00");
        entry.active_scene_id = Some("custom_focus".to_string());
        entry.active_scene_source = Some("custom".to_string());
        entry.active_scene_name = Some("Focus".to_string());
        entry.active_scene_prompt_chars = Some(64);
        entry.active_scene_prompt_truncated = true;

        store.add(entry).await.unwrap();

        let entries = store.list(10, 0).await.unwrap();
        assert_eq!(entries[0].active_scene_id.as_deref(), Some("custom_focus"));
        assert!(entries[0].active_scene_prompt_truncated);
        assert!(entries[0].output_status.is_none());
        assert!(entries[0].output_error.is_none());
    }

    #[tokio::test]
    async fn dictionary_store_persists_correction_rules() {
        let store = temp_dictionary_store("correction-rules");

        store.add_correction(" 拓肯 ", " Token ").await.unwrap();
        let rules = store.correction_rules().await.unwrap();

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "拓肯");
        assert_eq!(rules[0].replacement, "Token");
        assert!(rules[0].enabled);
    }

    #[tokio::test]
    async fn dictionary_store_ignores_disabled_correction_rules_for_prompt() {
        let store = temp_dictionary_store("correction-rules-disabled");

        store.add_correction("克劳德", "Claude").await.unwrap();
        let rules = store.correction_rules().await.unwrap();
        store
            .set_correction_enabled(rules[0].id, false)
            .await
            .unwrap();

        let enabled = store.enabled_correction_rules().await;

        assert!(enabled.is_empty());
    }
}
