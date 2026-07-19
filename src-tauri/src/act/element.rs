//! OS-agnostic UI element model.
//!
//! Every accessibility backend (Windows UIA, macOS AX, the test mock) normalizes
//! its native tree into these types so the grounding resolver and planner see one
//! shape regardless of platform. Values are never carried here — only their
//! length — so a snapshot can be logged or sent to the planner without leaking
//! field contents (PHI-safety).

use serde::{Deserialize, Serialize};

/// A normalized control role, mapped from a native role (UIA `ControlType`,
/// macOS `AXRole`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Button,
    TextField,
    CheckBox,
    RadioButton,
    ComboBox,
    List,
    ListItem,
    Menu,
    MenuBar,
    MenuItem,
    Tab,
    TabItem,
    Link,
    Window,
    Pane,
    Group,
    Text,
    Image,
    Slider,
    Spinner,
    ProgressBar,
    ScrollBar,
    Toolbar,
    TitleBar,
    Separator,
    Tree,
    TreeItem,
    Table,
    Row,
    Cell,
    Document,
    Unknown,
}

/// Path id addressing an element within a single snapshot, e.g. `#/1/4/2`.
pub type ElementPath = String;

/// Interactive state flags on an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementState {
    Focused,
    Selected,
    Enabled,
    Disabled,
    Checked,
    Expanded,
    Offscreen,
}

/// The accessibility invoke patterns an element supports. The executor prefers
/// these over synthetic mouse/keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPattern {
    Invoke,
    SetValue,
    Toggle,
    Select,
    Expand,
    Scroll,
    Focus,
}

/// On-screen bounds in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Bounds {
    /// Center point, used for a click fallback when no invoke pattern exists.
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// A single interactive element in an accessibility snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiElement {
    pub path: ElementPath,
    pub role: Role,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Length of the element's value only — never the value itself (PHI-safety).
    #[serde(default)]
    pub value_len: usize,
    #[serde(default)]
    pub states: Vec<ElementState>,
    #[serde(default)]
    pub bounds: Option<Bounds>,
    #[serde(default)]
    pub patterns: Vec<ActionPattern>,
}

impl UiElement {
    pub fn has_state(&self, state: ElementState) -> bool {
        self.states.contains(&state)
    }

    /// Whether the element is something a user can act on (so grounding should
    /// consider it and the planner should be told about it).
    pub fn is_interactive(&self) -> bool {
        !self.patterns.is_empty()
            || matches!(
                self.role,
                Role::Button
                    | Role::TextField
                    | Role::CheckBox
                    | Role::RadioButton
                    | Role::ComboBox
                    | Role::ListItem
                    | Role::MenuItem
                    | Role::Tab
                    | Role::Link
                    | Role::Slider
            )
    }

    /// Visible = has bounds and is not explicitly offscreen.
    pub fn is_visible(&self) -> bool {
        self.bounds.is_some() && !self.has_state(ElementState::Offscreen)
    }
}

/// A snapshot of the focused window's interactive elements plus focus / pointer /
/// selection context. This is the L0+L1 tier the grounding resolver works over.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub app: String,
    pub window_title: String,
    #[serde(default)]
    pub focused: Option<ElementPath>,
    #[serde(default)]
    pub pointer: Option<ElementPath>,
    /// Length of the current text selection, if any (never its contents).
    #[serde(default)]
    pub selection_text_len: usize,
    pub elements: Vec<UiElement>,
}

impl Snapshot {
    pub fn get(&self, path: &str) -> Option<&UiElement> {
        self.elements.iter().find(|e| e.path == path)
    }

    pub fn focused_element(&self) -> Option<&UiElement> {
        self.focused.as_deref().and_then(|p| self.get(p))
    }

    pub fn pointer_element(&self) -> Option<&UiElement> {
        self.pointer.as_deref().and_then(|p| self.get(p))
    }

    /// Visible interactive candidates in the snapshot's (reading/focus) order.
    pub fn interactive(&self) -> impl Iterator<Item = &UiElement> {
        self.elements
            .iter()
            .filter(|e| e.is_interactive() && e.is_visible())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn bounds_center_is_midpoint() {
        let b = Bounds {
            x: 10,
            y: 20,
            w: 40,
            h: 60,
        };
        assert_eq!(b.center(), (30, 50));
    }

    #[test]
    fn interactive_filters_visible_actionable() {
        let mut offscreen = el("#/2", Role::Button, "Hidden");
        offscreen.states = vec![ElementState::Offscreen];
        let snap = Snapshot {
            app: "Test".into(),
            window_title: "W".into(),
            focused: Some("#/1".into()),
            pointer: None,
            selection_text_len: 0,
            elements: vec![
                el("#/1", Role::TextField, "Message"),
                offscreen,
                el("#/3", Role::Button, "Send"),
            ],
        };
        let names: Vec<_> = snap.interactive().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Message", "Send"]);
        assert_eq!(snap.focused_element().unwrap().name, "Message");
    }

    #[test]
    fn value_length_only_never_content() {
        // The schema has no field that can carry the raw value — only value_len.
        let e = el("#/1", Role::TextField, "SSN");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("value_len"));
        assert!(!json.contains("\"value\""));
    }
}
