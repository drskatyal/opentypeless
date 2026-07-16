//! The compact, capped, PHI-safe view of a [`Snapshot`] handed to the planner.
//!
//! We never send the model raw element values or full document text — only
//! control roles, names/labels, states, value *lengths*, and stable paths, all
//! capped so the prompt stays small (the planner is on the latency-critical
//! path). The rendered block is wrapped in explicit UNTRUSTED markers so the
//! planner treats it strictly as data.

use super::element::{ElementState, Snapshot, UiElement};

/// Default caps for a packet (see [`GroundingPacket::from_snapshot`]).
pub const DEFAULT_MAX_ELEMENTS: usize = 40;
pub const DEFAULT_MAX_NAME_CHARS: usize = 60;

/// One element as the planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundingElement {
    pub path: String,
    pub role: String,
    pub name: String,
    pub value_len: usize,
    pub states: Vec<String>,
}

/// The full grounding packet for one planning turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundingPacket {
    pub app_name: String,
    pub window_title: String,
    pub focused_path: Option<String>,
    pub elements: Vec<GroundingElement>,
}

fn role_str(role: &super::element::Role) -> &'static str {
    use super::element::Role::*;
    match role {
        Button => "button",
        TextField => "textfield",
        CheckBox => "checkbox",
        RadioButton => "radio",
        ComboBox => "combobox",
        List => "list",
        ListItem => "listitem",
        Menu => "menu",
        MenuBar => "menubar",
        MenuItem => "menuitem",
        Tab => "tab",
        TabItem => "tabitem",
        Link => "link",
        Window => "window",
        Pane => "pane",
        Group => "group",
        Text => "text",
        Image => "image",
        Slider => "slider",
        Spinner => "spinner",
        ProgressBar => "progressbar",
        ScrollBar => "scrollbar",
        Toolbar => "toolbar",
        TitleBar => "titlebar",
        Separator => "separator",
        Tree => "tree",
        TreeItem => "treeitem",
        Table => "table",
        Row => "row",
        Cell => "cell",
        Document => "document",
        Unknown => "unknown",
    }
}

fn state_str(state: ElementState) -> &'static str {
    match state {
        ElementState::Focused => "focused",
        ElementState::Selected => "selected",
        ElementState::Enabled => "enabled",
        ElementState::Disabled => "disabled",
        ElementState::Checked => "checked",
        ElementState::Expanded => "expanded",
        ElementState::Offscreen => "offscreen",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

fn to_grounding_element(el: &UiElement, max_name: usize) -> GroundingElement {
    GroundingElement {
        path: el.path.clone(),
        role: role_str(&el.role).to_string(),
        name: truncate(&el.name, max_name),
        value_len: el.value_len,
        states: el
            .states
            .iter()
            .map(|s| state_str(*s).to_string())
            .collect(),
    }
}

impl GroundingPacket {
    /// Build a capped packet from a fresh snapshot. Visible+interactive elements
    /// come first (they are the actionable targets), then any remaining, up to
    /// `max_elements`.
    pub fn from_snapshot(snap: &Snapshot, max_elements: usize, max_name_chars: usize) -> Self {
        let mut ordered: Vec<&UiElement> = Vec::with_capacity(snap.elements.len());
        ordered.extend(snap.elements.iter().filter(|e| e.is_interactive()));
        ordered.extend(snap.elements.iter().filter(|e| !e.is_interactive()));

        let elements = ordered
            .into_iter()
            .take(max_elements)
            .map(|e| to_grounding_element(e, max_name_chars))
            .collect();

        Self {
            app_name: truncate(&snap.app, 80),
            window_title: truncate(&snap.window_title, 80),
            focused_path: snap.focused.clone(),
            elements,
        }
    }

    /// Render the packet as a compact, UNTRUSTED-wrapped prompt block. Contains no
    /// element values — only labels, roles, states and value lengths.
    pub fn to_prompt_block(&self) -> String {
        let mut out = String::new();
        out.push_str("<<<UNTRUSTED_UI_SNAPSHOT\n");
        out.push_str(&format!("app: {}\n", self.app_name));
        out.push_str(&format!("window: {}\n", self.window_title));
        if let Some(f) = &self.focused_path {
            out.push_str(&format!("focused: {f}\n"));
        }
        out.push_str("elements:\n");
        for e in &self.elements {
            let states = if e.states.is_empty() {
                String::new()
            } else {
                format!(" states={}", e.states.join(","))
            };
            let vlen = if e.value_len > 0 {
                format!(" value_len={}", e.value_len)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "  path={} role={} name=\"{}\"{}{}\n",
                e.path,
                e.role,
                e.name.replace('"', "'"),
                states,
                vlen
            ));
        }
        out.push_str("<<<END_UNTRUSTED_UI_SNAPSHOT");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::element::{ActionPattern, Bounds, Role, UiElement};
    use super::*;

    fn el(path: &str, role: Role, name: &str, interactive: bool) -> UiElement {
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
                w: 5,
                h: 5,
            }),
            patterns: if interactive {
                vec![ActionPattern::Invoke]
            } else {
                Vec::new()
            },
        }
    }

    fn snap() -> Snapshot {
        Snapshot {
            app: "Editor".into(),
            window_title: "Untitled".into(),
            focused: Some("#/2".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![
                el("#/1", Role::Text, "just a label", false),
                el("#/2", Role::TextField, "Message", true),
                el("#/3", Role::Button, "Send", true),
            ],
        }
    }

    #[test]
    fn interactive_elements_come_first() {
        let p = GroundingPacket::from_snapshot(&snap(), 40, 60);
        assert_eq!(p.elements[0].name, "Message");
        assert_eq!(p.elements[1].name, "Send");
        assert_eq!(p.elements[2].name, "just a label");
    }

    #[test]
    fn caps_element_count_and_name_length() {
        let mut s = snap();
        s.elements[1].name = "x".repeat(200);
        let p = GroundingPacket::from_snapshot(&s, 2, 10);
        assert_eq!(p.elements.len(), 2);
        assert!(p.elements[0].name.chars().count() <= 11); // 10 + ellipsis
    }

    #[test]
    fn prompt_block_is_untrusted_wrapped_and_value_free() {
        let mut s = snap();
        s.elements[1].value_len = 8; // a value is present but only its length shows
        let block = GroundingPacket::from_snapshot(&s, 40, 60).to_prompt_block();
        assert!(block.starts_with("<<<UNTRUSTED_UI_SNAPSHOT"));
        assert!(block.trim_end().ends_with("<<<END_UNTRUSTED_UI_SNAPSHOT"));
        assert!(block.contains("value_len=8"));
        assert!(block.contains("name=\"Send\""));
    }
}
