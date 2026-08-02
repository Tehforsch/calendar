use jiff::{
    Span, ToSpan, Zoned,
    civil::{Date, Time, Weekday},
    tz::TimeZone,
};

#[derive(Debug, Clone)]
pub struct DateParser {
    time_zone: TimeZone,
    now: Zoned,
    default_time: Time,
}

impl DateParser {
    pub fn new(time_zone: TimeZone, now: Zoned, default_time: &str) -> Result<Self, String> {
        let default_time = parse_clock(default_time)
            .ok_or_else(|| format!("invalid default_start_time {default_time:?}"))?;
        Ok(Self {
            time_zone,
            now,
            default_time,
        })
    }

    /// Parses friendly calendar input. `base_date` lets a time-only end value
    /// (for example `14:30`) reuse the start date.
    pub fn parse(&self, input: &str, base_date: Option<Date>) -> Result<Zoned, String> {
        let input = input.trim().to_ascii_lowercase();
        if input.is_empty() {
            return Err("enter a date and time".to_string());
        }

        let mut parts = input.split_whitespace().collect::<Vec<_>>();
        let trailing_time = parts.last().and_then(|part| parse_clock(part));
        if trailing_time.is_some() {
            parts.pop();
        }

        if parts.is_empty() {
            let date = base_date.unwrap_or_else(|| self.now.date());
            return date
                .at(
                    trailing_time.unwrap().hour(),
                    trailing_time.unwrap().minute(),
                    0,
                    0,
                )
                .to_zoned(self.time_zone.clone())
                .map_err(|error| error.to_string());
        }

        let date_part = parts.join(" ");
        if (date_part.starts_with('+') || date_part.starts_with('-'))
            && let Ok(span) = date_part.parse::<Span>()
        {
            let relative = &self.now + span;
            if let Some(time) = trailing_time {
                return relative
                    .date()
                    .at(time.hour(), time.minute(), 0, 0)
                    .to_zoned(self.time_zone.clone())
                    .map_err(|error| error.to_string());
            }
            return Ok(relative);
        }

        let date = self.parse_date(&date_part)?;
        let time = trailing_time.unwrap_or(self.default_time);
        date.at(time.hour(), time.minute(), 0, 0)
            .to_zoned(self.time_zone.clone())
            .map_err(|error| error.to_string())
    }

    fn parse_date(&self, input: &str) -> Result<Date, String> {
        match input {
            "today" => return Ok(self.now.date()),
            "tomorrow" | "tom" => return self.now.date().tomorrow().map_err(|e| e.to_string()),
            "yesterday" => return self.now.date().yesterday().map_err(|e| e.to_string()),
            _ => {}
        }

        if let Some(weekday) = parse_weekday(input) {
            return self
                .now
                .date()
                .nth_weekday(1, weekday)
                .map_err(|error| error.to_string());
        }

        if let Some(date) = parse_iso_date(input) {
            return date;
        }

        let parts = input.split_whitespace().collect::<Vec<_>>();
        let (day, month, year) = match parts.as_slice() {
            [first, second] => {
                if let Some(month) = parse_month(first) {
                    (parse_day(second)?, month, None)
                } else if let Some(month) = parse_month(second) {
                    (parse_day(first)?, month, None)
                } else {
                    return Err(format!("could not understand date {input:?}"));
                }
            }
            [first, second, year] => {
                let year = year
                    .parse::<i16>()
                    .map_err(|_| format!("invalid year {year:?}"))?;
                if let Some(month) = parse_month(first) {
                    (parse_day(second)?, month, Some(year))
                } else if let Some(month) = parse_month(second) {
                    (parse_day(first)?, month, Some(year))
                } else {
                    return Err(format!("could not understand date {input:?}"));
                }
            }
            _ => return Err(format!("could not understand date {input:?}")),
        };

        if let Some(year) = year {
            return Date::new(year, month, day).map_err(|error| error.to_string());
        }

        let this_year = self.now.year();
        let candidate = Date::new(this_year, month, day).map_err(|error| error.to_string())?;
        let ten_days_ago = self.now.date() - 10.days();
        let year = if candidate < ten_days_ago {
            this_year + 1
        } else {
            this_year
        };
        Date::new(year, month, day).map_err(|error| error.to_string())
    }
}

pub fn parse_clock(input: &str) -> Option<Time> {
    let input = input.trim().to_ascii_lowercase();
    match input.as_str() {
        "noon" => return Time::new(12, 0, 0, 0).ok(),
        "midnight" => return Time::new(0, 0, 0, 0).ok(),
        _ => {}
    }
    let (clock, meridiem) = if let Some(clock) = input.strip_suffix("am") {
        (clock, Some(false))
    } else if let Some(clock) = input.strip_suffix("pm") {
        (clock, Some(true))
    } else {
        (input.as_str(), None)
    };
    let (hour, minute) = if let Some((hour, minute)) = clock.split_once(':') {
        (hour.parse::<i8>().ok()?, minute.parse::<i8>().ok()?)
    } else if meridiem.is_some() {
        (clock.parse::<i8>().ok()?, 0)
    } else {
        return None;
    };
    let hour = match meridiem {
        Some(false) if hour == 12 => 0,
        Some(false) if (1..=11).contains(&hour) => hour,
        Some(true) if hour == 12 => 12,
        Some(true) if (1..=11).contains(&hour) => hour + 12,
        Some(_) => return None,
        None => hour,
    };
    Time::new(hour, minute, 0, 0).ok()
}

