use jiff::{ToSpan, civil::date};

use crate::{
    config::ViewMode,
    hotkey::{AgendaAction, ConfirmAction, DialogAction, Direction, NormalAction, Sequence},
    store::CalendarEvent,
};

use super::utils::{Harness, Instruction, escape, key};

#[test]
fn navigation_and_week_view_are_driven_by_configured_keys() {
    let mut harness = Harness::new("navigation_and_week_view");
    harness.run([
        Instruction::Raw(key('l')),
        Instruction::Raw(key('j')),
        Instruction::Snapshot("four_week_after_hjkl"),
        Instruction::Normal(NormalAction::WeekView),
        Instruction::Snapshot("week"),
    ]);

    assert_eq!(harness.app.selected, date(2026, 6, 9));
    assert_eq!(harness.app.view, ViewMode::Week);
}

#[test]
fn wasd_navigation_and_prefixed_view_keys_are_available() {
    let mut harness = Harness::new("wasd_and_view_prefix");
    harness.run([
        Instruction::Raw(key('d')),
        Instruction::Raw(key('s')),
        Instruction::Raw(key('a')),
        Instruction::Raw(key('w')),
    ]);
    assert_eq!(harness.app.selected, date(2026, 6, 1));

    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('w'))]);
    assert_eq!(harness.app.view, ViewMode::Week);
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('m'))]);
    assert_eq!(harness.app.view, ViewMode::Month);
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('a'))]);
    assert!(matches!(harness.app.mode, crate::app::Mode::Agenda(_)));
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('w'))]);
    assert_eq!(harness.app.view, ViewMode::Week);
    assert!(matches!(harness.app.mode, crate::app::Mode::Normal));
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('a'))]);
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('m'))]);
    assert_eq!(harness.app.view, ViewMode::Month);
    assert!(matches!(harness.app.mode, crate::app::Mode::Normal));
    harness.run([Instruction::Raw(key('v')), Instruction::Raw(key('a'))]);
    harness.run([Instruction::Raw(key('n'))]);
    assert!(matches!(harness.app.mode, crate::app::Mode::Edit(_)));
}

fn add_timed(harness: &mut Harness, summary: &str, day: jiff::civil::Date, hour: i8) {
    let start = day
        .at(hour, 0, 0, 0)
        .to_zoned(harness.app.timezone().clone())
        .unwrap();
    let end = &start + 1.hour();
    harness.app.store.add_event(summary, &start, &end).unwrap();
}

fn selected_event(harness: &Harness) -> &CalendarEvent {
    let crate::app::Mode::Agenda(agenda) = &harness.app.mode else {
        panic!("expected agenda mode");
    };
    &harness.app.events[agenda.selected_item().unwrap().event_index]
}

#[test]
fn q_returns_to_the_default_view_from_every_mode() {
    let mut normal = Harness::new("back_normal");
    normal.run([
        Instruction::Normal(NormalAction::WeekView),
        Instruction::Raw(key('q')),
    ]);
    assert_eq!(normal.app.view, ViewMode::Month);
    assert!(matches!(normal.app.mode, crate::app::Mode::Normal));
    assert!(!normal.app.should_quit());
    normal.run([Instruction::Raw(key('q'))]);
    assert!(normal.app.should_quit());

    let mut help = Harness::new("back_help");
    help.run([
        Instruction::Normal(NormalAction::Help),
        Instruction::Raw(key('q')),
    ]);
    assert!(matches!(help.app.mode, crate::app::Mode::Normal));

    let mut editor = Harness::new("back_editor");
    editor.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Raw(key('q')),
    ]);
    assert!(matches!(editor.app.mode, crate::app::Mode::Normal));

    let mut agenda = Harness::new("back_agenda");
    agenda.run([
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Raw(key('q')),
    ]);
    assert!(matches!(agenda.app.mode, crate::app::Mode::Normal));

    let mut confirmation = Harness::new("back_confirmation");
    add_timed(&mut confirmation, "Disposable", date(2026, 6, 1), 10);
    confirmation.run([
        Instruction::Normal(NormalAction::Reload),
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Agenda(AgendaAction::Delete),
        Instruction::Raw(key('q')),
    ]);
    assert!(matches!(confirmation.app.mode, crate::app::Mode::Normal));
}

#[test]
fn escape_returns_to_the_default_view_from_every_mode() {
    let mut harness = Harness::new("escape_back");
    harness.run([
        Instruction::Normal(NormalAction::WeekView),
        Instruction::Normal(NormalAction::DefaultView),
    ]);
    assert_eq!(harness.app.view, ViewMode::Month);

    harness.run([
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Agenda(AgendaAction::Close),
    ]);
    assert!(matches!(harness.app.mode, crate::app::Mode::Normal));

    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Dialog(DialogAction::Cancel),
    ]);
    assert!(matches!(harness.app.mode, crate::app::Mode::Normal));

    harness.run([
        Instruction::Normal(NormalAction::Help),
        Instruction::Raw(escape()),
    ]);
    assert!(matches!(harness.app.mode, crate::app::Mode::Normal));
}

