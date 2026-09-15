use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use eyre::{Context, Result, eyre};
use jiff::{
    Timestamp, ToSpan, Unit, Zoned,
    civil::{Date, DateTime, Weekday},
    tz::TimeZone,
};
use uuid::Uuid;

#[cfg(test)]
std::thread_local! {
    static OCCURRENCE_EXPANSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_occurrence_expansion_count() {
    OCCURRENCE_EXPANSIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn occurrence_expansion_count() -> usize {
    OCCURRENCE_EXPANSIONS.with(std::cell::Cell::get)
}

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub start: Zoned,
    pub end: Zoned,
    pub all_day: bool,
    pub calendar: String,
    pub source: PathBuf,
    /// The original VEVENT properties, retained for the agenda detail view.
    pub metadata: Vec<EventMetadata>,
    recurrence: Option<Recurrence>,
    exclusions: Vec<Timestamp>,
    recurrence_id: Option<Timestamp>,
    cancelled: bool,
}

#[derive(Debug, Clone)]
pub struct EventMetadata {
    pub name: String,
    pub parameters: BTreeMap<String, String>,
    pub value: String,
}

impl CalendarEvent {
    pub fn is_recurring(&self) -> bool {
        self.recurrence.is_some() || self.recurrence_id.is_some()
    }

    pub fn representative_occurrence<'a>(
        &'a self,
        today: Date,
        display_timezone: &TimeZone,
    ) -> EventOccurrence<'a> {
        if self.recurrence.is_some() {
            for offset in 0..=732 {
                if let Some(occurrence) = self
                    .occurrences_on(today + offset.days(), display_timezone)
                    .into_iter()
                    .next()
                {
                    return occurrence;
                }
            }
        }
        EventOccurrence {
            event: self,
            start: self.start.clone(),
            end: self.end.clone(),
        }
    }

    pub fn occurs_on(&self, date: Date, display_timezone: &TimeZone) -> bool {
        let day_start = date
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone.clone())
            .expect("a display date must map to its timezone");
        let day_end = (date + 1.day())
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone.clone())
            .expect("a display date must map to its timezone");
        self.start.timestamp() < day_end.timestamp() && self.end.timestamp() > day_start.timestamp()
    }

    pub fn occurrences_on<'a>(
        &'a self,
        date: Date,
        display_timezone: &TimeZone,
    ) -> Vec<EventOccurrence<'a>> {
        #[cfg(test)]
        OCCURRENCE_EXPANSIONS.with(|count| count.set(count.get() + 1));

        let Some(rule) = &self.recurrence else {
            return self
                .occurs_on(date, display_timezone)
                .then(|| EventOccurrence {
                    event: self,
                    start: self.start.clone(),
                    end: self.end.clone(),
                })
                .into_iter()
                .collect();
        };

        let day_start = date
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone.clone())
            .expect("a display date must map to its timezone");
        let event_timezone = self.start.time_zone().clone();
        let nearby = day_start.timestamp().to_zoned(event_timezone).date();
        let mut occurrences = Vec::new();
        // A local date can differ by one day between timezones. The extra day
        // also catches an event continuing from the previous date.
        let duration_seconds =
            (self.end.timestamp().as_second() - self.start.timestamp().as_second()).max(0);
        let lookback_days = ((duration_seconds + 86_399) / 86_400 + 1).max(2);
        for offset in -lookback_days..=2 {
            let candidate = nearby + offset.days();
            if !rule.matches(candidate, self) {
                continue;
            }
            let start = match candidate
                .at(
                    self.start.hour(),
                    self.start.minute(),
                    self.start.second(),
                    self.start.subsec_nanosecond(),
                )
                .to_zoned(self.start.time_zone().clone())
            {
                Ok(start) => start,
                Err(_) => continue,
            };
            if self.exclusions.contains(&start.timestamp()) {
                continue;
            }
            let end = if self.all_day {
                let duration_days = self
                    .start
                    .date()
                    .until((Unit::Day, self.end.date()))
                    .map(|span| span.get_days())
                    .unwrap_or(1)
                    .max(1);
                (candidate + i64::from(duration_days).days())
                    .at(0, 0, 0, 0)
                    .to_zoned(self.start.time_zone().clone())
                    .expect("all-day recurrence end must be valid")
            } else {
                let duration = self.end.timestamp() - self.start.timestamp();
                (start.timestamp() + duration).to_zoned(self.start.time_zone().clone())
            };
            let occurrence = EventOccurrence {
                event: self,
                start,
                end,
            };
            if occurrence.occurs_on(date, display_timezone) {
                occurrences.push(occurrence);
            }
        }
        occurrences.sort_by_key(|occurrence| occurrence.start.timestamp());
        occurrences.dedup_by_key(|occurrence| occurrence.start.timestamp());
        occurrences
    }
}

#[derive(Debug, Clone)]
pub enum EventTiming {
    Timed { start: Zoned, end: Zoned },
    AllDay { date: Date },
}

#[derive(Debug, Clone)]
pub struct EventOccurrence<'a> {
    pub event: &'a CalendarEvent,
    pub start: Zoned,
    pub end: Zoned,
}

impl EventOccurrence<'_> {
    pub fn occurs_on(&self, date: Date, display_timezone: &TimeZone) -> bool {
        let day_start = date
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone.clone())
            .expect("a display date must map to its timezone");
        let day_end = (date + 1.day())
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone.clone())
            .expect("a display date must map to its timezone");
        self.start.timestamp() < day_end.timestamp() && self.end.timestamp() > day_start.timestamp()
    }
}

