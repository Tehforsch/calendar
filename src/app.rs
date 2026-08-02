use std::collections::{BTreeMap, BTreeSet};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use eyre::Result;
use jiff::{ToSpan, Zoned, civil::Date, tz::TimeZone};
use tui_input::{Input, InputRequest};

use crate::{
    config::{Config, ViewMode},
    date_input::{DateParser, parse_clock, parse_duration_minutes},
    hotkey::{
        AgendaAction, ConfirmAction, DialogAction, Direction, Handler, Match, NormalAction,
        SearchAction,
    },
    store::{CalendarEvent, CalendarStore, EventOccurrence, EventTiming},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditField {
    Description,
    StartDate,
    StartTime,
    Duration,
    ExactStart,
    ExactEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    Duration,
    ExactRange,
}

#[derive(Debug, Clone)]
pub struct EditState {
    pub description: Input,
    pub start_date: Input,
    pub start_time: Input,
    pub duration: Input,
    pub exact_start: Input,
    pub exact_end: Input,
    pub edit_mode: EditMode,
    pub field: EditField,
    pub error: Option<String>,
    target: Option<EditTarget>,
}

impl EditState {
    pub fn active_input(&self) -> &Input {
        match self.field {
            EditField::Description => &self.description,
            EditField::StartDate => &self.start_date,
            EditField::StartTime => &self.start_time,
            EditField::Duration => &self.duration,
            EditField::ExactStart => &self.exact_start,
            EditField::ExactEnd => &self.exact_end,
        }
    }

    fn active_input_mut(&mut self) -> &mut Input {
        match self.field {
            EditField::Description => &mut self.description,
            EditField::StartDate => &mut self.start_date,
            EditField::StartTime => &mut self.start_time,
            EditField::Duration => &mut self.duration,
            EditField::ExactStart => &mut self.exact_start,
            EditField::ExactEnd => &mut self.exact_end,
        }
    }

    fn next_field(&mut self) {
        self.field = match (self.edit_mode, self.field) {
            (EditMode::Duration, EditField::Description) => EditField::StartDate,
            (EditMode::Duration, EditField::StartDate) => EditField::StartTime,
            (EditMode::Duration, EditField::StartTime) => EditField::Duration,
            (EditMode::Duration, EditField::Duration) => EditField::Description,
            (EditMode::ExactRange, EditField::Description) => EditField::ExactStart,
            (EditMode::ExactRange, EditField::ExactStart) => EditField::ExactEnd,
            (EditMode::ExactRange, EditField::ExactEnd) => EditField::Description,
            (EditMode::Duration, _) => EditField::StartDate,
            (EditMode::ExactRange, _) => EditField::ExactStart,
        };
    }

    fn previous_field(&mut self) {
        self.field = match (self.edit_mode, self.field) {
            (EditMode::Duration, EditField::Description) => EditField::Duration,
            (EditMode::Duration, EditField::StartDate) => EditField::Description,
            (EditMode::Duration, EditField::StartTime) => EditField::StartDate,
            (EditMode::Duration, EditField::Duration) => EditField::StartTime,
            (EditMode::ExactRange, EditField::Description) => EditField::ExactEnd,
            (EditMode::ExactRange, EditField::ExactStart) => EditField::Description,
            (EditMode::ExactRange, EditField::ExactEnd) => EditField::ExactStart,
            (EditMode::Duration, _) => EditField::Duration,
            (EditMode::ExactRange, _) => EditField::ExactEnd,
        };
    }

    fn toggle_mode(&mut self) {
        self.edit_mode = match self.edit_mode {
            EditMode::Duration => EditMode::ExactRange,
            EditMode::ExactRange => EditMode::Duration,
        };
        if self.field != EditField::Description {
            self.field = match self.edit_mode {
                EditMode::Duration => EditField::StartDate,
                EditMode::ExactRange => EditField::ExactStart,
            };
        }
        self.error = None;
    }

    pub fn is_editing(&self) -> bool {
        self.target.is_some()
    }

    pub fn is_recurring(&self) -> bool {
        self.target.as_ref().is_some_and(|target| target.recurring)
    }
}

#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Agenda(AgendaState),
    ConfirmDelete(Box<AgendaState>),
    Edit(Box<EditState>),
    Search(SearchState),
    Help,
}

