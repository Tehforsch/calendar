use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use jiff::{civil::date, tz::TimeZone};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;

use crate::{
    app::App,
    config::Config,
    hotkey::{Action, AgendaAction, ConfirmAction, DialogAction, HotkeyConfig, NormalAction},
    store::CalendarStore,
    ui,
};

const WIDTH: u16 = 112;
const HEIGHT: u16 = 36;

#[derive(Debug, Clone)]
pub enum Instruction {
    Normal(NormalAction),
    Agenda(AgendaAction),
    Confirm(ConfirmAction),
    Dialog(DialogAction),
    Type(&'static str),
    Raw(KeyEvent),
    Snapshot(&'static str),
}

pub struct Harness {
    _directory: TempDir,
    pub app: App,
    test_name: &'static str,
}

impl Harness {
    pub fn new(test_name: &'static str) -> Self {
        let directory = TempDir::new().unwrap();
        let config = Config {
            calendar_dir: directory.path().to_path_buf(),
            default_calendar: "personal".to_string(),
            ..Config::default()
        };
        let timezone = TimeZone::UTC;
        let now = date(2026, 6, 1)
            .at(8, 0, 0, 0)
            .to_zoned(timezone.clone())
            .unwrap();
        let store = CalendarStore::with_timezone(
            directory.path().to_path_buf(),
            config.default_calendar.clone(),
            timezone,
        );
        let app = App::new_at(config, store, now).unwrap();
        Self {
            _directory: directory,
            app,
            test_name,
        }
    }

    pub fn run(&mut self, instructions: impl IntoIterator<Item = Instruction>) {
        for instruction in instructions {
            match instruction {
                Instruction::Normal(action) => {
                    let sequence = lookup(&self.app.config.hotkeys.normal, &action);
                    for key in sequence {
                        self.app.handle_key(key).unwrap();
                    }
                }
                Instruction::Agenda(action) => {
                    let sequence = lookup(&self.app.config.hotkeys.agenda, &action);
                    for key in sequence {
                        self.app.handle_key(key).unwrap();
                    }
                }
                Instruction::Confirm(action) => {
                    let sequence = lookup(&self.app.config.hotkeys.confirm, &action);
                    for key in sequence {
                        self.app.handle_key(key).unwrap();
                    }
                }
                Instruction::Dialog(action) => {
                    let sequence = lookup(&self.app.config.hotkeys.dialog, &action);
                    for key in sequence {
                        self.app.handle_key(key).unwrap();
                    }
                }
                Instruction::Type(text) => {
                    for character in text.chars() {
                        self.app
                            .handle_key(KeyEvent::new(
                                KeyCode::Char(character),
                                if character.is_uppercase() {
                                    KeyModifiers::SHIFT
                                } else {
                                    KeyModifiers::NONE
                                },
                            ))
                            .unwrap();
                    }
                }
                Instruction::Raw(key) => self.app.handle_key(key).unwrap(),
                Instruction::Snapshot(label) => {
                    insta::assert_snapshot!(format!("{}_{}", self.test_name, label), self.render());
                }
            }
        }
    }

    pub fn render(&self) -> String {
        let backend = TestBackend::new(WIDTH, HEIGHT);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| ui::draw(frame, &self.app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                let mut line = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect::<String>();
                while line.ends_with(' ') {
                    line.pop();
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }
}

fn lookup<A: Action>(config: &HotkeyConfig<A>, action: &A) -> Vec<KeyEvent> {
    config
        .bindings
        .iter()
        .find(|(candidate, _)| candidate == action)
        .unwrap_or_else(|| panic!("no configured binding for {action:?}"))
        .1
        .0
        .clone()
}

pub fn key(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
}