impl std::ops::Deref for EventOccurrence<'_> {
    type Target = CalendarEvent;

    fn deref(&self) -> &Self::Target {
        self.event
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, Copy)]
struct ByDay {
    ordinal: Option<i8>,
    weekday: Weekday,
}

#[derive(Debug, Clone)]
struct Recurrence {
    frequency: Frequency,
    interval: i64,
    count: Option<u32>,
    until: Option<Until>,
    by_days: Vec<ByDay>,
    by_month_days: Vec<i8>,
    by_months: Vec<i8>,
    by_set_positions: Vec<i16>,
}

#[derive(Debug, Clone)]
enum Until {
    Date(Date),
    Moment(Timestamp),
}

impl Recurrence {
    fn matches(&self, date: Date, event: &CalendarEvent) -> bool {
        let master = event.start.date();
        if date < master || !self.matches_without_count(date, master) {
            return false;
        }
        let candidate = match date
            .at(
                event.start.hour(),
                event.start.minute(),
                event.start.second(),
                event.start.subsec_nanosecond(),
            )
            .to_zoned(event.start.time_zone().clone())
        {
            Ok(candidate) => candidate,
            Err(_) => return false,
        };
        if let Some(until) = &self.until {
            let after_until = match until {
                Until::Date(until) => date > *until,
                Until::Moment(until) => candidate.timestamp() > *until,
            };
            if after_until {
                return false;
            }
        }
        if let Some(count) = self.count {
            let mut cursor = master;
            let mut ordinal = 0_u32;
            while cursor <= date && ordinal < count {
                if self.matches_without_count(cursor, master) {
                    ordinal += 1;
                    if cursor == date {
                        return ordinal <= count;
                    }
                }
                cursor += 1.day();
            }
            return false;
        }
        true
    }

    fn matches_without_count(&self, date: Date, master: Date) -> bool {
        if !self.by_months.is_empty() && !self.by_months.contains(&date.month()) {
            return false;
        }
        if !self.by_month_days.is_empty()
            && !self.by_month_days.iter().any(|day| {
                (*day > 0 && date.day() == *day)
                    || (*day < 0 && date.day() == date.days_in_month() + 1 + *day)
            })
        {
            return false;
        }
        if !self.by_days.is_empty() && !self.by_days.iter().any(|by_day| by_day.matches(date)) {
            return false;
        }

        let frequency_matches = match self.frequency {
            Frequency::Daily => {
                days_between(master, date).is_some_and(|days| days % self.interval == 0)
            }
            Frequency::Weekly => {
                let weekdays_match = if self.by_days.is_empty() {
                    date.weekday() == master.weekday()
                } else {
                    true
                };
                weekdays_match
                    && days_between(monday_of(master), monday_of(date))
                        .is_some_and(|days| (days / 7) % self.interval == 0)
            }
            Frequency::Monthly => {
                let month_delta = i64::from(date.year() - master.year()) * 12
                    + i64::from(date.month() - master.month());
                let default_day_matches = !self.by_month_days.is_empty()
                    || !self.by_days.is_empty()
                    || date.day() == master.day();
                month_delta >= 0 && month_delta % self.interval == 0 && default_day_matches
            }
            Frequency::Yearly => {
                let year_delta = i64::from(date.year() - master.year());
                let default_month = !self.by_months.is_empty() || date.month() == master.month();
                let default_day = !self.by_month_days.is_empty()
                    || !self.by_days.is_empty()
                    || date.day() == master.day();
                year_delta >= 0 && year_delta % self.interval == 0 && default_month && default_day
            }
        };
        frequency_matches && self.matches_set_position(date, master)
    }

    fn matches_set_position(&self, date: Date, master: Date) -> bool {
        if self.by_set_positions.is_empty() {
            return true;
        }
        let candidates = match self.frequency {
            Frequency::Monthly => (1..=date.days_in_month())
                .filter_map(|day| Date::new(date.year(), date.month(), day).ok())
                .filter(|candidate| self.matches_filters(*candidate))
                .collect::<Vec<_>>(),
            // BYSETPOS is overwhelmingly used with monthly rules. Keep other
            // frequencies conservative until their complete period semantics
            // (including WKST) are implemented.
            _ => return true,
        };
        self.by_set_positions.iter().any(|position| {
            let index = if *position > 0 {
                usize::try_from(*position - 1).ok()
            } else {
                usize::try_from(candidates.len() as i64 + i64::from(*position)).ok()
            };
            index
                .and_then(|index| candidates.get(index))
                .is_some_and(|candidate| *candidate == date)
        }) && date >= master
    }

    fn matches_filters(&self, date: Date) -> bool {
        (self.by_months.is_empty() || self.by_months.contains(&date.month()))
            && (self.by_month_days.is_empty()
                || self.by_month_days.iter().any(|day| {
                    (*day > 0 && date.day() == *day)
                        || (*day < 0 && date.day() == date.days_in_month() + 1 + *day)
                }))
            && (self.by_days.is_empty() || self.by_days.iter().any(|by_day| by_day.matches(date)))
    }
}

impl ByDay {
    fn matches(self, date: Date) -> bool {
        if date.weekday() != self.weekday {
            return false;
        }
        match self.ordinal {
            None => true,
            Some(ordinal) if ordinal > 0 => (date.day() - 1) / 7 + 1 == ordinal,
            Some(ordinal) => {
                let from_end = (date.days_in_month() - date.day()) / 7 + 1;
                from_end == -ordinal
            }
        }
    }
}

