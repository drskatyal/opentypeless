use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

use super::registry::AppRegistry;
use super::types::{ContextFamily, ContextProfile, ContextSignals, ContextSource};

const MAPPING_STORE_FILE: &str = "context-mappings.json";
const MAPPING_STORE_KEY: &str = "mapping_state";
const MAPPING_STORE_VERSION: u32 = 1;
const MAX_MAPPING_LABEL_CHARS: usize = 40;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum UserAppMatcher {
    NativeBundleId(String),
    NativeExecutable(String),
    ExactWebHost(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomAppMapping {
    pub id: String,
    pub label: String,
    pub matcher: UserAppMatcher,
    pub family: ContextFamily,
    pub scene_id: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserAppMatcherType {
    NativeBundleId,
    NativeExecutable,
    ExactWebHost,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MappingCandidateView {
    pub generation: u64,
    pub matcher_type: UserAppMatcherType,
    pub display_value: String,
    pub suggested_label: String,
    pub current_family: ContextFamily,
    pub icon_key: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomAppMappingView {
    pub id: String,
    pub label: String,
    pub matcher_type: UserAppMatcherType,
    pub display_value: String,
    pub family: ContextFamily,
    pub scene_id: Option<String>,
    pub enabled: bool,
    pub icon_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MappingCandidate {
    pub(crate) generation: u64,
    pub(crate) matcher: UserAppMatcher,
    pub(crate) suggested_label: String,
    pub(crate) current_family: ContextFamily,
    pub(crate) icon_key: String,
}

impl MappingCandidate {
    pub(crate) fn view(&self) -> MappingCandidateView {
        MappingCandidateView {
            generation: self.generation,
            matcher_type: self.matcher.matcher_type(),
            display_value: matcher_display_value(&self.matcher, &self.suggested_label),
            suggested_label: self.suggested_label.clone(),
            current_family: self.current_family,
            icon_key: self.icon_key.clone(),
        }
    }
}

impl UserAppMatcher {
    fn matcher_type(&self) -> UserAppMatcherType {
        match self {
            Self::NativeBundleId(_) => UserAppMatcherType::NativeBundleId,
            Self::NativeExecutable(_) => UserAppMatcherType::NativeExecutable,
            Self::ExactWebHost(_) => UserAppMatcherType::ExactWebHost,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UserAppMappingCollection {
    mappings: Vec<CustomAppMapping>,
}

impl UserAppMappingCollection {
    fn from_stored(mappings: Vec<CustomAppMapping>) -> Self {
        let mut seen = HashSet::new();
        let mut normalized = Vec::new();
        for mut mapping in mappings {
            let Ok(matcher) = normalize_matcher(mapping.matcher) else {
                continue;
            };
            if !seen.insert(matcher.clone()) {
                continue;
            }
            let label = sanitize_mapping_label(&mapping.label);
            if label.is_empty() {
                continue;
            }
            mapping.id = if Uuid::parse_str(&mapping.id).is_ok() {
                mapping.id
            } else {
                Uuid::new_v4().to_string()
            };
            mapping.label = label;
            mapping.matcher = matcher;
            mapping.scene_id = sanitize_scene_id(mapping.scene_id);
            normalized.push(mapping);
        }
        Self {
            mappings: normalized,
        }
    }

    pub(crate) fn mappings(&self) -> &[CustomAppMapping] {
        &self.mappings
    }

    pub(crate) fn save_candidate(
        &mut self,
        candidate: &MappingCandidate,
        label: &str,
        family: ContextFamily,
        scene_id: Option<String>,
    ) -> Result<CustomAppMapping, String> {
        let matcher = normalize_matcher(candidate.matcher.clone())?;
        if self
            .mappings
            .iter()
            .any(|mapping| mapping.matcher == matcher)
        {
            return Err("custom_app_mapping_duplicate".to_string());
        }
        let label = sanitize_mapping_label(label);
        if label.is_empty() {
            return Err("custom_app_mapping_label_empty".to_string());
        }
        let mapping = CustomAppMapping {
            id: Uuid::new_v4().to_string(),
            label,
            matcher,
            family,
            scene_id: sanitize_scene_id(scene_id),
            enabled: true,
        };
        self.mappings.push(mapping.clone());
        Ok(mapping)
    }

    pub(crate) fn update(
        &mut self,
        id: &str,
        label: &str,
        family: ContextFamily,
        scene_id: Option<String>,
        enabled: bool,
    ) -> Result<CustomAppMapping, String> {
        let label = sanitize_mapping_label(label);
        if label.is_empty() {
            return Err("custom_app_mapping_label_empty".to_string());
        }
        let mapping = self
            .mappings
            .iter_mut()
            .find(|mapping| mapping.id == id)
            .ok_or_else(|| "custom_app_mapping_not_found".to_string())?;
        mapping.label = label;
        mapping.family = family;
        mapping.scene_id = sanitize_scene_id(scene_id);
        mapping.enabled = enabled;
        Ok(mapping.clone())
    }

    pub(crate) fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        let mapping = self
            .mappings
            .iter_mut()
            .find(|mapping| mapping.id == id)
            .ok_or_else(|| "custom_app_mapping_not_found".to_string())?;
        mapping.enabled = enabled;
        Ok(())
    }

    pub(crate) fn delete(&mut self, id: &str) -> Result<(), String> {
        let initial_len = self.mappings.len();
        self.mappings.retain(|mapping| mapping.id != id);
        if self.mappings.len() == initial_len {
            return Err("custom_app_mapping_not_found".to_string());
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        self.mappings.clear();
    }

    pub(crate) fn find_match(&self, signals: &ContextSignals) -> Option<&CustomAppMapping> {
        let web_matcher = signals
            .browser_host
            .as_deref()
            .filter(|_| signals.is_supported_browser)
            .and_then(|host| normalize_web_host(host).ok())
            .map(UserAppMatcher::ExactWebHost);
        if let Some(matcher) = web_matcher.as_ref() {
            if let Some(mapping) = self
                .mappings
                .iter()
                .find(|mapping| mapping.enabled && &mapping.matcher == matcher)
            {
                return Some(mapping);
            }
        }

        let identity = signals
            .native_identity
            .as_deref()
            .or(signals.process_alias.as_deref());
        let identity = identity?;
        self.mappings.iter().find(|mapping| {
            if !mapping.enabled {
                return false;
            }
            match &mapping.matcher {
                UserAppMatcher::NativeBundleId(value) => {
                    normalize_bundle_id(identity).is_ok_and(|identity| identity == *value)
                }
                UserAppMatcher::NativeExecutable(value) => {
                    normalize_executable(identity).is_ok_and(|identity| identity == *value)
                }
                UserAppMatcher::ExactWebHost(_) => false,
            }
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedProfile {
    pub(crate) profile: ContextProfile,
    pub(crate) mapped_scene_id: Option<String>,
}

pub(crate) fn resolve_profile(
    registry: &AppRegistry,
    mappings: &UserAppMappingCollection,
    signals: &ContextSignals,
) -> ResolvedProfile {
    let builtin = registry.classify(signals);
    let Some(mapping) = mappings.find_match(signals) else {
        return ResolvedProfile {
            profile: builtin,
            mapped_scene_id: None,
        };
    };
    ResolvedProfile {
        profile: ContextProfile {
            id: format!("user.{}", mapping.id.replace('-', "")),
            family: mapping.family,
            app_label: mapping.label.clone(),
            icon_key: builtin.icon_key,
            override_id: builtin.override_id,
            source: ContextSource::UserMapping,
            confidence: 1.0,
        },
        mapped_scene_id: mapping.scene_id.clone(),
    }
}

pub(crate) fn candidate_from_signals(
    signals: &ContextSignals,
    profile: &ContextProfile,
    generation: u64,
) -> Option<MappingCandidate> {
    let matcher = if signals.is_supported_browser {
        signals
            .browser_host
            .as_deref()
            .and_then(|host| normalize_web_host(host).ok())
            .map(UserAppMatcher::ExactWebHost)
    } else {
        let identity = signals.native_identity.as_deref()?;
        #[cfg(target_os = "macos")]
        let matcher = normalize_bundle_id(identity)
            .ok()
            .map(UserAppMatcher::NativeBundleId);
        #[cfg(not(target_os = "macos"))]
        let matcher = normalize_executable(identity)
            .ok()
            .map(UserAppMatcher::NativeExecutable);
        matcher
    }?;

    let suggested_label = match &matcher {
        UserAppMatcher::ExactWebHost(host) => host.clone(),
        _ => sanitize_mapping_label(
            signals
                .process_alias
                .as_deref()
                .unwrap_or(&profile.app_label),
        ),
    };
    if suggested_label.is_empty() {
        return None;
    }
    Some(MappingCandidate {
        generation,
        matcher,
        suggested_label,
        current_family: profile.family,
        icon_key: profile.icon_key.clone(),
    })
}

pub(crate) fn normalize_matcher(matcher: UserAppMatcher) -> Result<UserAppMatcher, String> {
    match matcher {
        UserAppMatcher::NativeBundleId(value) => {
            normalize_bundle_id(&value).map(UserAppMatcher::NativeBundleId)
        }
        UserAppMatcher::NativeExecutable(value) => {
            normalize_executable(&value).map(UserAppMatcher::NativeExecutable)
        }
        UserAppMatcher::ExactWebHost(value) => {
            normalize_web_host(&value).map(UserAppMatcher::ExactWebHost)
        }
    }
}

fn normalize_bundle_id(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 253
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
    {
        return Err("custom_app_mapping_invalid_bundle_id".to_string());
    }
    Ok(value)
}

fn normalize_executable(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 255
        || value == "."
        || value == ".."
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
    {
        return Err("custom_app_mapping_invalid_executable".to_string());
    }
    Ok(value)
}

fn normalize_web_host(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('.');
    if value.is_empty()
        || value.len() > 253
        || value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | '?' | '#' | '@' | ':' | '*' | '(' | ')' | '[' | ']'
                )
        })
    {
        return Err("custom_app_mapping_invalid_host".to_string());
    }
    match url::Host::parse(value).map_err(|_| "custom_app_mapping_invalid_host".to_string())? {
        url::Host::Domain(domain) => Ok(domain.trim_end_matches('.').to_ascii_lowercase()),
        url::Host::Ipv4(address) => Ok(address.to_string()),
        url::Host::Ipv6(_) => Err("custom_app_mapping_invalid_host".to_string()),
    }
}

pub(crate) fn sanitize_mapping_label(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_MAPPING_LABEL_CHARS)
        .collect()
}

fn sanitize_scene_id(scene_id: Option<String>) -> Option<String> {
    scene_id
        .map(|value| {
            value
                .replace('\0', "")
                .trim()
                .chars()
                .take(120)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
}

fn matcher_display_value(matcher: &UserAppMatcher, native_label: &str) -> String {
    match matcher {
        UserAppMatcher::ExactWebHost(host) => host.clone(),
        _ => format!("{} · {}", native_label, platform_display_name()),
    }
}

fn platform_display_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        _ => "Desktop",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredMappingState {
    version: u32,
    mappings: Vec<CustomAppMapping>,
}

#[derive(Clone)]
pub struct UserAppMappingStore {
    app_handle: Option<tauri::AppHandle>,
    collection: Arc<RwLock<UserAppMappingCollection>>,
    registry: AppRegistry,
}

impl UserAppMappingStore {
    pub fn new(app_handle: tauri::AppHandle, registry: AppRegistry) -> Self {
        let mappings = app_handle
            .store(MAPPING_STORE_FILE)
            .ok()
            .and_then(|store| store.get(MAPPING_STORE_KEY))
            .and_then(|value| serde_json::from_value::<StoredMappingState>(value.clone()).ok())
            .filter(|state| state.version == MAPPING_STORE_VERSION)
            .map(|state| state.mappings)
            .unwrap_or_default();
        Self {
            app_handle: Some(app_handle),
            collection: Arc::new(RwLock::new(UserAppMappingCollection::from_stored(mappings))),
            registry,
        }
    }

    #[cfg(test)]
    pub(crate) fn memory(registry: AppRegistry) -> Self {
        Self {
            app_handle: None,
            collection: Arc::new(RwLock::new(UserAppMappingCollection::default())),
            registry,
        }
    }

    pub(crate) fn resolve(&self, signals: &ContextSignals) -> ResolvedProfile {
        let collection = self
            .collection
            .read()
            .unwrap_or_else(|error| error.into_inner());
        resolve_profile(&self.registry, &collection, signals)
    }

    pub(crate) fn has_match(&self, signals: &ContextSignals) -> bool {
        self.collection
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .find_match(signals)
            .is_some()
    }

    pub fn list_views(&self) -> Vec<CustomAppMappingView> {
        let collection = self
            .collection
            .read()
            .unwrap_or_else(|error| error.into_inner());
        collection
            .mappings()
            .iter()
            .map(|mapping| self.mapping_view(mapping))
            .collect()
    }

    pub(crate) fn save_candidate(
        &self,
        candidate: &MappingCandidate,
        label: &str,
        family: ContextFamily,
        scene_id: Option<String>,
    ) -> Result<CustomAppMappingView, String> {
        self.mutate(|collection| collection.save_candidate(candidate, label, family, scene_id))
            .map(|mapping| self.mapping_view(&mapping))
    }

    pub fn update(
        &self,
        id: &str,
        label: &str,
        family: ContextFamily,
        scene_id: Option<String>,
        enabled: bool,
    ) -> Result<CustomAppMappingView, String> {
        self.mutate(|collection| collection.update(id, label, family, scene_id, enabled))
            .map(|mapping| self.mapping_view(&mapping))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        self.mutate(|collection| collection.set_enabled(id, enabled))
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        self.mutate(|collection| collection.delete(id))
    }

    pub fn reset(&self) -> Result<(), String> {
        self.mutate(|collection| {
            collection.reset();
            Ok(())
        })
    }

    fn mapping_view(&self, mapping: &CustomAppMapping) -> CustomAppMappingView {
        let signals = signals_for_matcher(&mapping.matcher);
        let base = self.registry.classify(&signals);
        CustomAppMappingView {
            id: mapping.id.clone(),
            label: mapping.label.clone(),
            matcher_type: mapping.matcher.matcher_type(),
            display_value: matcher_display_value(&mapping.matcher, &mapping.label),
            family: mapping.family,
            scene_id: mapping.scene_id.clone(),
            enabled: mapping.enabled,
            icon_key: base.icon_key,
        }
    }

    fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut UserAppMappingCollection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut collection = self
            .collection
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let previous = collection.clone();
        let result = operation(&mut collection)?;
        if let Err(error) = self.persist(&collection) {
            *collection = previous;
            return Err(error);
        }
        Ok(result)
    }

    fn persist(&self, collection: &UserAppMappingCollection) -> Result<(), String> {
        let Some(app_handle) = self.app_handle.as_ref() else {
            return Ok(());
        };
        let store = app_handle
            .store(MAPPING_STORE_FILE)
            .map_err(|_| "custom_app_mapping_store_unavailable".to_string())?;
        let value = serde_json::to_value(StoredMappingState {
            version: MAPPING_STORE_VERSION,
            mappings: collection.mappings.clone(),
        })
        .map_err(|_| "custom_app_mapping_store_invalid".to_string())?;
        store.set(MAPPING_STORE_KEY, value);
        store
            .save()
            .map_err(|_| "custom_app_mapping_store_save_failed".to_string())
    }
}

fn signals_for_matcher(matcher: &UserAppMatcher) -> ContextSignals {
    match matcher {
        UserAppMatcher::ExactWebHost(host) => ContextSignals {
            browser_host: Some(host.clone()),
            is_supported_browser: true,
            ..ContextSignals::default()
        },
        UserAppMatcher::NativeBundleId(identity) | UserAppMatcher::NativeExecutable(identity) => {
            ContextSignals {
                native_identity: Some(identity.clone()),
                process_alias: Some(identity.clone()),
                ..ContextSignals::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_detector::types::{ContextFamily, ContextSignals};

    fn web_candidate(generation: u64, host: &str) -> MappingCandidate {
        MappingCandidate {
            generation,
            matcher: UserAppMatcher::ExactWebHost(host.to_string()),
            suggested_label: host.to_string(),
            current_family: ContextFamily::General,
            icon_key: "general".to_string(),
        }
    }

    #[test]
    fn user_app_mapping_normalizes_exact_hosts_and_rejects_url_material() {
        assert_eq!(
            normalize_matcher(UserAppMatcher::ExactWebHost(" EXAMPLE.COM. ".to_string())).unwrap(),
            UserAppMatcher::ExactWebHost("example.com".to_string())
        );
        assert_eq!(
            normalize_matcher(UserAppMatcher::ExactWebHost("例子.测试".to_string())).unwrap(),
            UserAppMatcher::ExactWebHost("xn--fsqu00a.xn--0zwm56d".to_string())
        );
        for rejected in [
            "https://example.com",
            "example.com/path",
            "example.com?query=1",
            "example.com#fragment",
            "user@example.com",
            "example.com:443",
            ".*example.*",
            "window title",
        ] {
            assert!(normalize_matcher(UserAppMatcher::ExactWebHost(rejected.to_string())).is_err());
        }
    }

    #[test]
    fn user_app_mapping_normalizes_native_identity_without_paths() {
        assert_eq!(
            normalize_matcher(UserAppMatcher::NativeBundleId(
                " COM.Example.Writer ".to_string()
            ))
            .unwrap(),
            UserAppMatcher::NativeBundleId("com.example.writer".to_string())
        );
        assert_eq!(
            normalize_matcher(UserAppMatcher::NativeExecutable(" Writer.EXE ".to_string()))
                .unwrap(),
            UserAppMatcher::NativeExecutable("writer.exe".to_string())
        );
        for rejected in ["/Applications/Writer", "C:\\Apps\\writer.exe", "../writer"] {
            assert!(
                normalize_matcher(UserAppMatcher::NativeExecutable(rejected.to_string())).is_err()
            );
        }
        for rejected in ["com.example/writer", "com.example writer", "com.example.*"] {
            assert!(
                normalize_matcher(UserAppMatcher::NativeBundleId(rejected.to_string())).is_err()
            );
        }
    }

    #[test]
    fn user_app_mapping_sanitizes_labels_and_caps_unicode_scalars() {
        assert_eq!(
            sanitize_mapping_label("  My\n\t Writer\0 App  "),
            "My Writer App"
        );
        assert_eq!(sanitize_mapping_label(&"你".repeat(50)).chars().count(), 40);
    }

    #[test]
    fn user_app_mapping_collection_enforces_unique_immutable_matchers() {
        let mut collection = UserAppMappingCollection::default();
        let first = collection
            .save_candidate(
                &web_candidate(1, "docs.example.com"),
                "Docs",
                ContextFamily::Document,
                Some("builtin_clean_dictation".to_string()),
            )
            .unwrap();
        assert!(collection
            .save_candidate(
                &web_candidate(2, "DOCS.EXAMPLE.COM"),
                "Duplicate",
                ContextFamily::Email,
                None,
            )
            .is_err());

        let updated = collection
            .update(
                &first.id,
                "Docs Writer",
                ContextFamily::WorkChat,
                None,
                false,
            )
            .unwrap();
        assert_eq!(updated.matcher, first.matcher);
        assert!(!updated.enabled);
        assert!(collection
            .find_match(&browser_signals("docs.example.com"))
            .is_none());

        collection.set_enabled(&first.id, true).unwrap();
        assert!(collection
            .find_match(&browser_signals("docs.example.com"))
            .is_some());
        collection.delete(&first.id).unwrap();
        assert!(collection.mappings().is_empty());
        collection
            .save_candidate(
                &web_candidate(3, "mail.example.com"),
                "Mail",
                ContextFamily::Email,
                None,
            )
            .unwrap();
        collection.reset();
        assert!(collection.mappings().is_empty());
    }

    #[test]
    fn user_app_mapping_precedes_builtin_registry_classification() {
        let mut collection = UserAppMappingCollection::default();
        collection
            .save_candidate(
                &web_candidate(1, "mail.google.com"),
                "Personal Gmail",
                ContextFamily::PersonalChat,
                None,
            )
            .unwrap();
        let registry = crate::app_detector::registry::AppRegistry::builtin().unwrap();
        let signals = browser_signals("mail.google.com");
        let builtin = registry.classify(&signals);
        let resolved = resolve_profile(&registry, &collection, &signals);

        assert_eq!(builtin.family, ContextFamily::Email);
        assert_eq!(resolved.profile.family, ContextFamily::PersonalChat);
        assert_eq!(resolved.profile.app_label, "Personal Gmail");
        assert_eq!(
            resolved.profile.source,
            crate::app_detector::types::ContextSource::UserMapping
        );
    }

    #[test]
    fn user_app_mapping_views_never_serialize_raw_matcher_material() {
        let registry = crate::app_detector::registry::AppRegistry::builtin().unwrap();
        let store = UserAppMappingStore::memory(registry);
        let native = MappingCandidate {
            generation: 1,
            matcher: UserAppMatcher::NativeBundleId("com.example.private".to_string()),
            suggested_label: "Private Writer".to_string(),
            current_family: ContextFamily::Document,
            icon_key: "general".to_string(),
        };
        let view = store
            .save_candidate(&native, "Private Writer", ContextFamily::Document, None)
            .unwrap();
        let serialized = serde_json::to_value(view).unwrap();

        assert!(serialized.get("matcher").is_none());
        assert!(serialized.get("bundleId").is_none());
        assert!(serialized.get("nativeBundleId").is_none());
        assert!(serialized.get("executable").is_none());
        assert!(serialized.get("windowTitle").is_none());
        assert!(serialized.get("processId").is_none());
        assert!(!serialized.to_string().contains("com.example.private"));
    }

    fn browser_signals(host: &str) -> ContextSignals {
        ContextSignals {
            browser_host: Some(host.to_string()),
            is_supported_browser: true,
            ..ContextSignals::default()
        }
    }
}
