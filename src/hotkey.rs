use std::{collections::BTreeMap, fmt};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NormalAction {
    Navigate(Direction),
    PreviousPeriod,
    NextPeriod,
    AddEvent,
    OpenAgenda,
    MonthView,
    WeekView,
    Today,
    Reload,
    Help,
    DefaultView,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DialogAction {
    ToggleMode,
    NextField,
    PreviousField,
    ClearField,
    Save,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgendaAction {
    Navigate(Direction),
    AddEvent,
    AgendaView,
    MonthView,
    WeekView,
    Delete,
    Edit,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfirmAction {
    Confirm,
    Cancel,
}

pub trait Action: Clone + fmt::Debug + PartialEq + Eq + Serialize + DeserializeOwned {
    fn label(&self) -> String;
}

impl Action for NormalAction {
    fn label(&self) -> String {
        match self {
            Self::Navigate(Direction::Left) => "Previous day",
            Self::Navigate(Direction::Right) => "Next day",
            Self::Navigate(Direction::Up) => "Previous week",
            Self::Navigate(Direction::Down) => "Next week",
            Self::PreviousPeriod => "Previous period",
            Self::NextPeriod => "Next period",
            Self::AddEvent => "Add event",
            Self::OpenAgenda => "Open agenda",
            Self::MonthView => "Four-week view",
            Self::WeekView => "Week view",
            Self::Today => "Go to today",
            Self::Reload => "Reload calendars",
            Self::Help => "Show all hotkeys",
            Self::DefaultView => "Back to default view / quit",
        }
        .to_string()
    }
}

impl Action for AgendaAction {
    fn label(&self) -> String {
        match self {
            Self::Navigate(Direction::Up) => "Previous appointment",
            Self::Navigate(Direction::Down) => "Next appointment",
            Self::Navigate(Direction::Left) => "Previous appointment",
            Self::Navigate(Direction::Right) => "Next appointment",
            Self::AddEvent => "Add event",
            Self::AgendaView => "Agenda view",
            Self::MonthView => "Four-week view",
            Self::WeekView => "Week view",
            Self::Delete => "Delete appointment",
            Self::Edit => "Edit appointment",
            Self::Close => "Close agenda",
        }
        .to_string()
    }
}

impl Action for ConfirmAction {
    fn label(&self) -> String {
        match self {
            Self::Confirm => "Confirm deletion",
            Self::Cancel => "Cancel deletion",
        }
        .to_string()
    }
}

impl Action for DialogAction {
    fn label(&self) -> String {
        match self {
            Self::ToggleMode => "Toggle duration/exact-end mode",
            Self::NextField => "Next field",
            Self::PreviousField => "Previous field",
            Self::ClearField => "Clear field",
            Self::Save => "Save event",
            Self::Cancel => "Cancel",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sequence(pub Vec<KeyEvent>);

impl Sequence {
    pub fn parse(input: &str) -> Option<Self> {
        if input == " " {
            return Some(Self(vec![KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::NONE,
            )]));
        }
        input
            .split_whitespace()
            .map(parse_key)
            .collect::<Option<Vec<_>>>()
            .map(Self)
    }

    fn starts_with(&self, other: &Self) -> bool {
        self.0.starts_with(&other.0)
    }

    pub fn display(&self) -> String {
        self.0.iter().map(display_key).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Debug, Clone)]
pub struct HotkeyConfig<A: Action> {
    pub bindings: Vec<(A, Sequence)>,
}

impl<A: Action> HotkeyConfig<A> {
    pub fn key_for(&self, action: &A) -> Option<String> {
        self.bindings
            .iter()
            .find(|(candidate, _)| candidate == action)
            .map(|(_, sequence)| sequence.display())
    }

    pub fn rows(&self) -> Vec<(String, String)> {
        let mut rows = self
            .bindings
            .iter()
            .map(|(action, sequence)| (sequence.display(), action.label()))
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }

    pub fn continuations(&self, prefix: &Sequence) -> Vec<(String, String)> {
        let mut rows = self
            .bindings
            .iter()
            .filter(|(_, sequence)| {
                sequence.0.len() > prefix.0.len() && sequence.starts_with(prefix)
            })
            .map(|(action, sequence)| {
                (
                    Sequence(sequence.0[prefix.0.len()..].to_vec()).display(),
                    action.label(),
                )
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }
}

impl<A: Action> Serialize for HotkeyConfig<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.bindings
            .iter()
            .map(|(action, sequence)| (sequence.display(), action))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
}

impl<'de, A: Action> Deserialize<'de> for HotkeyConfig<A> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = BTreeMap::<String, A>::deserialize(deserializer)?;
        let mut bindings = Vec::with_capacity(map.len());
        for (key, action) in map {
            let sequence = Sequence::parse(&key)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid key sequence {key:?}")))?;
            bindings.push((action, sequence));
        }
        Ok(Self { bindings })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hotkeys {
    pub normal: HotkeyConfig<NormalAction>,
    pub dialog: HotkeyConfig<DialogAction>,
    pub agenda: HotkeyConfig<AgendaAction>,
    pub confirm: HotkeyConfig<ConfirmAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match<A> {
    Action(A),
    Pending,
    NoMatch,
}

#[derive(Debug, Default)]
pub struct Handler {
    pending: Sequence,
}

impl Handler {
    pub fn handle<A: Action>(&mut self, config: &HotkeyConfig<A>, mut key: KeyEvent) -> Match<A> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Match::NoMatch;
        }
        // Terminals commonly report Shift-Tab as BackTab with the SHIFT bit,
        // while crossterm's synthetic BackTab uses no modifiers.
        if key.code == KeyCode::BackTab {
            key.modifiers.remove(KeyModifiers::SHIFT);
        }
        self.pending.0.push(key);
        let matches = config
            .bindings
            .iter()
            .filter(|(_, sequence)| sequence.starts_with(&self.pending))
            .collect::<Vec<_>>();
        if let Some((action, _)) = matches
            .iter()
            .find(|(_, sequence)| sequence.0.len() == self.pending.0.len())
        {
            let action = (*action).clone();
            self.reset();
            Match::Action(action)
        } else if !matches.is_empty() {
            Match::Pending
        } else {
            self.reset();
            Match::NoMatch
        }
    }

    pub fn pending(&self) -> &Sequence {
        &self.pending
    }

    pub fn reset(&mut self) {
        self.pending.0.clear();
    }
}

fn parse_key(input: &str) -> Option<KeyEvent> {
    let mut modifiers = KeyModifiers::NONE;
    let mut remaining = input;
    while let Some((prefix, rest)) = remaining.split_once('-') {
        match prefix {
            "C" | "Ctrl" => modifiers.insert(KeyModifiers::CONTROL),
            "S" | "Shift" => modifiers.insert(KeyModifiers::SHIFT),
            "A" | "Alt" => modifiers.insert(KeyModifiers::ALT),
            "M" | "Meta" => modifiers.insert(KeyModifiers::META),
            _ => break,
        }
        remaining = rest;
    }
    let code = match remaining {
        "Backspace" => KeyCode::Backspace,
        "Enter" => KeyCode::Enter,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Tab" => KeyCode::Tab,
        "BackTab" => KeyCode::BackTab,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "Esc" => KeyCode::Esc,
        value if value.starts_with('F') => KeyCode::F(value[1..].parse().ok()?),
        value if value.chars().count() == 1 => {
            let character = value.chars().next()?;
            if character.is_uppercase() {
                modifiers.insert(KeyModifiers::SHIFT);
            }
            KeyCode::Char(character)
        }
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

fn display_key(key: &KeyEvent) -> String {
    let mut result = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        result.push_str("C-");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        result.push_str("A-");
    }
    if key.modifiers.contains(KeyModifiers::META) {
        result.push_str("M-");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT)
        && !matches!(key.code, KeyCode::Char(character) if character.is_uppercase())
    {
        result.push_str("S-");
    }
    match key.code {
        KeyCode::Char(character) => result.push(character),
        ref other => result.push_str(&format!("{other:?}")),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_hotkeys_parse_and_sequences_are_discoverable() {
        let config: crate::config::Config =
            serde_yaml::from_str(include_str!("../assets/default_config.yml")).unwrap();
        let hotkeys = config.hotkeys;
        assert_eq!(
            hotkeys.normal.key_for(&NormalAction::Today).as_deref(),
            Some("g t")
        );

        let prefix = Sequence::parse("g").unwrap();
        assert_eq!(
            hotkeys.normal.continuations(&prefix),
            vec![("t".to_string(), "Go to today".to_string())]
        );
        assert_eq!(
            hotkeys.dialog.key_for(&DialogAction::ToggleMode).as_deref(),
            Some("C-t")
        );
    }
}