fn days_between(start: Date, end: Date) -> Option<i64> {
    start
        .until((Unit::Day, end))
        .ok()
        .map(|span| i64::from(span.get_days()))
}

fn monday_of(date: Date) -> Date {
    date - i64::from(date.weekday().to_monday_zero_offset()).days()
}

#[derive(Debug, Default)]
pub struct LoadedCalendar {
    pub events: Vec<CalendarEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CalendarStore {
    root: PathBuf,
    default_calendar: String,
    display_timezone: TimeZone,
}

impl CalendarStore {
    pub fn new(root: PathBuf, default_calendar: String) -> Self {
        Self {
            root,
            default_calendar,
            display_timezone: TimeZone::system(),
        }
    }

    #[cfg(test)]
    pub fn with_timezone(
        root: PathBuf,
        default_calendar: String,
        display_timezone: TimeZone,
    ) -> Self {
        Self {
            root,
            default_calendar,
            display_timezone,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn display_timezone(&self) -> &TimeZone {
        &self.display_timezone
    }

    pub fn load(&self) -> Result<LoadedCalendar> {
        fs::create_dir_all(&self.root)
            .wrap_err_with(|| format!("creating calendar directory {}", self.root.display()))?;
        let mut paths = Vec::new();
        collect_ics_files(&self.root, &mut paths)?;
        paths.sort();

        let mut loaded = LoadedCalendar::default();
        for path in paths {
            let input = match fs::read_to_string(&path) {
                Ok(input) => input,
                Err(error) => {
                    loaded.warnings.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            match parse_ics(&input, &path, &self.root, self.display_timezone.clone()) {
                Ok(mut events) => loaded.events.append(&mut events),
                Err(error) => loaded.warnings.push(format!("{}: {error}", path.display())),
            }
        }
        loaded.events.sort_by(|left, right| {
            left.start
                .timestamp()
                .cmp(&right.start.timestamp())
                .then_with(|| left.summary.cmp(&right.summary))
        });
        let overrides = loaded
            .events
            .iter()
            .filter_map(|event| {
                event
                    .recurrence_id
                    .map(|recurrence_id| (event.uid.clone(), recurrence_id))
            })
            .collect::<Vec<_>>();
        for event in &mut loaded.events {
            if event.recurrence.is_some() {
                event.exclusions.extend(
                    overrides
                        .iter()
                        .filter(|(uid, _)| uid == &event.uid)
                        .map(|(_, recurrence_id)| *recurrence_id),
                );
            }
        }
        loaded.events.retain(|event| !event.cancelled);
        Ok(loaded)
    }

    pub fn add_event(&self, summary: &str, start: &Zoned, end: &Zoned) -> Result<PathBuf> {
        if summary.trim().is_empty() {
            return Err(eyre!("description cannot be empty"));
        }
        if end.timestamp() <= start.timestamp() {
            return Err(eyre!("end must be after start"));
        }
        self.write_event(
            summary,
            format_datetime_property("DTSTART", start),
            format_datetime_property("DTEND", end),
        )
    }

    pub fn add_all_day_event(&self, summary: &str, date: Date) -> Result<PathBuf> {
        if summary.trim().is_empty() {
            return Err(eyre!("description cannot be empty"));
        }
        self.write_event(
            summary,
            format!("DTSTART;VALUE=DATE:{}", date.strftime("%Y%m%d")),
            format!("DTEND;VALUE=DATE:{}", (date + 1.day()).strftime("%Y%m%d")),
        )
    }

    pub fn delete_occurrence(&self, event: &CalendarEvent, occurrence_start: &Zoned) -> Result<()> {
        let path = self.checked_source(event)?;
        let mut lines = unfold(&fs::read_to_string(&path)?);
        let range = self
            .find_event_range(&lines, event)?
            .ok_or_else(|| eyre!("event {} is no longer present", event.uid))?;

        if event.recurrence.is_some() {
            let exception = format_occurrence_property("EXDATE", occurrence_start, event.all_day);
            lines.insert(range.end - 1, exception);
            self.rewrite_calendar(&path, &lines)
        } else if event.recurrence_id.is_some() {
            replace_event_properties(
                &mut lines,
                range,
                vec!["STATUS"],
                vec!["STATUS:CANCELLED".to_string()],
            );
            self.rewrite_calendar(&path, &lines)
        } else {
            let event_count = lines
                .iter()
                .filter(|line| line.eq_ignore_ascii_case("BEGIN:VEVENT"))
                .count();
            if event_count == 1 {
                fs::remove_file(&path)
                    .wrap_err_with(|| format!("deleting event {}", path.display()))
            } else {
                lines.drain(range);
                self.rewrite_calendar(&path, &lines)
            }
        }
    }

    pub fn update_occurrence(
        &self,
        event: &CalendarEvent,
        occurrence_start: &Zoned,
        summary: &str,
        timing: EventTiming,
    ) -> Result<()> {
        if summary.trim().is_empty() {
            return Err(eyre!("description cannot be empty"));
        }
        let path = self.checked_source(event)?;
        let mut lines = unfold(&fs::read_to_string(&path)?);
        if event.recurrence.is_some() {
            let insertion = lines
                .iter()
                .rposition(|line| line.eq_ignore_ascii_case("END:VCALENDAR"))
                .ok_or_else(|| eyre!("{} has no END:VCALENDAR", path.display()))?;
            let mut component = vec![
                "BEGIN:VEVENT".to_string(),
                format!("UID:{}", escape_text(&event.uid)),
                format_occurrence_property("RECURRENCE-ID", occurrence_start, event.all_day),
            ];
            component.extend(editable_event_properties(summary, timing)?);
            component.push("END:VEVENT".to_string());
            lines.splice(insertion..insertion, component);
        } else {
            let range = self
                .find_event_range(&lines, event)?
                .ok_or_else(|| eyre!("event {} is no longer present", event.uid))?;
            replace_event_properties(
                &mut lines,
                range,
                vec![
                    "DTSTART",
                    "DTEND",
                    "DURATION",
                    "SUMMARY",
                    "DTSTAMP",
                    "LAST-MODIFIED",
                ],
                editable_event_properties(summary, timing)?,
            );
        }
        self.rewrite_calendar(&path, &lines)
    }

    fn checked_source(&self, event: &CalendarEvent) -> Result<PathBuf> {
        let root = self
            .root
            .canonicalize()
            .wrap_err_with(|| format!("resolving calendar root {}", self.root.display()))?;
        let source = event
            .source
            .canonicalize()
            .wrap_err_with(|| format!("resolving event file {}", event.source.display()))?;
        if !source.starts_with(&root) {
            return Err(eyre!(
                "refusing to modify event outside calendar root: {}",
                source.display()
            ));
        }
        Ok(source)
    }

    fn find_event_range(
        &self,
        lines: &[String],
        event: &CalendarEvent,
    ) -> Result<Option<std::ops::Range<usize>>> {
        let mut start = None;
        for (index, line) in lines.iter().enumerate() {
            if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
                start = Some(index);
            } else if line.eq_ignore_ascii_case("END:VEVENT")
                && let Some(begin) = start.take()
            {
                let properties = lines[begin + 1..index]
                    .iter()
                    .filter_map(|line| parse_property(line))
                    .collect::<Vec<_>>();
                if property_value(&properties, "UID")
                    .map(unescape_text)
                    .as_deref()
                    != Some(event.uid.as_str())
                {
                    continue;
                }
                let recurrence_id = property(&properties, "RECURRENCE-ID")
                    .map(|property| parse_moment(property, self.display_timezone.clone()))
                    .transpose()?
                    .map(|moment| moment.zoned.timestamp());
                if recurrence_id == event.recurrence_id {
                    return Ok(Some(begin..index + 1));
                }
            }
        }
        Ok(None)
    }

    fn rewrite_calendar(&self, path: &Path, lines: &[String]) -> Result<()> {
        let body = lines
            .iter()
            .filter(|line| !line.is_empty())
            .map(|line| fold_content_line(line))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\n";
        let parent = path
            .parent()
            .ok_or_else(|| eyre!("event file has no parent: {}", path.display()))?;
        let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
        fs::write(&temporary, body)
            .wrap_err_with(|| format!("writing temporary event {}", temporary.display()))?;
        fs::rename(&temporary, path)
            .wrap_err_with(|| format!("committing event {}", path.display()))
    }

    fn write_event(&self, summary: &str, start: String, end: String) -> Result<PathBuf> {
        let target = if self.default_calendar == "default" || self.default_calendar.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&self.default_calendar)
        };
        fs::create_dir_all(&target)
            .wrap_err_with(|| format!("creating calendar {}", target.display()))?;

        let uid = format!("{}@calendar", Uuid::new_v4());
        let filename = format!("{}.ics", uid.trim_end_matches("@calendar"));
        let path = target.join(filename);
        let now = jiff::Timestamp::now().to_zoned(TimeZone::UTC);
        let mut lines = vec![
            "BEGIN:VCALENDAR".to_string(),
            "VERSION:2.0".to_string(),
            "PRODID:-//calendar-tui//EN".to_string(),
            "CALSCALE:GREGORIAN".to_string(),
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}", escape_text(&uid)),
            format!("DTSTAMP:{}Z", now.strftime("%Y%m%dT%H%M%S")),
            start,
            end,
            format!("SUMMARY:{}", escape_text(summary.trim())),
        ];
        lines.extend(reminder_components(summary));
        lines.extend(["END:VEVENT".to_string(), "END:VCALENDAR".to_string()]);
        let body = lines
            .iter()
            .map(|line| fold_content_line(line))
            .collect::<Vec<_>>()
            .join("\r\n")
            + "\r\n";
        let temporary = target.join(format!(".{}.tmp", Uuid::new_v4()));
        fs::write(&temporary, body)
            .wrap_err_with(|| format!("writing temporary event {}", temporary.display()))?;
        fs::rename(&temporary, &path)
            .wrap_err_with(|| format!("committing event {}", path.display()))?;
        Ok(path)
    }
}

fn reminder_components(summary: &str) -> Vec<String> {
    ["-P1D", "-PT1H"]
        .into_iter()
        .flat_map(|trigger| {
            [
                "BEGIN:VALARM".to_string(),
                "ACTION:DISPLAY".to_string(),
                format!("DESCRIPTION:{}", escape_text(summary.trim())),
                format!("TRIGGER:{trigger}"),
                "END:VALARM".to_string(),
            ]
        })
        .collect()
}

fn editable_event_properties(summary: &str, timing: EventTiming) -> Result<Vec<String>> {
    let (start, end) = match timing {
        EventTiming::Timed { start, end } => {
            if end.timestamp() <= start.timestamp() {
                return Err(eyre!("end must be after start"));
            }
            (
                format_datetime_property("DTSTART", &start),
                format_datetime_property("DTEND", &end),
            )
        }
        EventTiming::AllDay { date } => (
            format!("DTSTART;VALUE=DATE:{}", date.strftime("%Y%m%d")),
            format!("DTEND;VALUE=DATE:{}", (date + 1.day()).strftime("%Y%m%d")),
        ),
    };
    let now = Timestamp::now().to_zoned(TimeZone::UTC);
    Ok(vec![
        format!("DTSTAMP:{}Z", now.strftime("%Y%m%dT%H%M%S")),
        start,
        end,
        format!("SUMMARY:{}", escape_text(summary.trim())),
    ])
}

fn format_occurrence_property(name: &str, zoned: &Zoned, all_day: bool) -> String {
    if all_day {
        format!("{name};VALUE=DATE:{}", zoned.strftime("%Y%m%d"))
    } else {
        format_datetime_property(name, zoned)
    }
}

fn replace_event_properties(
    lines: &mut Vec<String>,
    range: std::ops::Range<usize>,
    removed_names: Vec<&str>,
    replacements: Vec<String>,
) {
    let end = range.end - 1;
    let replacement_set = removed_names
        .into_iter()
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>();
    let mut body = lines[range.start + 1..end]
        .iter()
        .filter(|line| {
            parse_property(line).is_none_or(|property| !replacement_set.contains(&property.name))
        })
        .cloned()
        .collect::<Vec<_>>();
    body.extend(replacements);
    let mut component = vec!["BEGIN:VEVENT".to_string()];
    component.extend(body);
    component.push("END:VEVENT".to_string());
    lines.splice(range, component);
}

fn collect_ics_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .wrap_err_with(|| format!("reading calendar directory {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_ics_files(&path, output)?;
        } else if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ics"))
        {
            output.push(path);
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Property {
    name: String,
    params: BTreeMap<String, String>,
    value: String,
}

fn parse_ics(
    input: &str,
    source: &Path,
    root: &Path,
    display_timezone: TimeZone,
) -> Result<Vec<CalendarEvent>> {
    let lines = unfold(input);
    let properties = lines
        .iter()
        .filter_map(|line| parse_property(line))
        .collect::<Vec<_>>();
    let calendar_title = properties
        .iter()
        .find(|property| property.name == "X-WR-CALNAME")
        .map(|property| unescape_text(&property.value));
    let calendar = calendar_name(source, root, calendar_title);

    let mut events = Vec::new();
    let mut in_event = false;
    let mut nested_components = 0;
    let mut current = Vec::new();
    for property in properties {
        match (property.name.as_str(), property.value.as_str()) {
            ("BEGIN", "VEVENT") if !in_event => {
                in_event = true;
                nested_components = 0;
                current.clear();
            }
            ("BEGIN", _) if in_event => nested_components += 1,
            ("END", "VEVENT") if in_event && nested_components == 0 => {
                if let Some(event) =
                    parse_event(&current, source, &calendar, display_timezone.clone())?
                {
                    events.push(event);
                }
                in_event = false;
                current.clear();
            }
            ("END", _) if in_event && nested_components > 0 => nested_components -= 1,
            _ if in_event && nested_components == 0 => current.push(property),
            _ => {}
        }
    }
    Ok(events)
}

fn parse_event(
    properties: &[Property],
    source: &Path,
    calendar: &str,
    display_timezone: TimeZone,
) -> Result<Option<CalendarEvent>> {
    let cancelled =
        property_value(properties, "STATUS").is_some_and(|status| status == "CANCELLED");
    let start_property =
        property(properties, "DTSTART").ok_or_else(|| eyre!("VEVENT has no DTSTART"))?;
    let start = parse_moment(start_property, display_timezone.clone())?;
    let end = if let Some(property) = property(properties, "DTEND") {
        parse_moment(property, display_timezone.clone())?.zoned
    } else if let Some(duration) = property_value(properties, "DURATION") {
        let span = duration
            .parse::<jiff::Span>()
            .map_err(|error| eyre!("invalid DURATION {duration:?}: {error}"))?;
        &start.zoned + span
    } else if start.all_day {
        &start.zoned + 1.day()
    } else {
        &start.zoned + 1.hour()
    };
    let uid = property_value(properties, "UID")
        .map(unescape_text)
        .unwrap_or_else(|| source.display().to_string());
    let summary = property_value(properties, "SUMMARY")
        .map(unescape_text)
        .unwrap_or_else(|| "(untitled)".to_string());
    let recurrence = property(properties, "RRULE")
        .map(|property| parse_recurrence(property, display_timezone.clone()))
        .transpose()?;
    let exclusions = properties
        .iter()
        .filter(|property| property.name == "EXDATE")
        .flat_map(|property| {
            property.value.split(',').filter_map(|value| {
                let mut item = property.clone();
                item.value = value.to_string();
                parse_moment(&item, display_timezone.clone())
                    .ok()
                    .map(|moment| moment.zoned.timestamp())
            })
        })
        .collect();
    let recurrence_id = property(properties, "RECURRENCE-ID")
        .map(|property| parse_moment(property, display_timezone.clone()))
        .transpose()?
        .map(|moment| moment.zoned.timestamp());
    Ok(Some(CalendarEvent {
        uid,
        summary,
        start: start.zoned,
        end,
        all_day: start.all_day,
        calendar: calendar.to_string(),
        source: source.to_path_buf(),
        metadata: properties
            .iter()
            .map(|property| EventMetadata {
                name: property.name.clone(),
                parameters: property.params.clone(),
                value: unescape_text(&property.value),
            })
            .collect(),
        recurrence,
        exclusions,
        recurrence_id,
        cancelled,
    }))
}

fn parse_recurrence(property: &Property, display_timezone: TimeZone) -> Result<Recurrence> {
    let parts = property
        .value
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.to_ascii_uppercase(), value))
        .collect::<BTreeMap<_, _>>();
    let frequency = match parts.get("FREQ").copied() {
        Some("DAILY") => Frequency::Daily,
        Some("WEEKLY") => Frequency::Weekly,
        Some("MONTHLY") => Frequency::Monthly,
        Some("YEARLY") => Frequency::Yearly,
        Some(other) => return Err(eyre!("unsupported RRULE frequency {other:?}")),
        None => return Err(eyre!("RRULE has no FREQ")),
    };
    let interval = parts
        .get("INTERVAL")
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(1)
        .max(1);
    let count = parts
        .get("COUNT")
        .map(|value| value.parse::<u32>())
        .transpose()?;
    let until = parts
        .get("UNTIL")
        .map(|value| parse_until(value, display_timezone))
        .transpose()?;
    let by_days = parts
        .get("BYDAY")
        .map(|value| {
            value
                .split(',')
                .map(parse_by_day)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let by_month_days = parse_number_list(parts.get("BYMONTHDAY").copied())?;
    let by_months = parse_number_list(parts.get("BYMONTH").copied())?;
    let by_set_positions = parts
        .get("BYSETPOS")
        .map(|value| {
            value
                .split(',')
                .map(str::parse)
                .collect::<Result<Vec<i16>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Recurrence {
        frequency,
        interval,
        count,
        until,
        by_days,
        by_month_days,
        by_months,
        by_set_positions,
    })
}

fn parse_until(value: &str, display_timezone: TimeZone) -> Result<Until> {
    if !value.contains('T') {
        return Ok(Until::Date(parse_compact_date(value)?));
    }
    let property = Property {
        name: "UNTIL".to_string(),
        params: BTreeMap::new(),
        value: value.to_string(),
    };
    Ok(Until::Moment(
        parse_moment(&property, display_timezone)?.zoned.timestamp(),
    ))
}

fn parse_by_day(value: &str) -> Result<ByDay> {
    if value.len() < 2 {
        return Err(eyre!("invalid BYDAY value {value:?}"));
    }
    let (ordinal, weekday) = value.split_at(value.len() - 2);
    let weekday = match weekday {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return Err(eyre!("invalid BYDAY weekday {weekday:?}")),
    };
    let ordinal = if ordinal.is_empty() {
        None
    } else {
        Some(ordinal.parse::<i8>()?)
    };
    Ok(ByDay { ordinal, weekday })
}

fn parse_number_list(value: Option<&str>) -> Result<Vec<i8>> {
    value
        .map(|value| {
            value
                .split(',')
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(Into::into)
}

struct ParsedMoment {
    zoned: Zoned,
    all_day: bool,
}

fn parse_moment(property: &Property, display_timezone: TimeZone) -> Result<ParsedMoment> {
    let value = property.value.trim();
    let is_date = property
        .params
        .get("VALUE")
        .is_some_and(|value| value.eq_ignore_ascii_case("DATE"))
        || !value.contains('T');
    if is_date {
        let date = parse_compact_date(value)?;
        let zoned = date
            .at(0, 0, 0, 0)
            .to_zoned(display_timezone)
            .map_err(|error| eyre!(error))?;
        return Ok(ParsedMoment {
            zoned,
            all_day: true,
        });
    }

    let utc = value.ends_with('Z');
    let value = value.trim_end_matches('Z');
    let datetime = parse_compact_datetime(value)?;
    let timezone = if utc {
        TimeZone::UTC
    } else if let Some(timezone) = property.params.get("TZID") {
        let normalized = timezone
            .rsplit("/Tzfile/")
            .next()
            .unwrap_or(timezone)
            .trim_matches('"');
        TimeZone::get(normalized).unwrap_or(display_timezone)
    } else {
        display_timezone
    };
    let zoned = datetime.to_zoned(timezone).map_err(|error| eyre!(error))?;
    Ok(ParsedMoment {
        zoned,
        all_day: false,
    })
}

fn parse_compact_date(value: &str) -> Result<Date> {
    if value.len() != 8 {
        return Err(eyre!("invalid iCalendar date {value:?}"));
    }
    Date::new(
        value[0..4].parse()?,
        value[4..6].parse()?,
        value[6..8].parse()?,
    )
    .map_err(|error| eyre!(error))
}

fn parse_compact_datetime(value: &str) -> Result<DateTime> {
    if value.len() != 13 && value.len() != 15 {
        return Err(eyre!("invalid iCalendar datetime {value:?}"));
    }
    let second = if value.len() == 15 {
        value[13..15].parse()?
    } else {
        0
    };
    DateTime::new(
        value[0..4].parse()?,
        value[4..6].parse()?,
        value[6..8].parse()?,
        value[9..11].parse()?,
        value[11..13].parse()?,
        second,
        0,
    )
    .map_err(|error| eyre!(error))
}

fn unfold(input: &str) -> Vec<String> {
    let mut output: Vec<String> = Vec::new();
    for line in input.replace("\r\n", "\n").split('\n') {
        if (line.starts_with(' ') || line.starts_with('\t')) && !output.is_empty() {
            output.last_mut().unwrap().push_str(&line[1..]);
        } else {
            output.push(line.trim_end_matches('\r').to_string());
        }
    }
    output
}

fn parse_property(line: &str) -> Option<Property> {
    let (head, value) = line.split_once(':')?;
    let mut head_parts = head.split(';');
    let name = head_parts.next()?.to_ascii_uppercase();
    let params = head_parts
        .filter_map(|parameter| {
            let (key, value) = parameter.split_once('=')?;
            Some((key.to_ascii_uppercase(), value.to_string()))
        })
        .collect();
    Some(Property {
        name,
        params,
        value: value.to_string(),
    })
}

fn property<'a>(properties: &'a [Property], name: &str) -> Option<&'a Property> {
    properties.iter().find(|property| property.name == name)
}

fn property_value<'a>(properties: &'a [Property], name: &str) -> Option<&'a str> {
    property(properties, name).map(|property| property.value.as_str())
}

fn calendar_name(source: &Path, root: &Path, title: Option<String>) -> String {
    let relative = source.strip_prefix(root).unwrap_or(source);
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    if let Some(Component::Normal(name)) = parent.components().next() {
        return name.to_string_lossy().into_owned();
    }
    title
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

fn format_datetime_property(name: &str, zoned: &Zoned) -> String {
    if let Some(timezone) = zoned.time_zone().iana_name() {
        format!("{name};TZID={timezone}:{}", zoned.strftime("%Y%m%dT%H%M%S"))
    } else {
        let utc = zoned.timestamp().to_zoned(TimeZone::UTC);
        format!("{name}:{}Z", utc.strftime("%Y%m%dT%H%M%S"))
    }
}

fn fold_content_line(line: &str) -> String {
    const FIRST_LIMIT: usize = 75;
    const CONTINUATION_LIMIT: usize = 74;
    if line.len() <= FIRST_LIMIT {
        return line.to_string();
    }
    let mut output = String::new();
    let mut rest = line;
    let mut limit = FIRST_LIMIT;
    while !rest.is_empty() {
        let mut split = rest.len().min(limit);
        while split > 0 && !rest.is_char_boundary(split) {
            split -= 1;
        }
        if split == 0 {
            split = rest
                .char_indices()
                .nth(1)
                .map_or(rest.len(), |(index, _)| index);
        }
        if !output.is_empty() {
            output.push_str("\r\n ");
        }
        output.push_str(&rest[..split]);
        rest = &rest[split..];
        limit = CONTINUATION_LIMIT;
    }
    output
}

fn escape_text(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn unescape_text(input: &str) -> String {
    let mut output = String::new();
    let mut characters = input.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next() {
                Some('n' | 'N') => output.push('\n'),
                Some(next) => output.push(next),
                None => output.push('\\'),
            }
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_folded_timed_and_all_day_events() {
        let input = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Personal\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART;TZID=Europe/Berlin:20260607T130000\r\nDTEND;TZID=Europe/Berlin:20260607T143000\r\nSUMMARY:A long \r\n title\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:two\r\nDTSTART;VALUE=DATE:20260608\r\nDTEND;VALUE=DATE:20260609\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse_ics(
            input,
            Path::new("/calendar/event.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "A long title");
        assert_eq!(events[0].calendar, "Personal");
        assert!(events[1].all_day);
    }

    #[test]
    fn excludes_nested_alarm_properties_from_event_metadata() {
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART:20260607T130000Z\r\nDTEND:20260607T140000Z\r\nSUMMARY:Meeting\r\nLOCATION:Office\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Meeting\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ics(
            input,
            Path::new("/calendar/event.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap()
        .remove(0);

        assert!(
            event
                .metadata
                .iter()
                .any(|property| property.name == "LOCATION")
        );
        assert!(event.metadata.iter().all(|property| !matches!(
            property.name.as_str(),
            "BEGIN" | "END" | "ACTION" | "DESCRIPTION" | "TRIGGER"
        )));
    }

    #[test]
    fn folds_utf8_content_lines_without_splitting_characters() {
        let line = format!("SUMMARY:{}", "🗓 calendar appointment ".repeat(5));
        let folded = fold_content_line(&line);
        assert!(folded.contains("\r\n "));
        assert_eq!(unfold(&folded), vec![line]);
        assert!(folded.split("\r\n").all(|part| part.len() <= 75));
    }

    #[test]
    fn expands_weekly_recurrence_with_count_and_exception() {
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:standup\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T093000Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4\r\nEXDATE:20260603T090000Z\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ics(
            input,
            Path::new("/calendar/standup.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap()
        .remove(0);

        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 1), &TimeZone::UTC)
                .len(),
            1
        );
        assert!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 3), &TimeZone::UTC)
                .is_empty()
        );
        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 8), &TimeZone::UTC)
                .len(),
            1
        );
        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 10), &TimeZone::UTC)
                .len(),
            1
        );
        assert!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 15), &TimeZone::UTC)
                .is_empty()
        );
    }

    #[test]
    fn recurring_search_result_uses_the_next_occurrence() {
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:daily\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ics(
            input,
            Path::new("/calendar/daily.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap()
        .remove(0);

        let occurrence =
            event.representative_occurrence(jiff::civil::date(2026, 6, 2), &TimeZone::UTC);

        assert_eq!(occurrence.start.date(), jiff::civil::date(2026, 6, 2));
    }

    #[test]
    fn expands_nth_weekday_monthly_recurrence() {
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:monthly\r\nDTSTART:20260609T100000Z\r\nDTEND:20260609T110000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=2TU;COUNT=2\r\nSUMMARY:Monthly meeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ics(
            input,
            Path::new("/calendar/monthly.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap()
        .remove(0);

        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 7, 14), &TimeZone::UTC)
                .len(),
            1
        );
        assert!(
            event
                .occurrences_on(jiff::civil::date(2026, 8, 11), &TimeZone::UTC)
                .is_empty()
        );
    }

    #[test]
    fn monthly_by_set_position_does_not_expand_every_week() {
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:monthly-setpos\r\nDTSTART:20260609T100000Z\r\nDTEND:20260609T110000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=TU;BYSETPOS=2;COUNT=2\r\nSUMMARY:Monthly meeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = parse_ics(
            input,
            Path::new("/calendar/monthly.ics"),
            Path::new("/calendar"),
            TimeZone::UTC,
        )
        .unwrap()
        .remove(0);

        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 9), &TimeZone::UTC)
                .len(),
            1
        );
        assert!(
            event
                .occurrences_on(jiff::civil::date(2026, 6, 16), &TimeZone::UTC)
                .is_empty()
        );
        assert_eq!(
            event
                .occurrences_on(jiff::civil::date(2026, 7, 14), &TimeZone::UTC)
                .len(),
            1
        );
    }

    #[test]
    fn recurrence_overrides_replace_or_cancel_the_master_instance() {
        let directory = tempfile::tempdir().unwrap();
        let input = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:series\r\nRECURRENCE-ID:20260602T090000Z\r\nDTSTART:20260602T140000Z\r\nDTEND:20260602T150000Z\r\nSUMMARY:Moved daily\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:series\r\nRECURRENCE-ID:20260603T090000Z\r\nDTSTART:20260603T090000Z\r\nDTEND:20260603T100000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        fs::write(directory.path().join("series.ics"), input).unwrap();
        let store = CalendarStore::with_timezone(
            directory.path().to_path_buf(),
            "default".to_string(),
            TimeZone::UTC,
        );
        let events = store.load().unwrap().events;

        let june_second = events
            .iter()
            .flat_map(|event| event.occurrences_on(jiff::civil::date(2026, 6, 2), &TimeZone::UTC))
            .collect::<Vec<_>>();
        assert_eq!(june_second.len(), 1);
        assert_eq!(june_second[0].summary, "Moved daily");
        assert_eq!(june_second[0].start.hour(), 14);
        assert!(
            events
                .iter()
                .flat_map(
                    |event| event.occurrences_on(jiff::civil::date(2026, 6, 3), &TimeZone::UTC)
                )
                .next()
                .is_none()
        );
    }

    #[test]
    fn deleting_one_recurring_occurrence_adds_an_exception() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("series.ics");
        fs::write(
            &path,
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let store = CalendarStore::with_timezone(
            directory.path().to_path_buf(),
            "default".to_string(),
            TimeZone::UTC,
        );
        let event = store.load().unwrap().events.remove(0);
        let occurrence = event
            .occurrences_on(jiff::civil::date(2026, 6, 2), &TimeZone::UTC)
            .remove(0)
            .start;

        store.delete_occurrence(&event, &occurrence).unwrap();

        let events = store.load().unwrap().events;
        assert!(
            events
                .iter()
                .flat_map(|event| {
                    event.occurrences_on(jiff::civil::date(2026, 6, 2), &TimeZone::UTC)
                })
                .next()
                .is_none()
        );
        assert_eq!(
            events
                .iter()
                .flat_map(|event| {
                    event.occurrences_on(jiff::civil::date(2026, 6, 3), &TimeZone::UTC)
                })
                .count(),
            1
        );
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .contains("EXDATE;TZID=UTC:20260602T090000")
        );
    }

    #[test]
    fn editing_one_recurring_occurrence_writes_an_override() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("series.ics");
        fs::write(
            &path,
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART:20260601T090000Z\r\nDTEND:20260601T100000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        let store = CalendarStore::with_timezone(
            directory.path().to_path_buf(),
            "default".to_string(),
            TimeZone::UTC,
        );
        let event = store.load().unwrap().events.remove(0);
        let occurrence = event
            .occurrences_on(jiff::civil::date(2026, 6, 2), &TimeZone::UTC)
            .remove(0)
            .start;
        let moved_start = jiff::civil::date(2026, 6, 2)
            .at(14, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap();
        let moved_end = &moved_start + 90.minutes();

        store
            .update_occurrence(
                &event,
                &occurrence,
                "Moved daily",
                EventTiming::Timed {
                    start: moved_start,
                    end: moved_end,
                },
            )
            .unwrap();

        let events = store.load().unwrap().events;
        let june_second = events
            .iter()
            .flat_map(|event| event.occurrences_on(jiff::civil::date(2026, 6, 2), &TimeZone::UTC))
            .collect::<Vec<_>>();
        assert_eq!(june_second.len(), 1);
        assert_eq!(june_second[0].summary, "Moved daily");
        assert_eq!(june_second[0].start.hour(), 14);
        let body = fs::read_to_string(path).unwrap();
        assert!(body.contains("RECURRENCE-ID;TZID=UTC:20260602T090000"));
    }
}