#[derive(Debug, Clone)]
pub struct AgendaItem {
    pub event_index: usize,
    pub start: Zoned,
    pub end: Zoned,
}

#[derive(Debug, Clone)]
pub struct AgendaState {
    pub center_date: Date,
    pub items: Vec<AgendaItem>,
    pub selected: usize,
}

impl AgendaState {
    pub fn selected_item(&self) -> Option<&AgendaItem> {
        self.items.get(self.selected)
    }
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: Input,
    candidates: Vec<AgendaItem>,
    pub items: Vec<AgendaItem>,
    pub selected: usize,
}

impl SearchState {
    fn new(
        candidates: Vec<AgendaItem>,
        events: &[CalendarEvent],
        today: Date,
        timezone: &TimeZone,
    ) -> Self {
        let mut state = Self {
            query: Input::default(),
            candidates,
            items: Vec::new(),
            selected: 0,
        };
        state.refresh(events, today, timezone);
        state
    }

    fn refresh(&mut self, events: &[CalendarEvent], today: Date, timezone: &TimeZone) {
        let query = self.query.value();
        self.items = self
            .candidates
            .iter()
            .filter(|item| fuzzy_matches(&events[item.event_index].summary, query))
            .cloned()
            .collect();
        self.selected = self
            .items
            .iter()
            .position(|item| item.start.timestamp().to_zoned(timezone.clone()).date() >= today)
            .unwrap_or_else(|| self.items.len().saturating_sub(1));
    }

    pub fn selected_item(&self) -> Option<&AgendaItem> {
        self.items.get(self.selected)
    }
}

#[derive(Debug, Clone)]
struct EditTarget {
    event_index: usize,
    agenda_center: Date,
    recurring: bool,
    occurrence_start: Zoned,
}

pub struct App {
    pub config: Config,
    pub store: CalendarStore,
    pub events: Vec<CalendarEvent>,
    pub now: Zoned,
    pub selected: Date,
    pub view: ViewMode,
    pub mode: Mode,
    pub status: Option<String>,
    view_start: Date,
    occurrence_cache: BTreeMap<Date, Vec<CachedOccurrence>>,
    occurrence_cache_start: Date,
    occurrence_cache_end: Date,
    hotkey_handler: Handler,
    should_quit: bool,
}

#[derive(Debug, Clone)]
struct CachedOccurrence {
    event_index: usize,
    start: Zoned,
    end: Zoned,
}

enum EventDraft {
    Timed { start: Zoned, end: Zoned },
    AllDay { date: Date },
}

impl App {
    pub fn load(config: Config, store: CalendarStore) -> Result<Self> {
        let now = jiff::Timestamp::now().to_zoned(store.display_timezone().clone());
        Self::new_at(config, store, now)
    }

