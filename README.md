# calendar

A keyboard-first terminal calendar for a directory of `.ics` files (the
vdirsyncer/vdir storage layout). It reads the directory recursively, so both a
single folder full of events and one subdirectory per calendar work.

## Run

```console
cargo run --release
```

By default events are read from `~/.local/share/dav/calendar`. Override that
without changing configuration with:

```console
cargo run --release -- --calendar-dir /path/to/calendars
```

The first run creates `~/.config/calendar/config.yml`. Press `?` in the app to
see the active bindings. The footer and add dialog also use the configured
keys rather than hard-coded labels.

The main defaults are:

- `h/j/k/l`: move one day / week
- `H/L`: previous / next four-week period (or week)
- `m/w`: four-week / week view
- `a`: add an appointment
- `g t`: return to today; pressing `g` shows the available continuation
- `?`: show every normal and dialog binding
- `q`: quit

All of those actions and all dialog actions are configurable under `hotkeys`
in the YAML file. A key may be a single key (`a`, `Enter`, `C-u`) or a sequence
(`g t`).

## Adding appointments

The default add mode contains description, start date, start time, and duration
fields. The date starts on the selected calendar day; time and duration start
empty. Leave both empty for an all-day event, or enter durations such as `45m`,
`1h30m`, or `1:30`.

Press `C-t` to switch to exact-range mode, which has combined start and end
date/time fields. `C-t`, like the other dialog keys, is configurable. Dates
accept ISO dates and friendly forms including:

```text
today 09:30
tomorrow 2pm
jun 7 13:00
7 jun 2027 5:30pm
sun 5pm
+3d
```

A time by itself in the exact end field uses the start date; if it is earlier
than the start time, it means the following day. Each saved appointment is a
new, standard VCALENDAR file, ready for the CalDAV sync tool to upload. New
appointments include display reminders one day and one hour before their start.

Existing timed and all-day events may use local, UTC, or named IANA timezones.
Common daily, weekly, monthly, and yearly `RRULE`s are expanded in the views,
including `COUNT`, `UNTIL`, `BYDAY`, `BYMONTHDAY`, `BYMONTH`, `EXDATE`, and
rescheduled recurrence instances.

`default_calendar` controls where new files go. `default` writes directly in
`calendar_dir`; another value such as `personal` writes to
`calendar_dir/personal`. Calendars get stable distinct colors. Override them
with names (`cyan`, `magenta`, etc.) or hex colors:

```yaml
calendar_colors:
  personal: cyan
  work: "#ff9f43"
```

## Tests

```console
cargo test
```

The TUI tests drive configured key sequences and dialog actions against a
temporary `.ics` directory, render with ratatui's test backend, and snapshot
the resulting calendar. They cover navigation, both views, hotkey discovery,
and creating and reloading an appointment.
