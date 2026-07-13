use std::collections::{HashMap, HashSet};

use super::profiles::{builtin_profile_definitions, style_override, ProfileDefinition};
use super::types::{
    is_valid_profile_id, ContextFamily, ContextProfile, ContextSignals, ContextSource,
};

#[derive(Clone, Debug)]
struct TitleMatcher {
    profile_index: usize,
    suffix: String,
    required_host_suffix: Option<String>,
    required_native_identity: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AppRegistry {
    definitions: Vec<ProfileDefinition>,
    profiles: Vec<ContextProfile>,
    profile_ids: HashMap<String, usize>,
    exact_hosts: HashMap<String, usize>,
    host_suffixes: Vec<(String, usize)>,
    native_identities: HashMap<String, usize>,
    process_aliases: HashMap<String, usize>,
    title_markers: Vec<TitleMatcher>,
}

impl AppRegistry {
    pub fn builtin() -> Result<Self, String> {
        Self::build(builtin_profile_definitions())
    }

    pub fn build(definitions: Vec<ProfileDefinition>) -> Result<Self, String> {
        validate_definitions(&definitions)?;

        let profiles = definitions
            .iter()
            .map(|definition| ContextProfile {
                id: definition.id.clone(),
                family: definition.family,
                app_label: definition.app_label.clone(),
                icon_key: definition.icon_key.clone(),
                override_id: definition.override_id.clone(),
                source: ContextSource::Fallback,
                confidence: 0.0,
            })
            .collect::<Vec<_>>();

        let mut registry = Self {
            definitions,
            profiles,
            profile_ids: HashMap::new(),
            exact_hosts: HashMap::new(),
            host_suffixes: Vec::new(),
            native_identities: HashMap::new(),
            process_aliases: HashMap::new(),
            title_markers: Vec::new(),
        };

        for (index, definition) in registry.definitions.iter().enumerate() {
            registry.profile_ids.insert(definition.id.clone(), index);
            for host in &definition.exact_hosts {
                registry.exact_hosts.insert(normalize_host(host), index);
            }
            for suffix in &definition.host_suffixes {
                registry.host_suffixes.push((normalize_host(suffix), index));
            }
            for identity in &definition.native_identities {
                registry
                    .native_identities
                    .insert(normalize_identity(identity), index);
            }
            for alias in &definition.process_aliases {
                registry
                    .process_aliases
                    .insert(normalize_identity(alias), index);
            }
            for marker in &definition.title_markers {
                registry.title_markers.push(TitleMatcher {
                    profile_index: index,
                    suffix: marker.suffix.clone(),
                    required_host_suffix: marker
                        .required_host_suffix
                        .as_deref()
                        .map(normalize_host),
                    required_native_identity: marker
                        .required_native_identity
                        .as_deref()
                        .map(normalize_identity),
                });
            }
        }
        registry
            .host_suffixes
            .sort_by(|(left, _), (right, _)| right.len().cmp(&left.len()).then(left.cmp(right)));

        Ok(registry)
    }

    pub fn profiles(&self) -> &[ContextProfile] {
        &self.profiles
    }

    pub fn definitions(&self) -> &[ProfileDefinition] {
        &self.definitions
    }

    pub fn profile(&self, id: &str) -> Option<ContextProfile> {
        self.profile_ids
            .get(id)
            .map(|index| self.profiles[*index].clone())
    }

    pub(crate) fn classify(&self, signals: &ContextSignals) -> ContextProfile {
        self.classify_with_mapping(signals, None)
    }