pub fn parse_duration_minutes(input: &str) -> Result<i64, String> {
    let input = input.trim().to_ascii_lowercase();
    if input.is_empty() {
        return Err("enter a duration".to_string());
    }
    if let Some((hours, minutes)) = input.split_once(':') {
        let hours = hours
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("invalid hours {hours:?}"))?;
        let minutes = minutes
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("invalid minutes {minutes:?}"))?;
        if hours < 0 || !(0..60).contains(&minutes) {
            return Err("use H:MM, for example 1:30".to_string());
        }
        let total = hours
            .checked_mul(60)
            .and_then(|value| value.checked_add(minutes))
            .ok_or_else(|| "duration is too large".to_string())?;
        return positive_duration(total);
    }
    if input.chars().all(|character| character.is_ascii_digit()) {
        let minutes = input
            .parse::<i64>()
            .map_err(|_| "duration is too large".to_string())?;
        return positive_duration(minutes);
    }

    let bytes = input.as_bytes();
    let mut position = 0;
    let mut total = 0_i64;
    let mut found_component = false;
    while position < bytes.len() {
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let number_start = position;
        while position < bytes.len() && bytes[position].is_ascii_digit() {
            position += 1;
        }
        if number_start == position {
            return Err("use values such as 45m, 1h30m, 1:30, or 2h".to_string());
        }
        let amount = input[number_start..position]
            .parse::<i64>()
            .map_err(|_| "duration is too large".to_string())?;
        while position < bytes.len() && bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let unit_start = position;
        while position < bytes.len() && bytes[position].is_ascii_alphabetic() {
            position += 1;
        }
        let multiplier = match &input[unit_start..position] {
            "m" | "min" | "mins" | "minute" | "minutes" => 1,
            "h" | "hr" | "hrs" | "hour" | "hours" => 60,
            "d" | "day" | "days" => 24 * 60,
            _ => return Err("use m for minutes, h for hours, or d for days".to_string()),
        };
        total = total
            .checked_add(
                amount
                    .checked_mul(multiplier)
                    .ok_or_else(|| "duration is too large".to_string())?,
            )
            .ok_or_else(|| "duration is too large".to_string())?;
        found_component = true;
    }
    if !found_component {
        return Err("enter a duration".to_string());
    }
    positive_duration(total)
}

fn positive_duration(minutes: i64) -> Result<i64, String> {
    if minutes > 0 {
        Ok(minutes)
    } else {
        Err("duration must be greater than zero".to_string())
    }
}

fn parse_iso_date(input: &str) -> Option<Result<Date, String>> {
    let mut parts = input.split('-');
    let year = parts.next()?.parse::<i16>().ok()?;
    let month = parts.next()?.parse::<i8>().ok()?;
    let day = parts.next()?.parse::<i8>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Date::new(year, month, day).map_err(|error| error.to_string()))
}

fn parse_day(input: &str) -> Result<i8, String> {
    input
        .trim_end_matches(|character: char| character.is_ascii_alphabetic())
        .parse::<i8>()
        .map_err(|_| format!("invalid day {input:?}"))
}

fn parse_month(input: &str) -> Option<i8> {
    match input.trim_end_matches('.') {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn parse_weekday(input: &str) -> Option<Weekday> {
    match input {
        "mon" | "monday" => Some(Weekday::Monday),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tuesday),
        "wed" | "wednesday" => Some(Weekday::Wednesday),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thursday),
        "fri" | "friday" => Some(Weekday::Friday),
        "sat" | "saturday" => Some(Weekday::Saturday),
        "sun" | "sunday" => Some(Weekday::Sunday),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use jiff::{
        civil::{DateTime, date},
        tz::TimeZone,
    };

    use super::*;

    fn parser() -> DateParser {
        let timezone = TimeZone::UTC;
        let now = date(2026, 6, 1)
            .at(8, 0, 0, 0)
            .to_zoned(timezone.clone())
            .unwrap();
        DateParser::new(timezone, now, "09:00").unwrap()
    }

    #[test]
    fn parses_requested_month_day_and_time() {
        assert_eq!(
            parser().parse("jun 7 13:00", None).unwrap().datetime(),
            DateTime::new(2026, 6, 7, 13, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn parses_requested_weekday_and_twelve_hour_time() {
        assert_eq!(
            parser().parse("sun 5pm", None).unwrap().datetime(),
            DateTime::new(2026, 6, 7, 17, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn time_only_end_reuses_start_date() {
        assert_eq!(
            parser()
                .parse("6:30pm", Some(date(2026, 7, 2)))
                .unwrap()
                .datetime(),
            DateTime::new(2026, 7, 2, 18, 30, 0, 0).unwrap()
        );
    }

    #[test]
    fn parses_friendly_durations() {
        assert_eq!(parse_duration_minutes("90"), Ok(90));
        assert_eq!(parse_duration_minutes("1:30"), Ok(90));
        assert_eq!(parse_duration_minutes("1h30m"), Ok(90));
        assert_eq!(parse_duration_minutes("2 hours 15 minutes"), Ok(135));
        assert!(parse_duration_minutes("0m").is_err());
    }
}
