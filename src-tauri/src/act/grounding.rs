//! Grounding — resolve a spoken target to a concrete element using the snapshot,
//! without vision.
//!
//! The ordered resolution stack (cheap/deterministic first), per
//! `docs/flowrad-act-architecture.md` §5:
//!
//! 1. **Deictic / focus-relative** — "this", "here", "that", "it" resolve to the
//!    focused element, else the pointer element.
//! 2. **Ordinal / structural** — "first/second/third/last <role>" over the visible
//!    interactive candidates in snapshot (reading/focus) order.
//! 3. **State filters** — "the selected …", "the checked …".
//! 4. **Role + name match** — fuzzy contains match on name/description, optionally
//!    filtered by a spoken role word.
//!
//! Only visible + actionable elements ([`Snapshot::interactive`]) are considered.
//! A unique hit yields [`Grounded::One`]; a tie yields [`Grounded::Ambiguous`]
//! (for numbered-overlay disambiguation); nothing yields [`Grounded::None`].

use super::element::{ElementPath, ElementState, Role, Snapshot, UiElement};

/// The outcome of trying to resolve a spoken reference to an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Grounded {
    /// A single confident match.
    One(ElementPath),
    /// Several plausible matches — disambiguate with a numbered overlay.
    Ambiguous(Vec<ElementPath>),
    /// No match.
    None,
}

/// Resolve a spoken phrase against the snapshot.
pub fn resolve(snapshot: &Snapshot, phrase: &str) -> Grounded {
    let norm = normalize(phrase);
    let tokens: Vec<&str> = norm.split_whitespace().collect();
    if tokens.is_empty() {
        return Grounded::None;
    }

    // 1. Deictic / focus-relative — highest priority.
    if tokens.iter().any(|t| is_deictic(t)) {
        if let Some(path) = snapshot
            .focused
            .clone()
            .or_else(|| snapshot.pointer.clone())
        {
            return Grounded::One(path);
        }
    }

    let candidates: Vec<&UiElement> = snapshot.interactive().collect();
    let role_filter = role_from_tokens(&tokens);

    // 2. Ordinal / structural — "second button", "last tab".
    if let Some(ord) = ordinal_from_tokens(&tokens) {
        let filtered: Vec<&UiElement> = candidates
            .iter()
            .copied()
            .filter(|e| role_allows(e, role_filter))
            .collect();
        let idx = match ord {
            Ordinal::Last => filtered.len().checked_sub(1),
            Ordinal::Nth(n) => Some(n),
        };
        return match idx.and_then(|i| filtered.get(i)) {
            Some(e) => Grounded::One(e.path.clone()),
            None => Grounded::None,
        };
    }

    // 3. State filters — "the selected row", "the checked box".
    if let Some(state) = state_from_tokens(&tokens) {
        let filtered: Vec<&UiElement> = candidates
            .iter()
            .copied()
            .filter(|e| e.has_state(state) && role_allows(e, role_filter))
            .collect();
        return unique_or_ambiguous(&filtered);
    }

    // 4. Role + name fuzzy match.
    let name_query = name_query(&tokens);
    if !name_query.is_empty() {
        let matches: Vec<&UiElement> = candidates
            .iter()
            .copied()
            .filter(|e| role_allows(e, role_filter) && name_matches(e, &name_query))
            .collect();
        return unique_or_ambiguous(&matches);
    }

    // Bare role word ("button") — every element of that role is a candidate.
    if role_filter.is_some() {
        let filtered: Vec<&UiElement> = candidates
            .iter()
            .copied()
            .filter(|e| role_allows(e, role_filter))
            .collect();
        return unique_or_ambiguous(&filtered);
    }

    Grounded::None
}

/// Lowercase, collapse whitespace, strip trailing sentence punctuation.
fn normalize(phrase: &str) -> String {
    phrase
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches([' ', '.', ',', '!', '?', ';', ':'])
        .to_string()
}

