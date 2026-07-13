pub mod cache;
pub mod platform;
pub mod profiles;
pub mod registry;
pub mod types;
pub mod user_mappings;

pub use cache::ContextDetectorHandle;

#[cfg(test)]
mod context_types_tests {
    use super::types::{ContextFamily, ContextProfile, ContextSource};

    #[test]
    fn context_types_serialize_without_raw_signals() {
        assert_eq!(
            serde_json::to_value(ContextFamily::DeveloperCollaboration).unwrap(),
            "developer_collaboration"
        );

        let profile = ContextProfile {
            id: "dev.github".to_string(),
            family: ContextFamily::DeveloperCollaboration,
            app_label: "GitHub".to_string(),
            icon_key: "github".to_string(),
            override_id: Some("github".to_string()),
            source: ContextSource::BrowserDomain,
            confidence: 1.0,
        };
        let serialized = serde_json::to_string(&profile).unwrap();
        for forbidden in [
            "window_title",
            "browser_host",
            "process_id",
            "native_identity",
            "url",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }
}

#[cfg(test)]
mod app_registry_tests {
    use super::registry::AppRegistry;
    use super::types::{ContextFamily, ContextSignals};

    fn browser(host: &str) -> ContextSignals {
        ContextSignals {
            browser_host: Some(host.to_string()),
            is_supported_browser: true,
            ..ContextSignals::default()
        }
    }

    fn native(identity: &str) -> ContextSignals {
        ContextSignals {
            native_identity: Some(identity.to_string()),
            ..ContextSignals::default()
        }
    }

    #[test]
    fn app_registry_uses_exact_and_boundary_safe_matching() {
        let registry = AppRegistry::builtin().unwrap();
        assert_eq!(
            registry.classify(&browser("mail.google.com")).id,
            "email.gmail"
        );
        assert_eq!(
            registry.classify(&browser("acme.slack.com")).id,
            "chat.slack"
        );
        assert_eq!(
            registry.classify(&browser("evillinear.app")).id,
            "general.browser"
        );
        assert_eq!(
            registry.classify(&native("com.tinyspeck.slackmacgap")).id,
            "chat.slack"
        );
    }

    #[test]
    fn app_registry_rejects_browser_hosts_without_a_supported_adapter() {
        let registry = AppRegistry::builtin().unwrap();
        let signals = ContextSignals {
            browser_host: Some("mail.google.com".to_string()),
            is_supported_browser: false,
            ..ContextSignals::default()
        };
        assert_eq!(registry.classify(&signals).id, "general.native");
    }

    #[test]
    fn app_registry_covers_every_required_family() {
        let registry = AppRegistry::builtin().unwrap();
        for family in [
            ContextFamily::Email,
            ContextFamily::WorkChat,
            ContextFamily::PersonalChat,
            ContextFamily::Document,
            ContextFamily::ProjectManagement,
            ContextFamily::DeveloperCollaboration,
            ContextFamily::PromptOrCode,
            ContextFamily::Support,
            ContextFamily::Social,
        ] {
            assert!(registry
                .profiles()
                .iter()
                .any(|profile| profile.family == family));
        }
        assert!(registry.profiles().len() >= 70);
    }
}