    pub fn new_at(config: Config, store: CalendarStore, now: Zoned) -> Result<Self> {
        let selected = now.date();
        let view_start = week_start(selected);
        let view = config.default_view;
        let mut app = Self {
            config,
            store,
            events: Vec::new(),
            now,
            selected,
            view,
            mode: Mode::Normal,
            status: None,
            view_start,
            occurrence_cache: BTreeMap::new(),
            occurrence_cache_start: view_start,
            occurrence_cache_end: view_start,
            hotkey_handler: Handler::default(),
            should_quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    pub fn timezone(&self) -> &TimeZone {
        self.store.display_timezone()
    }

    pub fn view_start(&self) -> Date {
        match self.view {
            ViewMode::Month => self.view_start,
            ViewMode::Week => week_start(self.selected),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn pending_hotkeys(&self) -> Vec<(String, String)> {
        if self.hotkey_handler.pending().0.is_empty() {
            return Vec::new();
        }
        match self.mode {
            Mode::Normal => self
                .config
                .hotkeys
                .normal
                .continuations(self.hotkey_handler.pending()),
            Mode::Edit(_) => self
                .config
                .hotkeys
                .dialog
                .continuations(self.hotkey_handler.pending()),
            Mode::Agenda(_) => self
                .config
                .hotkeys
                .agenda
                .continuations(self.hotkey_handler.pending()),
            Mode::ConfirmDelete(_) => self
                .config
                .hotkeys
                .confirm
                .continuations(self.hotkey_handler.pending()),
            Mode::Search(_) => self
                .config
                .hotkeys
                .search
                .continuations(self.hotkey_handler.pending()),
            Mode::Help => Vec::new(),
        }
    }

    pub fn pending_sequence(&self) -> String {
        self.hotkey_handler.pending().display()
    }

    pub fn events_on(&self, date: Date) -> Vec<EventOccurrence<'_>> {
        self.occurrence_cache
            .get(&date)
            .into_iter()
            .flatten()
            .map(|occurrence| EventOccurrence {
                event: &self.events[occurrence.event_index],
                start: occurrence.start.clone(),
                end: occurrence.end.clone(),
            })
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(());
        }
        if matches!(self.mode, Mode::Help) {
            self.mode = Mode::Normal;
            self.view = self.config.default_view;
            self.keep_selection_visible();
            self.ensure_visible_occurrences_are_cached();
            self.hotkey_handler.reset();
            return Ok(());
        }

        if matches!(self.mode, Mode::Normal) {
            match self.hotkey_handler.handle(&self.config.hotkeys.normal, key) {
                Match::Action(action) => self.handle_normal(action),
                Match::Pending | Match::NoMatch => Ok(()),
            }
        } else if matches!(self.mode, Mode::Agenda(_)) {
            match self.hotkey_handler.handle(&self.config.hotkeys.agenda, key) {
                Match::Action(action) => self.handle_agenda(action),
                Match::Pending | Match::NoMatch => Ok(()),
            }
        } else if matches!(self.mode, Mode::ConfirmDelete(_)) {
            match self
                .hotkey_handler
                .handle(&self.config.hotkeys.confirm, key)
            {
                Match::Action(action) => self.handle_confirmation(action),
                Match::Pending | Match::NoMatch => Ok(()),
            }
        } else if matches!(self.mode, Mode::Search(_)) {
            match self.hotkey_handler.handle(&self.config.hotkeys.search, key) {
                Match::Action(action) => self.handle_search(action),
                Match::Pending => Ok(()),
                Match::NoMatch => {
                    self.handle_search_text_input(key);
                    Ok(())
                }
            }
        } else {
            match self.hotkey_handler.handle(&self.config.hotkeys.dialog, key) {
                Match::Action(action) => self.handle_dialog(action),
                Match::Pending => Ok(()),
                Match::NoMatch => {
                    self.handle_text_input(key);
                    Ok(())
                }
            }
        }
    }

    pub fn handle_normal(&mut self, action: NormalAction) -> Result<()> {
        match action {
            NormalAction::Navigate(direction) => {
                let days = match direction {
                    Direction::Left => -1,
                    Direction::Right => 1,
                    Direction::Up => -7,
                    Direction::Down => 7,
                };
                self.selected += days.days();
                self.keep_selection_visible();
            }
            NormalAction::PreviousPeriod => {
                let days = if self.view == ViewMode::Month {
                    -28
                } else {
                    -7
                };
                self.selected += days.days();
                self.view_start += days.days();
            }
            NormalAction::NextPeriod => {
                let days = if self.view == ViewMode::Month { 28 } else { 7 };
                self.selected += days.days();
                self.view_start += days.days();
            }
            NormalAction::AddEvent => self.open_editor(),
            NormalAction::OpenAgenda => self.open_agenda(),
            NormalAction::MonthView => {
                self.view = ViewMode::Month;
                self.keep_selection_visible();
            }
            NormalAction::WeekView => self.view = ViewMode::Week,
            NormalAction::Today => {
                self.selected = self.now.date();
                self.view_start = week_start(self.selected);
            }
            NormalAction::Reload => {
                self.reload()?;
                self.status = Some(format!("Reloaded {} events", self.events.len()));
            }
            NormalAction::Help => self.mode = Mode::Help,
            NormalAction::Search => self.open_search(),
            NormalAction::DefaultView => {
                if self.view == self.config.default_view {
                    self.should_quit = true;
                } else {
                    self.view = self.config.default_view;
                    self.keep_selection_visible();
                }
            }
        }
        self.ensure_visible_occurrences_are_cached();
        Ok(())
    }

    pub fn handle_search(&mut self, action: SearchAction) -> Result<()> {
        match action {
            SearchAction::Navigate(direction) => {
                if let Mode::Search(search) = &mut self.mode
                    && !search.items.is_empty()
                {
                    match direction {
                        Direction::Up | Direction::Left => {
                            search.selected = search.selected.saturating_sub(1);
                        }
                        Direction::Down | Direction::Right => {
                            search.selected = (search.selected + 1).min(search.items.len() - 1);
                        }
                    }
                }
            }
            SearchAction::Select => {
                let item = match &self.mode {
                    Mode::Search(search) => search.selected_item().cloned(),
                    _ => None,
                };
                if let Some(item) = item {
                    let date = item
                        .start
                        .timestamp()
                        .to_zoned(self.timezone().clone())
                        .date();
                    self.selected = date;
                    self.view_start = week_start(date);
                    self.rebuild_occurrence_cache();
                    self.mode = Mode::Agenda(self.build_agenda(date, Some(item.start.timestamp())));
                }
            }
            SearchAction::Cancel => {
                self.view = self.config.default_view;
                self.mode = Mode::Normal;
                self.keep_selection_visible();
                self.ensure_visible_occurrences_are_cached();
            }
        }
        self.hotkey_handler.reset();
        Ok(())
    }

    pub fn handle_agenda(&mut self, action: AgendaAction) -> Result<()> {
        let timezone = self.timezone().clone();
        match action {
            AgendaAction::Navigate(direction) => {
                if let Mode::Agenda(agenda) = &mut self.mode
                    && !agenda.items.is_empty()
                {
                    match direction {
                        Direction::Up | Direction::Left => {
                            agenda.selected = agenda.selected.saturating_sub(1);
                        }
                        Direction::Down | Direction::Right => {
                            agenda.selected = (agenda.selected + 1).min(agenda.items.len() - 1);
                        }
                    }
                    self.selected = agenda.items[agenda.selected]
                        .start
                        .timestamp()
                        .to_zoned(timezone)
                        .date();
                }
            }
            AgendaAction::AddEvent => self.open_editor(),
            AgendaAction::AgendaView => {
                self.mode = Mode::Agenda(self.build_agenda(self.selected, None));
            }
            AgendaAction::MonthView => {
                self.view = ViewMode::Month;
                self.mode = Mode::Normal;
                self.keep_selection_visible();
                self.ensure_visible_occurrences_are_cached();
            }
            AgendaAction::WeekView => {
                self.view = ViewMode::Week;
                self.mode = Mode::Normal;
                self.ensure_visible_occurrences_are_cached();
            }
            AgendaAction::Delete => {
                if let Mode::Agenda(agenda) = &self.mode
                    && agenda.selected_item().is_some()
                {
                    self.mode = Mode::ConfirmDelete(Box::new(agenda.clone()));
                }
            }
            AgendaAction::Edit => self.open_event_editor(),
            AgendaAction::Close => {
                self.view = self.config.default_view;
                self.mode = Mode::Normal;
                self.keep_selection_visible();
                self.ensure_visible_occurrences_are_cached();
            }
        }
        self.hotkey_handler.reset();
        Ok(())
    }

    pub fn handle_confirmation(&mut self, action: ConfirmAction) -> Result<()> {
        let Mode::ConfirmDelete(agenda) = &self.mode else {
            return Ok(());
        };
        let agenda = (**agenda).clone();
        match action {
            ConfirmAction::Cancel => {
                self.view = self.config.default_view;
                self.mode = Mode::Normal;
                self.keep_selection_visible();
                self.ensure_visible_occurrences_are_cached();
            }
            ConfirmAction::Confirm => {
                let Some(item) = agenda.selected_item().cloned() else {
                    self.mode = Mode::Agenda(agenda);
                    return Ok(());
                };
                let center = agenda.center_date;
                let event = &self.events[item.event_index];
                let summary = event.summary.clone();
                self.store.delete_occurrence(event, &item.start)?;
                self.reload()?;
                self.mode = Mode::Agenda(self.build_agenda(center, None));
                self.status = Some(format!("Deleted {summary}"));
            }
        }
        self.hotkey_handler.reset();
        Ok(())
    }

    pub fn handle_dialog(&mut self, action: DialogAction) -> Result<()> {
        match action {
            DialogAction::ToggleMode => {
                if let Mode::Edit(editor) = &mut self.mode {
                    editor.toggle_mode();
                }
                Ok(())
            }
            DialogAction::NextField => {
                if let Mode::Edit(editor) = &mut self.mode {
                    editor.next_field();
                    editor.error = None;
                }
                Ok(())
            }
            DialogAction::PreviousField => {
                if let Mode::Edit(editor) = &mut self.mode {
                    editor.previous_field();
                    editor.error = None;
                }
                Ok(())
            }
            DialogAction::ClearField => {
                if let Mode::Edit(editor) = &mut self.mode {
                    editor.active_input_mut().reset();
                    editor.error = None;
                }
                Ok(())
            }
            DialogAction::Cancel => {
                self.view = self.config.default_view;
                self.mode = Mode::Normal;
                self.keep_selection_visible();
                self.ensure_visible_occurrences_are_cached();
                self.hotkey_handler.reset();
                Ok(())
            }
            DialogAction::Save => self.save_editor(),
        }
    }

    pub fn type_text(&mut self, text: &str) {
        if let Mode::Edit(editor) = &mut self.mode {
            for character in text.chars() {
                editor
                    .active_input_mut()
                    .handle(InputRequest::InsertChar(character));
            }
        }
    }

    fn open_editor(&mut self) {
        self.mode = Mode::Edit(Box::new(EditState {
            description: Input::default(),
            start_date: Input::new(self.selected.to_string()),
            start_time: Input::default(),
            duration: Input::default(),
            exact_start: Input::default(),
            exact_end: Input::default(),
            edit_mode: EditMode::Duration,
            field: EditField::Description,
            error: None,
            target: None,
        }));
        self.hotkey_handler.reset();
    }

    fn open_agenda(&mut self) {
        self.ensure_visible_occurrences_are_cached();
        self.mode = Mode::Agenda(self.build_agenda(self.selected, None));
        self.hotkey_handler.reset();
    }

    fn open_search(&mut self) {
        let timezone = self.timezone().clone();
        let today = self.now.date();
        let mut candidates = self
            .events
            .iter()
            .enumerate()
            .map(|(event_index, event)| {
                let occurrence = event.representative_occurrence(today, &timezone);
                AgendaItem {
                    event_index,
                    start: occurrence.start,
                    end: occurrence.end,
                }
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.start
                .timestamp()
                .cmp(&right.start.timestamp())
                .then_with(|| {
                    self.events[left.event_index]
                        .summary
                        .cmp(&self.events[right.event_index].summary)
                })
        });
        self.mode = Mode::Search(SearchState::new(candidates, &self.events, today, &timezone));
        self.hotkey_handler.reset();
    }

    fn build_agenda(
        &self,
        center_date: Date,
        preferred_start: Option<jiff::Timestamp>,
    ) -> AgendaState {
        const AGENDA_RADIUS_DAYS: i64 = 14;

        let start_date = center_date - AGENDA_RADIUS_DAYS.days();
        let end_date = center_date + AGENDA_RADIUS_DAYS.days();
        let mut seen = BTreeSet::new();
        let mut items = Vec::new();
        let mut date = start_date;
        while date <= end_date {
            for occurrence in self.occurrence_cache.get(&date).into_iter().flatten() {
                if seen.insert((occurrence.event_index, occurrence.start.timestamp())) {
                    items.push(AgendaItem {
                        event_index: occurrence.event_index,
                        start: occurrence.start.clone(),
                        end: occurrence.end.clone(),
                    });
                }
            }
            date += 1.day();
        }
        items.sort_by(|left, right| {
            left.start
                .timestamp()
                .cmp(&right.start.timestamp())
                .then_with(|| {
                    self.events[left.event_index]
                        .summary
                        .cmp(&self.events[right.event_index].summary)
                })
        });

        let timezone = self.timezone().clone();
        let selected = preferred_start
            .and_then(|preferred| {
                items
                    .iter()
                    .position(|item| item.start.timestamp() == preferred)
            })
            .or_else(|| {
                items.iter().position(|item| {
                    item.start.timestamp().to_zoned(timezone.clone()).date() == center_date
                })
            })
            .or_else(|| {
                let center = center_date
                    .at(12, 0, 0, 0)
                    .to_zoned(timezone)
                    .ok()?
                    .timestamp();
                items
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, item)| {
                        (item.start.timestamp().as_second() - center.as_second()).abs()
                    })
                    .map(|(index, _)| index)
            })
            .unwrap_or(0);

        AgendaState {
            center_date,
            items,
            selected,
        }
    }