fn is_deictic(token: &str) -> bool {
    matches!(token, "this" | "here" | "that" | "it")
}

/// A word that carries no target identity and is dropped from the name query.
fn is_filler(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "a"
            | "an"
            | "click"
            | "press"
            | "tap"
            | "select"
            | "choose"
            | "hit"
            | "go"
            | "to"
            | "on"
            | "please"
    ) || is_deictic(token)
        || is_role_word(token)
        || ordinal_word(token).is_some()
        || state_word(token).is_some()
}

fn is_role_word(token: &str) -> bool {
    role_word(token).is_some()
}

fn role_word(token: &str) -> Option<Role> {
    Some(match token {
        "button" => Role::Button,
        "field" | "textbox" | "box" | "input" => Role::TextField,
        "menu" => Role::Menu,
        "tab" => Role::Tab,
        "link" => Role::Link,
        "checkbox" => Role::CheckBox,
        "row" => Role::Row,
        "cell" => Role::Cell,
        "item" => Role::ListItem,
        _ => return None,
    })
}

/// Extract a role filter from the phrase, honoring two-word role names.
fn role_from_tokens(tokens: &[&str]) -> Option<Role> {
    let joined = tokens.join(" ");
    if joined.contains("text field") {
        return Some(Role::TextField);
    }
    if joined.contains("check box") {
        return Some(Role::CheckBox);
    }
    tokens.iter().find_map(|t| role_word(t))
}

fn role_allows(element: &UiElement, filter: Option<Role>) -> bool {
    match filter {
        Some(role) => element.role == role,
        None => true,
    }
}

enum Ordinal {
    Nth(usize),
    Last,
}

fn ordinal_word(token: &str) -> Option<Ordinal> {
    Some(match token {
        "first" | "1st" => Ordinal::Nth(0),
        "second" | "2nd" => Ordinal::Nth(1),
        "third" | "3rd" => Ordinal::Nth(2),
        "fourth" | "4th" => Ordinal::Nth(3),
        "fifth" | "5th" => Ordinal::Nth(4),
        "last" => Ordinal::Last,
        _ => return None,
    })
}

fn ordinal_from_tokens(tokens: &[&str]) -> Option<Ordinal> {
    tokens.iter().find_map(|t| ordinal_word(t))
}

fn state_word(token: &str) -> Option<ElementState> {
    Some(match token {
        "selected" => ElementState::Selected,
        "checked" => ElementState::Checked,
        "focused" => ElementState::Focused,
        "expanded" => ElementState::Expanded,
        "enabled" => ElementState::Enabled,
        "disabled" => ElementState::Disabled,
        _ => return None,
    })
}

fn state_from_tokens(tokens: &[&str]) -> Option<ElementState> {
    tokens.iter().find_map(|t| state_word(t))
}