#[test]
fn agenda_is_centered_on_the_day_and_navigation_crosses_day_boundaries() {
    let mut harness = Harness::new("agenda_navigation");
    add_timed(&mut harness, "Previous day", date(2026, 5, 31), 9);
    add_timed(&mut harness, "Selected day", date(2026, 6, 1), 10);
    add_timed(&mut harness, "Following day", date(2026, 6, 2), 11);
    harness.run([
        Instruction::Normal(NormalAction::Reload),
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Snapshot("centered"),
    ]);

    assert_eq!(selected_event(&harness).summary, "Selected day");
    harness.run([Instruction::Raw(key('s'))]);
    assert_eq!(selected_event(&harness).summary, "Following day");
    harness.run([Instruction::Raw(key('w'))]);
    assert_eq!(selected_event(&harness).summary, "Selected day");
    harness.run([Instruction::Raw(key('d'))]);
    assert_eq!(selected_event(&harness).summary, "Following day");
    harness.run([Instruction::Raw(key('a'))]);
    assert_eq!(selected_event(&harness).summary, "Selected day");
    harness.run([Instruction::Agenda(AgendaAction::Navigate(Direction::Down))]);
    assert_eq!(selected_event(&harness).summary, "Following day");
    assert_eq!(harness.app.selected, date(2026, 6, 2));
    harness.run([
        Instruction::Agenda(AgendaAction::Navigate(Direction::Up)),
        Instruction::Agenda(AgendaAction::Navigate(Direction::Up)),
    ]);
    assert_eq!(selected_event(&harness).summary, "Previous day");
    assert_eq!(harness.app.selected, date(2026, 5, 31));
}