    fn open_event_editor(&mut self) {
        let Mode::Agenda(agenda) = &self.mode else {
            return;
        };
        let Some(item) = agenda.selected_item().cloned() else {
            return;
        };
        let center = agenda.center_date;
        let event = &self.events[item.event_index];
        let start = item.start.timestamp().to_zoned(self.timezone().clone());
        let end = item.end.timestamp().to_zoned(self.timezone().clone());
        let duration_minutes =
            ((end.timestamp().as_second() - start.timestamp().as_second()) / 60).max(1);
        let duration = if duration_minutes % 60 == 0 {
            format!("{}h", duration_minutes / 60)
        } else if duration_minutes > 60 {
            format!("{}h{}m", duration_minutes / 60, duration_minutes % 60)
        } else {
            format!("{duration_minutes}m")
        };
        let recurring = event.is_recurring();
        self.selected = start.date();
        self.mode = Mode::Edit(Box::new(EditState {
            description: Input::new(event.summary.clone()),
            start_date: Input::new(start.date().to_string()),
            start_time: Input::new(if event.all_day {
                String::new()
            } else {
                start.strftime("%H:%M").to_string()
            }),
            duration: Input::new(if event.all_day {
                String::new()
            } else {
                duration
            }),
            exact_start: Input::new(start.strftime("%Y-%m-%d %H:%M").to_string()),
            exact_end: Input::new(end.strftime("%Y-%m-%d %H:%M").to_string()),
            edit_mode: EditMode::Duration,
            field: EditField::Description,
            error: None,
            target: Some(EditTarget {
                event_index: item.event_index,
                agenda_center: center,
                recurring,
                occurrence_start: item.start,
            }),
        }));
        self.hotkey_handler.reset();
    }