/// The remaining meaningful tokens joined into a name query.
fn name_query(tokens: &[&str]) -> String {
    tokens
        .iter()
        .filter(|t| !is_filler(t))
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fuzzy contains match on the element's name or description (normalized).
fn name_matches(element: &UiElement, query: &str) -> bool {
    let name = element.name.to_lowercase();
    let desc = element.description.to_lowercase();
    name.contains(query) || desc.contains(query)
}

fn unique_or_ambiguous(matches: &[&UiElement]) -> Grounded {
    match matches {
        [] => Grounded::None,
        [only] => Grounded::One(only.path.clone()),
        many => Grounded::Ambiguous(many.iter().map(|e| e.path.clone()).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::act::element::{ActionPattern, Bounds};

    fn el(path: &str, role: Role, name: &str) -> UiElement {
        UiElement {
            path: path.to_string(),
            role,
            name: name.to_string(),
            description: String::new(),
            value_len: 0,
            states: Vec::new(),
            bounds: Some(Bounds {
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            }),
            patterns: vec![ActionPattern::Invoke],
        }
    }

    fn snapshot(elements: Vec<UiElement>, focused: Option<ElementPath>) -> Snapshot {
        Snapshot {
            app: "Test".into(),
            window_title: "W".into(),
            focused,
            pointer: None,
            selection_text_len: 0,
            elements,
        }
    }

    #[test]
    fn deictic_resolves_to_focus() {
        let snap = snapshot(
            vec![
                el("#/1", Role::TextField, "Message"),
                el("#/2", Role::Button, "Send"),
            ],
            Some("#/1".into()),
        );
        assert_eq!(resolve(&snap, "this"), Grounded::One("#/1".into()));
        assert_eq!(resolve(&snap, "that field"), Grounded::One("#/1".into()));
    }

    #[test]
    fn deictic_falls_back_to_pointer() {
        let mut snap = snapshot(vec![el("#/1", Role::Button, "OK")], None);
        snap.pointer = Some("#/1".into());
        assert_eq!(resolve(&snap, "click here"), Grounded::One("#/1".into()));
    }

    #[test]
    fn unique_name_resolves() {
        let snap = snapshot(
            vec![
                el("#/1", Role::Button, "Submit"),
                el("#/2", Role::Button, "Cancel"),
            ],
            None,
        );
        assert_eq!(resolve(&snap, "Submit button"), Grounded::One("#/1".into()));
        // Name match without the role word still works.
        assert_eq!(resolve(&snap, "cancel"), Grounded::One("#/2".into()));
    }

    #[test]
    fn duplicate_names_are_ambiguous() {
        let snap = snapshot(
            vec![
                el("#/1", Role::Button, "Delete"),
                el("#/2", Role::Button, "Delete"),
                el("#/3", Role::Button, "Keep"),
            ],
            None,
        );
        assert_eq!(
            resolve(&snap, "delete button"),
            Grounded::Ambiguous(vec!["#/1".into(), "#/2".into()])
        );
    }

    #[test]
    fn ordinal_picks_nth_of_role() {
        let snap = snapshot(
            vec![
                el("#/1", Role::TextField, "Name"),
                el("#/2", Role::Button, "One"),
                el("#/3", Role::Button, "Two"),
                el("#/4", Role::Button, "Three"),
            ],
            None,
        );
        assert_eq!(resolve(&snap, "second button"), Grounded::One("#/3".into()));
        assert_eq!(resolve(&snap, "first button"), Grounded::One("#/2".into()));
        assert_eq!(resolve(&snap, "last button"), Grounded::One("#/4".into()));
    }

    #[test]
    fn ordinal_out_of_range_is_none() {
        let snap = snapshot(vec![el("#/1", Role::Button, "Only")], None);
        assert_eq!(resolve(&snap, "third button"), Grounded::None);
    }

    #[test]
    fn state_filter_selects_by_state() {
        let mut selected = el("#/2", Role::Row, "Row B");
        selected.states = vec![ElementState::Selected];
        selected.patterns = vec![ActionPattern::Select];
        let mut plain = el("#/1", Role::Row, "Row A");
        plain.patterns = vec![ActionPattern::Select];
        let snap = snapshot(vec![plain, selected], None);
        assert_eq!(
            resolve(&snap, "the selected row"),
            Grounded::One("#/2".into())
        );
    }

    #[test]
    fn no_match_is_none() {
        let snap = snapshot(vec![el("#/1", Role::Button, "Submit")], None);
        assert_eq!(resolve(&snap, "the nonexistent widget"), Grounded::None);
        assert_eq!(resolve(&snap, ""), Grounded::None);
    }

    #[test]
    fn offscreen_elements_are_ignored() {
        let mut hidden = el("#/1", Role::Button, "Submit");
        hidden.states = vec![ElementState::Offscreen];
        let snap = snapshot(vec![hidden, el("#/2", Role::Button, "Submit")], None);
        assert_eq!(resolve(&snap, "submit"), Grounded::One("#/2".into()));
    }
}