#[test]
fn agenda_delete_requires_confirmation_and_escape_cancels() {
    let mut harness = Harness::new("agenda_delete");
    add_timed(&mut harness, "Disposable", date(2026, 6, 1), 10);
    harness.run([
        Instruction::Normal(NormalAction::Reload),
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Agenda(AgendaAction::Delete),
        Instruction::Snapshot("confirmation"),
        Instruction::Confirm(ConfirmAction::Cancel),
    ]);
    assert_eq!(harness.app.events.len(), 1);

    harness.run([
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Agenda(AgendaAction::Delete),
        Instruction::Confirm(ConfirmAction::Confirm),
        Instruction::Snapshot("deleted"),
    ]);
    assert!(harness.app.events.is_empty());
    assert_eq!(
        std::fs::read_dir(harness.app.store.root().join("personal"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn agenda_edit_prefills_and_updates_the_existing_event() {
    let mut harness = Harness::new("agenda_edit");
    add_timed(&mut harness, "Original title", date(2026, 6, 1), 10);
    harness.run([
        Instruction::Normal(NormalAction::Reload),
        Instruction::Normal(NormalAction::OpenAgenda),
        Instruction::Agenda(AgendaAction::Edit),
        Instruction::Snapshot("prefilled"),
    ]);
    let crate::app::Mode::Edit(editor) = &harness.app.mode else {
        panic!("edit action should open editor");
    };
    assert_eq!(editor.description.value(), "Original title");
    assert_eq!(editor.start_time.value(), "10:00");
    assert_eq!(editor.duration.value(), "1h");

    harness.run([
        Instruction::Dialog(DialogAction::ClearField),
        Instruction::Type("Revised title"),
        Instruction::Dialog(DialogAction::Save),
        Instruction::Snapshot("saved"),
    ]);
    assert_eq!(harness.app.events.len(), 1);
    assert_eq!(harness.app.events[0].summary, "Revised title");
    assert!(matches!(harness.app.mode, crate::app::Mode::Agenda(_)));
}

#[test]
fn adding_an_event_uses_the_dialog_and_writes_an_ics_file() {
    let mut harness = Harness::new("adding_an_event");
    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Type("Project kickoff"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Dialog(DialogAction::ClearField),
        Instruction::Type("jun 7"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("13:00"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("1h30m"),
        Instruction::Snapshot("filled_dialog"),
        Instruction::Dialog(DialogAction::Save),
        Instruction::Snapshot("saved_four_week"),
        Instruction::Normal(NormalAction::WeekView),
        Instruction::Snapshot("saved_week"),
    ]);

    assert_eq!(harness.app.events.len(), 1);
    assert_eq!(harness.app.events[0].summary, "Project kickoff");
    assert_eq!(harness.app.events[0].start.hour(), 13);
    assert_eq!(harness.app.events[0].end.minute(), 30);
    let written = std::fs::read_dir(harness.app.store.root().join("personal"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(written.len(), 1);
    let body = std::fs::read_to_string(written[0].path()).unwrap();
    assert!(body.contains("SUMMARY:Project kickoff"));
    assert!(body.contains("DTSTART;TZID=UTC:20260607T130000"));
}

#[test]
fn duration_mode_defaults_only_the_selected_start_date() {
    let mut harness = Harness::new("duration_mode_defaults");
    harness.run([Instruction::Normal(NormalAction::AddEvent)]);

    let crate::app::Mode::Edit(editor) = &harness.app.mode else {
        panic!("add should open the editor");
    };
    assert_eq!(editor.edit_mode, crate::app::EditMode::Duration);
    assert_eq!(editor.start_date.value(), "2026-06-01");
    assert!(editor.start_time.value().is_empty());
    assert!(editor.duration.value().is_empty());
}

#[test]
fn empty_time_and_duration_create_an_all_day_event() {
    let mut harness = Harness::new("all_day_event");
    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Type("Conference"),
        Instruction::Dialog(DialogAction::Save),
        Instruction::Snapshot("saved"),
    ]);

    assert_eq!(harness.app.events.len(), 1);
    assert!(harness.app.events[0].all_day);
    let written = std::fs::read_dir(harness.app.store.root().join("personal"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let body = std::fs::read_to_string(written.path()).unwrap();
    assert!(body.contains("DTSTART;VALUE=DATE:20260601"));
    assert!(body.contains("DTEND;VALUE=DATE:20260602"));
}

#[test]
fn timed_event_requires_a_duration() {
    let mut harness = Harness::new("duration_required");
    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Type("Incomplete meeting"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("13:00"),
        Instruction::Dialog(DialogAction::Save),
    ]);

    let crate::app::Mode::Edit(editor) = &harness.app.mode else {
        panic!("invalid event should remain in the editor");
    };
    assert_eq!(
        editor.error.as_deref(),
        Some("Duration is required when start time is set")
    );
    assert!(harness.app.events.is_empty());
}

#[test]
fn exact_range_mode_accepts_start_and_end_datetimes() {
    let mut harness = Harness::new("exact_range_mode");
    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Type("Train journey"),
        Instruction::Dialog(DialogAction::ToggleMode),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("jun 8 22:30"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("jun 9 06:15"),
        Instruction::Snapshot("filled"),
        Instruction::Dialog(DialogAction::Save),
    ]);

    assert_eq!(harness.app.events.len(), 1);
    assert_eq!(harness.app.events[0].start.hour(), 22);
    assert_eq!(harness.app.events[0].end.hour(), 6);
    assert_eq!(harness.app.events[0].end.minute(), 15);
}

#[test]
fn typing_and_rendering_reuses_the_occurrence_cache() {
    let mut harness = Harness::new("cached_text_input");
    harness.run([
        Instruction::Normal(NormalAction::AddEvent),
        Instruction::Type("Existing event"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Dialog(DialogAction::ClearField),
        Instruction::Type("jun 7"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("13:00"),
        Instruction::Dialog(DialogAction::NextField),
        Instruction::Type("1h"),
        Instruction::Dialog(DialogAction::Save),
        Instruction::Normal(NormalAction::AddEvent),
    ]);
    crate::store::reset_occurrence_expansion_count();

    for character in "responsive typing".chars() {
        harness.run([Instruction::Raw(key(character))]);
        let _ = harness.render();
    }

    assert_eq!(crate::store::occurrence_expansion_count(), 0);
}

#[test]
fn all_configured_hotkeys_are_discoverable_in_the_app() {
    let mut harness = Harness::new("hotkey_help");
    harness.run([
        Instruction::Normal(NormalAction::Help),
        Instruction::Snapshot("all_bindings"),
    ]);
}

#[test]
fn semantic_navigation_matches_hjkl_direction() {
    let mut harness = Harness::new("semantic_navigation");
    harness.run([
        Instruction::Normal(NormalAction::Navigate(Direction::Left)),
        Instruction::Normal(NormalAction::Navigate(Direction::Down)),
    ]);
    assert_eq!(harness.app.selected, date(2026, 6, 7));
}

#[test]
fn a_reconfigured_navigation_key_is_used_immediately() {
    let mut harness = Harness::new("custom_navigation");
    let (_, sequence) = harness
        .app
        .config
        .hotkeys
        .normal
        .bindings
        .iter_mut()
        .find(|(action, _)| action == &NormalAction::Navigate(Direction::Right))
        .unwrap();
    *sequence = Sequence::parse("x").unwrap();

    harness.run([Instruction::Raw(key('x'))]);

    assert_eq!(harness.app.selected, date(2026, 6, 2));
    assert_eq!(
        harness
            .app
            .config
            .hotkeys
            .normal
            .key_for(&NormalAction::Navigate(Direction::Right))
            .as_deref(),
        Some("x")
    );
}