    pub(crate) fn classify_with_mapping(
        &self,
        signals: &ContextSignals,
        mapped_profile_id: Option<&str>,
    ) -> ContextProfile {
        if let Some(index) = mapped_profile_id
            .and_then(|id| self.profile_ids.get(id))
            .copied()
        {
            return self.matched_profile(index, ContextSource::UserMapping, 1.0);
        }

        let normalized_host = signals
            .browser_host
            .as_deref()
            .filter(|_| signals.is_supported_browser)
            .map(normalize_host)
            .filter(|host| is_safe_host(host));

        if let Some(host) = normalized_host.as_deref() {
            if let Some(index) = self.exact_hosts.get(host).copied() {
                return self.matched_profile(index, ContextSource::BrowserDomain, 1.0);
            }
            if let Some((_, index)) = self
                .host_suffixes
                .iter()
                .find(|(suffix, _)| host_matches_suffix(host, suffix))
            {
                return self.matched_profile(*index, ContextSource::BrowserDomain, 0.98);
            }
        }

        if let Some(identity) = signals.native_identity.as_deref().map(normalize_identity) {
            if let Some(index) = self.native_identities.get(&identity).copied() {
                return self.matched_profile(index, ContextSource::NativeProcess, 1.0);
            }
        }

        if let Some(alias) = signals.process_alias.as_deref().map(normalize_identity) {
            if let Some(index) = self.process_aliases.get(&alias).copied() {
                return self.matched_profile(index, ContextSource::NativeProcess, 0.96);
            }
        }

        if let Some(title) = signals.window_title.as_deref() {
            for marker in &self.title_markers {
                if !title.ends_with(&marker.suffix) {
                    continue;
                }
                if let Some(required_host) = marker.required_host_suffix.as_deref() {
                    let Some(host) = normalized_host.as_deref() else {
                        continue;
                    };
                    if !host_matches_suffix(host, required_host) {
                        continue;
                    }
                }
                if let Some(required_identity) = marker.required_native_identity.as_deref() {
                    let identity = signals
                        .native_identity
                        .as_deref()
                        .map(normalize_identity)
                        .unwrap_or_default();
                    if identity != required_identity {
                        continue;
                    }
                }
                return self.matched_profile(
                    marker.profile_index,
                    ContextSource::WindowTitle,
                    0.94,
                );
            }
        }

        if signals.is_supported_browser {
            ContextProfile::general_browser()
        } else {
            ContextProfile::general_native()
        }
    }