    fn save_editor(&mut self) -> Result<()> {
        let Mode::Edit(editor) = &self.mode else {
            return Ok(());
        };
        let description = editor.description.value().trim().to_string();
        let edit_mode = editor.edit_mode;
        let start_date_input = editor.start_date.value().trim().to_string();
        let start_time_input = editor.start_time.value().trim().to_string();
        let duration_input = editor.duration.value().trim().to_string();
        let exact_start_input = editor.exact_start.value().trim().to_string();
        let exact_end_input = editor.exact_end.value().trim().to_string();
        let target = editor.target.clone();
        if description.is_empty() {
            self.set_editor_error("Description cannot be empty");
            return Ok(());
        }
        let parser = match DateParser::new(
            self.timezone().clone(),
            self.now.clone(),
            &self.config.default_start_time,
        ) {
            Ok(parser) => parser,
            Err(error) => {
                self.set_editor_error(error);
                return Ok(());
            }
        };
        let draft = match edit_mode {
            EditMode::Duration => self.duration_draft(
                &parser,
                &start_date_input,
                &start_time_input,
                &duration_input,
            ),
            EditMode::ExactRange => {
                self.exact_range_draft(&parser, &exact_start_input, &exact_end_input)
            }
        };
        let draft = match draft {
            Ok(draft) => draft,
            Err(error) => {
                self.set_editor_error(error);
                return Ok(());
            }
        };

        let selected = match &draft {
            EventDraft::Timed { start, .. } => {
                start.timestamp().to_zoned(self.timezone().clone()).date()
            }
            EventDraft::AllDay { date } => *date,
        };
        let preferred_start = match &draft {
            EventDraft::Timed { start, .. } => Some(start.timestamp()),
            EventDraft::AllDay { date } => date
                .at(0, 0, 0, 0)
                .to_zoned(self.timezone().clone())
                .ok()
                .map(|start| start.timestamp()),
        };
        let status = if target.is_some() {
            format!("Updated {description}")
        } else {
            match &draft {
                EventDraft::Timed { start, .. } => format!(
                    "Saved {} at {}",
                    description,
                    start.strftime("%a %-d %b %H:%M")
                ),
                EventDraft::AllDay { date } => format!(
                    "Saved {} all day on {}",
                    description,
                    date.strftime("%a %-d %b")
                ),
            }
        };
        if let Some(target) = &target {
            let timing = match draft {
                EventDraft::Timed { start, end } => EventTiming::Timed { start, end },
                EventDraft::AllDay { date } => EventTiming::AllDay { date },
            };
            self.store.update_occurrence(
                &self.events[target.event_index],
                &target.occurrence_start,
                &description,
                timing,
            )?;
        } else {
            match draft {
                EventDraft::Timed { start, end } => {
                    self.store.add_event(&description, &start, &end)?;
                }
                EventDraft::AllDay { date } => {
                    self.store.add_all_day_event(&description, date)?;
                }
            }
        }
        self.reload()?;
        self.selected = selected;
        self.keep_selection_visible();
        self.ensure_visible_occurrences_are_cached();
        self.mode = if let Some(target) = target {
            Mode::Agenda(self.build_agenda(target.agenda_center, preferred_start))
        } else {
            Mode::Normal
        };
        self.hotkey_handler.reset();
        self.status = Some(status);
        Ok(())
    }

