use crate::storage;

#[tauri::command]
pub async fn get_dictionary(
    state: tauri::State<'_, storage::DictionaryStore>,
) -> Result<Vec<storage::DictionaryEntry>, String> {
    state.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_dictionary_entry(
    state: tauri::State<'_, storage::DictionaryStore>,
    word: String,
    pronunciation: Option<String>,
) -> Result<(), String> {
    let word = word.trim().to_string();
    if word.is_empty() {
        return Err("Word cannot be empty".to_string());
    }
    if word.len() > 100 {
        return Err("Word is too long (max 100 characters)".to_string());
    }
    if let Some(ref p) = pronunciation {
        if p.len() > 100 {
            return Err("Pronunciation is too long (max 100 characters)".to_string());
        }
    }
    state
        .add(&word, pronunciation.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_dictionary_entry(
    state: tauri::State<'_, storage::DictionaryStore>,
    id: i64,
) -> Result<(), String> {
    state.remove(id).await.map_err(|e| e.to_string())
}

fn validate_correction_inputs(
    pattern: String,
    replacement: String,
) -> Result<(String, String), String> {
    let pattern = pattern.trim().to_string();
    let replacement = replacement.trim().to_string();
    if pattern.is_empty() {
        return Err("Wrong phrase cannot be empty".to_string());
    }
    if replacement.is_empty() {
        return Err("Correct phrase cannot be empty".to_string());
    }
    if pattern.chars().count() > 120 {
        return Err("Wrong phrase is too long (max 120 characters)".to_string());
    }
    if replacement.chars().count() > 120 {
        return Err("Correct phrase is too long (max 120 characters)".to_string());
    }
    Ok((pattern, replacement))
}

#[tauri::command]
pub async fn get_correction_rules(
    state: tauri::State<'_, storage::DictionaryStore>,
) -> Result<Vec<storage::CorrectionRule>, String> {
    state.correction_rules().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_correction_rule(
    state: tauri::State<'_, storage::DictionaryStore>,
    pattern: String,
    replacement: String,
) -> Result<(), String> {
    let (pattern, replacement) = validate_correction_inputs(pattern, replacement)?;
    state
        .add_correction(&pattern, &replacement)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_correction_rule(
    state: tauri::State<'_, storage::DictionaryStore>,
    id: i64,
) -> Result<(), String> {
    state.remove_correction(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_correction_rule_enabled(
    state: tauri::State<'_, storage::DictionaryStore>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    state
        .set_correction_enabled(id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_correction_inputs;

    #[test]
    fn validate_correction_inputs_trims_valid_values() {
        let (pattern, replacement) =
            validate_correction_inputs("  拓肯  ".to_string(), "  Token  ".to_string())
                .expect("valid correction");

        assert_eq!(pattern, "拓肯");
        assert_eq!(replacement, "Token");
    }

    #[test]
    fn validate_correction_inputs_rejects_empty_values() {
        assert!(validate_correction_inputs("".to_string(), "Token".to_string()).is_err());
        assert!(validate_correction_inputs("拓肯".to_string(), "  ".to_string()).is_err());
    }

    #[test]
    fn validate_correction_inputs_rejects_overlong_values() {
        let too_long = "a".repeat(121);

        assert!(validate_correction_inputs(too_long.clone(), "Token".to_string()).is_err());
        assert!(validate_correction_inputs("拓肯".to_string(), too_long).is_err());
    }
}