    fn matched_profile(
        &self,
        index: usize,
        source: ContextSource,
        confidence: f32,
    ) -> ContextProfile {
        let mut profile = self.profiles[index].clone();
        profile.source = source;
        profile.confidence = confidence;
        profile
    }
}

fn normalize_host(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_safe_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
}

pub(crate) fn host_matches_suffix(host: &str, suffix: &str) -> bool {
    host == suffix
        || host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn validate_definitions(definitions: &[ProfileDefinition]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut exact_hosts = HashMap::<String, &str>::new();
    let mut host_suffixes = HashMap::<String, &str>::new();
    let mut native_identities = HashMap::<String, &str>::new();
    let mut process_aliases = HashMap::<String, &str>::new();

    for definition in definitions {
        if !is_valid_profile_id(&definition.id) {
            return Err(format!("invalid profile id: {}", definition.id));
        }
        if !profile_prefix_matches_family(&definition.id, definition.family) {
            return Err(format!(
                "profile id {} does not match family {:?}",
                definition.id, definition.family
            ));
        }
        if !ids.insert(definition.id.as_str()) {
            return Err(format!("duplicate profile id: {}", definition.id));
        }
        if definition.app_label.trim().is_empty() || definition.icon_key.trim().is_empty() {
            return Err(format!(
                "profile {} is missing label or icon",
                definition.id
            ));
        }
        if definition.exact_hosts.is_empty()
            && definition.host_suffixes.is_empty()
            && definition.native_identities.is_empty()
            && definition.process_aliases.is_empty()
            && definition.title_markers.is_empty()
        {
            return Err(format!("profile {} has no matcher", definition.id));
        }
        if let Some(override_id) = definition.override_id.as_deref() {
            if style_override(override_id).is_none() {
                return Err(format!(
                    "profile {} uses unknown override {}",
                    definition.id, override_id
                ));
            }
        }

        validate_unique_values(
            &definition.id,
            &definition.exact_hosts,
            &mut exact_hosts,
            "exact host",
            normalize_host,
        )?;
        validate_unique_values(
            &definition.id,
            &definition.host_suffixes,
            &mut host_suffixes,
            "host suffix",
            normalize_host,
        )?;
        validate_unique_values(
            &definition.id,
            &definition.native_identities,
            &mut native_identities,
            "native identity",
            normalize_identity,
        )?;
        validate_unique_values(
            &definition.id,
            &definition.process_aliases,
            &mut process_aliases,
            "process alias",
            normalize_identity,
        )?;

        for host in definition
            .exact_hosts
            .iter()
            .chain(definition.host_suffixes.iter())
        {
            if !is_safe_host(&normalize_host(host)) {
                return Err(format!(
                    "profile {} has unsafe host {}",
                    definition.id, host
                ));
            }
        }
        for marker in &definition.title_markers {
            if !marker.suffix.starts_with(" - ")
                || (marker.required_host_suffix.is_none()
                    && marker.required_native_identity.is_none())
            {
                return Err(format!(
                    "profile {} has an unanchored title matcher",
                    definition.id
                ));
            }
        }
    }

    Ok(())
}

fn validate_unique_values<'a>(
    profile_id: &'a str,
    values: &[String],
    seen: &mut HashMap<String, &'a str>,
    matcher_name: &str,
    normalize: fn(&str) -> String,
) -> Result<(), String> {
    for value in values {
        let normalized = normalize(value);
        if let Some(previous) = seen.insert(normalized.clone(), profile_id) {
            return Err(format!(
                "duplicate {matcher_name} {normalized}: {previous} and {profile_id}"
            ));
        }
    }
    Ok(())
}

fn profile_prefix_matches_family(id: &str, family: ContextFamily) -> bool {
    let prefix = id.split('.').next().unwrap_or_default();
    match family {
        ContextFamily::Email => prefix == "email",
        ContextFamily::WorkChat => prefix == "chat",
        ContextFamily::PersonalChat => prefix == "message",
        ContextFamily::Document => prefix == "doc",
        ContextFamily::ProjectManagement => prefix == "project",
        ContextFamily::DeveloperCollaboration => prefix == "dev",
        ContextFamily::PromptOrCode => matches!(prefix, "code" | "prompt"),
        ContextFamily::Support => prefix == "support",
        ContextFamily::Social => prefix == "social",
        ContextFamily::General => prefix == "general",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::app_detector::profiles::ReleaseTier;

    fn browser(host: &str) -> ContextSignals {
        ContextSignals {
            browser_host: Some(host.to_string()),
            is_supported_browser: true,
            ..ContextSignals::default()
        }
    }

    #[test]
    fn app_registry_every_builtin_matcher_has_positive_and_negative_coverage() {
        let registry = AppRegistry::builtin().unwrap();
        for definition in registry.definitions() {
            for host in &definition.exact_hosts {
                assert_eq!(registry.classify(&browser(host)).id, definition.id);
                assert_ne!(
                    registry
                        .classify(&browser(&format!(
                            "evil-{}.example",
                            host.replace('.', "-")
                        )))
                        .id,
                    definition.id
                );
                assert_ne!(
                    registry
                        .classify(&browser(&format!("{host}.evil.example")))
                        .id,
                    definition.id
                );
            }
            for suffix in &definition.host_suffixes {
                assert_eq!(
                    registry.classify(&browser(&format!("tenant.{suffix}"))).id,
                    definition.id
                );
                assert_ne!(
                    registry.classify(&browser(&format!("evil{suffix}"))).id,
                    definition.id
                );
                assert_ne!(
                    registry
                        .classify(&browser(&format!("{suffix}.evil.example")))
                        .id,
                    definition.id
                );
            }
            for identity in &definition.native_identities {
                let signals = ContextSignals {
                    native_identity: Some(identity.clone()),
                    ..ContextSignals::default()
                };
                assert_eq!(registry.classify(&signals).id, definition.id);
                let near_match = ContextSignals {
                    native_identity: Some(format!("{identity}.evil")),
                    ..ContextSignals::default()
                };
                assert_ne!(registry.classify(&near_match).id, definition.id);
            }
            for alias in &definition.process_aliases {
                let signals = ContextSignals {
                    process_alias: Some(alias.clone()),
                    ..ContextSignals::default()
                };
                assert_eq!(registry.classify(&signals).id, definition.id);
                let near_match = ContextSignals {
                    process_alias: Some(format!("evil-{alias}")),
                    ..ContextSignals::default()
                };
                assert_ne!(registry.classify(&near_match).id, definition.id);
            }
        }
    }

    #[test]
    fn app_registry_title_markers_require_anchor_and_host_boundary() {
        let registry = AppRegistry::builtin().unwrap();
        let valid = ContextSignals {
            browser_host: Some("acme.atlassian.net".to_string()),
            is_supported_browser: true,
            window_title: Some("ENG-42 - Jira".to_string()),
            ..ContextSignals::default()
        };
        assert_eq!(registry.classify(&valid).id, "project.jira");

        let unanchored = ContextSignals {
            window_title: Some("Jira notes in another app".to_string()),
            ..valid.clone()
        };
        assert_eq!(registry.classify(&unanchored).id, "general.browser");

        let wrong_host = ContextSignals {
            browser_host: Some("evilatlassian.net".to_string()),
            ..valid
        };
        assert_eq!(registry.classify(&wrong_host).id, "general.browser");
    }

    #[test]
    fn app_registry_user_mapping_has_highest_precedence() {
        let registry = AppRegistry::builtin().unwrap();
        let profile =
            registry.classify_with_mapping(&browser("mail.google.com"), Some("project.linear"));
        assert_eq!(profile.id, "project.linear");
        assert_eq!(profile.source, ContextSource::UserMapping);
    }

    #[test]
    fn app_registry_rejects_duplicate_and_unstructured_definitions() {
        let mut definitions = builtin_profile_definitions();
        definitions.push(definitions[0].clone());
        assert!(AppRegistry::build(definitions).is_err());

        let invalid = ProfileDefinition {
            id: "email.invalid".to_string(),
            family: ContextFamily::Email,
            app_label: "Invalid".to_string(),
            icon_key: "email".to_string(),
            override_id: Some("free-form-prompt".to_string()),
            exact_hosts: vec!["invalid.example".to_string()],
            host_suffixes: Vec::new(),
            native_identities: Vec::new(),
            process_aliases: Vec::new(),
            title_markers: Vec::new(),
            release_tier: ReleaseTier::Extended,
        };
        assert!(AppRegistry::build(vec![invalid]).is_err());
    }

    #[test]
    fn registry_500_profiles_classifies_below_two_milliseconds_p95() {
        let definitions = (0..500)
            .map(|index| ProfileDefinition {
                id: format!("doc.generated-{index}"),
                family: ContextFamily::Document,
                app_label: format!("Generated {index}"),
                icon_key: "document".to_string(),
                override_id: None,
                exact_hosts: vec![format!("app-{index}.example.test")],
                host_suffixes: Vec::new(),
                native_identities: Vec::new(),
                process_aliases: Vec::new(),
                title_markers: Vec::new(),
                release_tier: ReleaseTier::Extended,
            })
            .collect();
        let registry = AppRegistry::build(definitions).unwrap();

        for index in 0..1_000 {
            let _ = registry.classify(&browser(&format!("app-{}.example.test", index % 500)));
        }

        let mut samples = Vec::with_capacity(10_000);
        for index in 0..10_000 {
            let started = Instant::now();
            let profile = registry.classify(&browser(&format!("app-{}.example.test", index % 500)));
            samples.push(started.elapsed());
            assert!(profile.id.starts_with("doc.generated-"));
        }
        samples.sort_unstable();
        let p95 = samples[9_499];
        println!("registry_500_profiles p95={p95:?}");
        assert!(p95 < std::time::Duration::from_millis(2));
    }
}