    fn duration_draft(
        &self,
        parser: &DateParser,
        date_input: &str,
        time_input: &str,
        duration_input: &str,
    ) -> Result<EventDraft, String> {
        let date = parser
            .parse(date_input, Some(self.selected))
            .map_err(|error| format!("Start date: {error}"))?
            .date();
        if time_input.is_empty() && duration_input.is_empty() {
            return Ok(EventDraft::AllDay { date });
        }
        if time_input.is_empty() {
            return Err("Start time is required when duration is set".to_string());
        }
        if duration_input.is_empty() {
            return Err("Duration is required when start time is set".to_string());
        }
        let time = parse_clock(time_input)
            .ok_or_else(|| "Start time: use a value such as 13:00 or 5pm".to_string())?;
        let duration_minutes =
            parse_duration_minutes(duration_input).map_err(|error| format!("Duration: {error}"))?;
        let start = date
            .at(time.hour(), time.minute(), 0, 0)
            .to_zoned(self.timezone().clone())
            .map_err(|error| format!("Start: {error}"))?;
        let end = &start + duration_minutes.minutes();
        Ok(EventDraft::Timed { start, end })
    }

    fn exact_range_draft(
        &self,
        parser: &DateParser,
        start_input: &str,
        end_input: &str,
    ) -> Result<EventDraft, String> {
        let start = parser
            .parse(start_input, Some(self.selected))
            .map_err(|error| format!("Start: {error}"))?;
        let mut end = parser
            .parse(end_input, Some(start.date()))
            .map_err(|error| format!("End: {error}"))?;
        if parse_clock(end_input).is_some() && end.timestamp() <= start.timestamp() {
            end = &end + 1.day();
        }
        if end.timestamp() <= start.timestamp() {
            return Err("End must be after start".to_string());
        }
        Ok(EventDraft::Timed { start, end })
    }

