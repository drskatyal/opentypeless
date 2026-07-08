use crate::credentials::{
    CredentialSecretReader, CredentialSecretRemover, CredentialVault, SystemCredentialVault,
};
use crate::storage;
use serde::Serialize;
use tauri::{Emitter, Window};

const CREDENTIALS_CHANGED_EVENT: &str = "credentials:changed";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialStorage {
    Unavailable,
    OsVault,
    SessionOnly,
    LegacyWarning,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
    pub namespace: String,
    pub provider: String,
    pub has_secret: bool,
    pub updated_at: Option<String>,
    pub storage: CredentialStorage,
}

#[tauri::command]
pub fn get_credential_status(
    namespace: String,
    provider: String,
) -> Result<CredentialStatus, String> {
    credential_status_from_vault(namespace, provider, &SystemCredentialVault)
}

#[tauri::command]
pub async fn migrate_legacy_credentials(
    config_state: tauri::State<'_, storage::ConfigManager>,
) -> Result<(), String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    config_state.save(&config).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_credential(
    window: Window,
    namespace: String,
    provider: String,
) -> Result<Option<String>, String> {
    ensure_main_window(&window)?;
    let (namespace, provider) = validate_credential_target(namespace, provider)?;
    SystemCredentialVault
        .get_secret(&namespace, &provider)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_credential(
    window: Window,
    namespace: String,
    provider: String,
    value: String,
) -> Result<(), String> {
    ensure_main_window(&window)?;
    let (namespace, provider) = validate_credential_target(namespace, provider)?;
    let vault = SystemCredentialVault;
    if value.trim().is_empty() {
        vault
            .remove_secret(&namespace, &provider)
            .map_err(|e| e.to_string())?;
    } else {
        vault
            .set_secret(&namespace, &provider, &value)
            .map_err(|e| e.to_string())?;
    }
    let _ = window.emit(CREDENTIALS_CHANGED_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn clear_credential(window: Window, namespace: String, provider: String) -> Result<(), String> {
    ensure_main_window(&window)?;
    clear_credential_from_vault(namespace, provider, &SystemCredentialVault)?;
    let _ = window.emit(CREDENTIALS_CHANGED_EVENT, ());
    Ok(())
}

fn clear_credential_from_vault<V: CredentialSecretRemover>(
    namespace: String,
    provider: String,
    vault: &V,
) -> Result<(), String> {
    let (namespace, provider) = validate_credential_target(namespace, provider)?;
    vault
        .remove_secret(&namespace, &provider)
        .map_err(|e| e.to_string())
}

fn credential_status_from_vault<V: CredentialSecretReader>(
    namespace: String,
    provider: String,
    vault: &V,
) -> Result<CredentialStatus, String> {
    let (namespace, provider) = validate_credential_target(namespace, provider)?;
    let has_secret = match vault.get_secret(&namespace, &provider) {
        Ok(secret) => secret.is_some(),
        Err(error) => {
            tracing::warn!(
                "Credential vault unavailable for {}.{}: {}",
                namespace,
                provider,
                error
            );
            return Ok(CredentialStatus {
                namespace,
                provider,
                has_secret: false,
                updated_at: None,
                storage: CredentialStorage::Unavailable,
            });
        }
    };
    let updated_at = if has_secret {
        vault
            .get_secret_updated_at(&namespace, &provider)
            .map_err(|e| e.to_string())?
    } else {
        None
    };

    Ok(CredentialStatus {
        namespace,
        provider,
        has_secret,
        updated_at,
        storage: CredentialStorage::OsVault,
    })
}

fn ensure_main_window(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("credential access is only allowed from the main window".to_string())
    }
}

fn validate_credential_target(
    namespace: String,
    provider: String,
) -> Result<(String, String), String> {
    let namespace = namespace.trim().to_string();
    let provider = provider.trim().to_string();

    if namespace != "stt" && namespace != "llm" {
        return Err(format!("unknown credential namespace: {namespace}"));
    }
    if provider.is_empty() {
        return Err("credential provider is required".to_string());
    }
    if provider.len() > 80
        || !provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!("unsupported credential provider: {provider}"));
    }

    Ok((namespace, provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryCredentialVault {
        records: Mutex<HashMap<(String, String), String>>,
        updated_at: Mutex<HashMap<(String, String), String>>,
    }

    impl CredentialVault for MemoryCredentialVault {
        fn set_secret(&self, namespace: &str, provider: &str, secret: &str) -> Result<()> {
            self.records.lock().unwrap().insert(
                (namespace.to_string(), provider.to_string()),
                secret.to_string(),
            );
            Ok(())
        }
    }

    impl CredentialSecretReader for MemoryCredentialVault {
        fn get_secret(&self, namespace: &str, provider: &str) -> Result<Option<String>> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .get(&(namespace.to_string(), provider.to_string()))
                .cloned())
        }

        fn get_secret_updated_at(&self, namespace: &str, provider: &str) -> Result<Option<String>> {
            Ok(self
                .updated_at
                .lock()
                .unwrap()
                .get(&(namespace.to_string(), provider.to_string()))
                .cloned())
        }
    }

    impl CredentialSecretRemover for MemoryCredentialVault {
        fn remove_secret(&self, namespace: &str, provider: &str) -> Result<()> {
            self.records
                .lock()
                .unwrap()
                .remove(&(namespace.to_string(), provider.to_string()));
            Ok(())
        }
    }

    struct FailingCredentialVault;

    impl CredentialSecretReader for FailingCredentialVault {
        fn get_secret(&self, _namespace: &str, _provider: &str) -> Result<Option<String>> {
            Err(anyhow::anyhow!("vault unavailable"))
        }
    }

    #[test]
    fn validates_supported_credential_target() {
        assert_eq!(
            validate_credential_target(" stt ".to_string(), "custom-whisper".to_string()).unwrap(),
            ("stt".to_string(), "custom-whisper".to_string())
        );
    }

    #[test]
    fn rejects_unknown_namespace() {
        let err =
            validate_credential_target("other".to_string(), "openai".to_string()).unwrap_err();

        assert!(err.contains("unknown credential namespace"));
    }

    #[test]
    fn rejects_provider_with_path_separators() {
        let err =
            validate_credential_target("llm".to_string(), "openai/key".to_string()).unwrap_err();

        assert!(err.contains("unsupported credential provider"));
    }

    #[test]
    fn clear_credential_from_vault_removes_valid_target() {
        let vault = MemoryCredentialVault::default();
        vault.set_secret("llm", "openai", "secret").unwrap();

        clear_credential_from_vault("llm".to_string(), "openai".to_string(), &vault).unwrap();

        assert_eq!(vault.get_secret("llm", "openai").unwrap(), None);
    }

    #[test]
    fn credential_status_reports_spec_contract_shape() {
        let vault = MemoryCredentialVault::default();
        vault.set_secret("llm", "openai", "secret").unwrap();

        let status =
            credential_status_from_vault("llm".to_string(), "openai".to_string(), &vault).unwrap();

        assert_eq!(status.namespace, "llm");
        assert_eq!(status.provider, "openai");
        assert!(status.has_secret);
        assert_eq!(status.updated_at, None);
        assert_eq!(status.storage, CredentialStorage::OsVault);
    }

    #[test]
    fn credential_status_maps_vault_failure_to_unavailable_storage() {
        let status = credential_status_from_vault(
            "llm".to_string(),
            "openai".to_string(),
            &FailingCredentialVault,
        )
        .unwrap();

        assert_eq!(status.namespace, "llm");
        assert_eq!(status.provider, "openai");
        assert!(!status.has_secret);
        assert_eq!(status.updated_at, None);
        assert_eq!(status.storage, CredentialStorage::Unavailable);
    }

    #[test]
    fn credential_status_reports_stored_updated_at() {
        let vault = MemoryCredentialVault::default();
        vault.set_secret("llm", "openai", "secret").unwrap();
        vault.updated_at.lock().unwrap().insert(
            ("llm".to_string(), "openai".to_string()),
            "2026-07-06T00:00:00Z".to_string(),
        );

        let status =
            credential_status_from_vault("llm".to_string(), "openai".to_string(), &vault).unwrap();

        assert_eq!(status.updated_at.as_deref(), Some("2026-07-06T00:00:00Z"));
    }
}