    fn set_editor_error(&mut self, error: impl Into<String>) {
        if let Mode::Edit(editor) = &mut self.mode {
            editor.error = Some(error.into());
        }
    }

    fn handle_text_input(&mut self, key: KeyEvent) {
        let request = input_request(key);
        if let (Some(request), Mode::Edit(editor)) = (request, &mut self.mode) {
            editor.active_input_mut().handle(request);
            editor.error = None;
        }
    }

    fn handle_search_text_input(&mut self, key: KeyEvent) {
        let Some(request) = input_request(key) else {
            return;
        };
        let timezone = self.timezone().clone();
        if let Mode::Search(search) = &mut self.mode {
            search.query.handle(request);
            search.refresh(&self.events, self.now.date(), &timezone);
        }
    }

    fn reload(&mut self) -> Result<()> {
        let loaded = self.store.load()?;
        self.events = loaded.events;
        self.rebuild_occurrence_cache();
        if !loaded.warnings.is_empty() {
            self.status = Some(format!(
                "Loaded with {} warning{} (press ? for keys)",
                loaded.warnings.len(),
                if loaded.warnings.len() == 1 { "" } else { "s" }
            ));
        }
        Ok(())
    }

    fn keep_selection_visible(&mut self) {
        if self.view != ViewMode::Month {
            return;
        }
        if self.selected < self.view_start {
            self.view_start = week_start(self.selected);
        } else if self.selected >= self.view_start + 28.days() {
            self.view_start = week_start(self.selected) - 21.days();
        }
    }

    fn ensure_visible_occurrences_are_cached(&mut self) {
        let visible_start = self.view_start();
        let visible_days = match self.view {
            ViewMode::Month => 28,
            ViewMode::Week => 7,
        };
        let visible_end = visible_start + visible_days.days();
        if visible_start < self.occurrence_cache_start || visible_end > self.occurrence_cache_end {
            self.rebuild_occurrence_cache();
        }
    }

    fn rebuild_occurrence_cache(&mut self) {
        const CACHE_MARGIN_DAYS: i64 = 14;

        let visible_start = self.view_start();
        let visible_days = match self.view {
            ViewMode::Month => 28,
            ViewMode::Week => 7,
        };
        let cache_start = visible_start - CACHE_MARGIN_DAYS.days();
        let cache_end = visible_start + (visible_days + CACHE_MARGIN_DAYS).days();
        let timezone = self.timezone().clone();
        let mut cache = BTreeMap::new();
        let mut date = cache_start;
        while date < cache_end {
            let mut occurrences = Vec::new();
            for (event_index, event) in self.events.iter().enumerate() {
                occurrences.extend(event.occurrences_on(date, &timezone).into_iter().map(
                    |occurrence| CachedOccurrence {
                        event_index,
                        start: occurrence.start,
                        end: occurrence.end,
                    },
                ));
            }
            occurrences.sort_by_key(|occurrence| occurrence.start.timestamp());
            cache.insert(date, occurrences);
            date += 1.day();
        }
        self.occurrence_cache = cache;
        self.occurrence_cache_start = cache_start;
        self.occurrence_cache_end = cache_end;
    }
}

fn input_request(key: KeyEvent) -> Option<InputRequest> {
    match (key.code, key.modifiers) {
        (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
            Some(InputRequest::InsertChar(character))
        }
        (KeyCode::Backspace, KeyModifiers::NONE) => Some(InputRequest::DeletePrevChar),
        (KeyCode::Delete, KeyModifiers::NONE) => Some(InputRequest::DeleteNextChar),
        (KeyCode::Left, KeyModifiers::NONE) => Some(InputRequest::GoToPrevChar),
        (KeyCode::Right, KeyModifiers::NONE) => Some(InputRequest::GoToNextChar),
        (KeyCode::Home, KeyModifiers::NONE) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
            Some(InputRequest::GoToStart)
        }
        (KeyCode::End, KeyModifiers::NONE) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
            Some(InputRequest::GoToEnd)
        }
        (KeyCode::Char('w'), KeyModifiers::CONTROL) => Some(InputRequest::DeletePrevWord),
        _ => None,
    }
}

fn fuzzy_matches(value: &str, query: &str) -> bool {
    let mut value = value.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|needle| value.by_ref().any(|candidate| candidate == needle))
}

pub fn week_start(date: Date) -> Date {
    date - i64::from(date.weekday().to_monday_zero_offset()).days()
}
