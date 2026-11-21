//! MySQL's date and time functions, in UTC (the session's `time_zone` is `+00:00`).
//!
//! A zero date (`0000-00-00[ 00:00:00]`) gives 0 from the part functions and NULL from formatting, as MySQL does.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use rusqlite::Connection;
use rusqlite::types::Value;

use super::{deterministic, int, text, volatile};

const DATETIME: &str = "%Y-%m-%d %H:%M:%S";

/// A MySQL date or datetime string read as numbers: year, month, day, hour, minute, second, microsecond.
fn components(s: &str) -> Option<[i64; 7]> {
    let s = s.trim();
    let (date, time) = match s.find([' ', 'T']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None if s.contains(':') && !s.contains('-') => ("", s),
        None => (s, ""),
    };
    let mut out = [0i64; 7];
    if !date.is_empty() {
        let d: Vec<i64> = date
            .split('-')
            .map(|p| p.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        if d.len() != 3 {
            return None;
        }
        out[..3].copy_from_slice(&d);
    }
    if !time.is_empty() {
        let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
        let t: Vec<i64> = hms
            .split(':')
            .map(|p| p.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        for (i, v) in t.iter().take(3).enumerate() {
            out[3 + i] = *v;
        }
        if !frac.is_empty() {
            let digits: String = format!("{frac:0<6}").chars().take(6).collect();
            out[6] = digits.parse().unwrap_or(0);
        }
    }
    Some(out)
}

/// A date or datetime string as a datetime, and whether it was a date alone; `None` for zero or invalid dates.
fn parse(s: &str) -> Option<(NaiveDateTime, bool)> {
    let c = components(s)?;
    let date = NaiveDate::from_ymd_opt(c[0] as i32, c[1] as u32, c[2] as u32)?;
    let time = NaiveTime::from_hms_micro_opt(c[3] as u32, c[4] as u32, c[5] as u32, c[6] as u32)?;
    let date_only = !s.trim().contains(':');
    Some((date.and_time(time), date_only))
}

fn is_zero(s: &str) -> bool {
    components(s).is_some_and(|c| c[0] == 0 && c[1] == 0 && c[2] == 0)
}

fn add_months(dt: NaiveDateTime, months: i64) -> Option<NaiveDateTime> {
    let total = dt.year() as i64 * 12 + dt.month0() as i64 + months;
    let (year, month0) = (total.div_euclid(12), total.rem_euclid(12));
    let last = last_day_of(year as i32, month0 as u32 + 1)?;
    let day = dt.day().min(last.day());
    Some(NaiveDate::from_ymd_opt(year as i32, month0 as u32 + 1, day)?.and_time(dt.time()))
}

fn last_day_of(year: i32, month: u32) -> Option<NaiveDate> {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()
}

/// The fields a compound unit's amount fills, largest first.
fn compound_fields(unit: &str) -> Option<&'static [&'static str]> {
    Some(match unit {
        "SECOND_MICROSECOND" => &["SECOND", "MICROSECOND"],
        "MINUTE_MICROSECOND" => &["MINUTE", "SECOND", "MICROSECOND"],
        "MINUTE_SECOND" => &["MINUTE", "SECOND"],
        "HOUR_MICROSECOND" => &["HOUR", "MINUTE", "SECOND", "MICROSECOND"],
        "HOUR_SECOND" => &["HOUR", "MINUTE", "SECOND"],
        "HOUR_MINUTE" => &["HOUR", "MINUTE"],
        "DAY_MICROSECOND" => &["DAY", "HOUR", "MINUTE", "SECOND", "MICROSECOND"],
        "DAY_SECOND" => &["DAY", "HOUR", "MINUTE", "SECOND"],
        "DAY_MINUTE" => &["DAY", "HOUR", "MINUTE"],
        "DAY_HOUR" => &["DAY", "HOUR"],
        "YEAR_MONTH" => &["YEAR", "MONTH"],
        _ => return None,
    })
}

/// `DATE_ADD(d, INTERVAL amount unit)`, negated for `DATE_SUB`.
fn date_add(d: &str, amount: &str, unit: &str, negate: bool) -> Option<String> {
    let (dt, date_only) = parse(d)?;
    let unit = unit.to_ascii_uppercase();
    let sign: i64 = if negate { -1 } else { 1 };
    let mut fields: Vec<(&str, i64)> = Vec::new();
    match compound_fields(&unit) {
        Some(names) => {
            let negative = amount.trim_start().starts_with('-');
            let nums: Vec<i64> = amount
                .split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect();
            // A shorter amount fills the smallest fields (`'30'` HOUR_MINUTE is 30 minutes).
            let skip = names.len().saturating_sub(nums.len());
            for (name, n) in names.iter().skip(skip).zip(nums) {
                fields.push((name, if negative { -n } else { n }));
            }
        }
        None => {
            let n = super::numeric_prefix(amount).round() as i64;
            fields.push((
                match unit.as_str() {
                    "MICROSECOND" | "SECOND" | "MINUTE" | "HOUR" | "DAY" | "WEEK" | "MONTH"
                    | "QUARTER" | "YEAR" => Box::leak(unit.clone().into_boxed_str()),
                    _ => return None,
                },
                n,
            ));
        }
    }
    let mut out = dt;
    for (field, n) in fields {
        let n = n * sign;
        out = match field {
            "YEAR" => add_months(out, n * 12)?,
            "QUARTER" => add_months(out, n * 3)?,
            "MONTH" => add_months(out, n)?,
            "WEEK" => out.checked_add_signed(Duration::weeks(n))?,
            "DAY" => out.checked_add_signed(Duration::days(n))?,
            "HOUR" => out.checked_add_signed(Duration::hours(n))?,
            "MINUTE" => out.checked_add_signed(Duration::minutes(n))?,
            "SECOND" => out.checked_add_signed(Duration::seconds(n))?,
            "MICROSECOND" => out.checked_add_signed(Duration::microseconds(n))?,
            _ => return None,
        };
    }
    let day_or_larger = matches!(
        unit.as_str(),
        "DAY" | "WEEK" | "MONTH" | "QUARTER" | "YEAR" | "YEAR_MONTH"
    );
    Some(if date_only && day_or_larger {
        out.format("%Y-%m-%d").to_string()
    } else if unit.contains("MICROSECOND") {
        out.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
    } else {
        out.format(DATETIME).to_string()
    })
}

/// MySQL's `WEEK(date, mode)` (sql/sql_time.cc's `calc_week`).
fn week(date: NaiveDate, mode: i64) -> i64 {
    const MONDAY_FIRST: i64 = 1;
    const WEEK_YEAR: i64 = 2;
    const FIRST_WEEKDAY: i64 = 4;
    let mut behaviour = mode & 7;
    if behaviour & MONDAY_FIRST == 0 {
        behaviour ^= FIRST_WEEKDAY;
    }
    let monday_first = behaviour & MONDAY_FIRST != 0;
    let mut week_year = behaviour & WEEK_YEAR != 0;
    let first_weekday = behaviour & FIRST_WEEKDAY != 0;
    let weekday_of = |d: NaiveDate| -> i64 {
        if monday_first {
            d.weekday().num_days_from_monday() as i64
        } else {
            d.weekday().num_days_from_sunday() as i64
        }
    };
    let days_in_year = |y: i32| -> i64 {
        if NaiveDate::from_ymd_opt(y, 2, 29).is_some() {
            366
        } else {
            365
        }
    };
    let mut year = date.year();
    let first = NaiveDate::from_ymd_opt(year, 1, 1).expect("January 1st exists");
    let daynr = date.num_days_from_ce() as i64;
    let mut first_daynr = first.num_days_from_ce() as i64;
    let mut weekday = weekday_of(first);
    if date.month() == 1 && (date.day() as i64) <= 7 - weekday {
        if !week_year && ((first_weekday && weekday != 0) || (!first_weekday && weekday >= 4)) {
            return 0;
        }
        week_year = true;
        year -= 1;
        let days = days_in_year(year);
        first_daynr -= days;
        weekday = (weekday + 53 * 7 - days) % 7;
    }
    let days = if (first_weekday && weekday != 0) || (!first_weekday && weekday >= 4) {
        daynr - (first_daynr + (7 - weekday))
    } else {
        daynr - (first_daynr - weekday)
    };
    if week_year && days >= 52 * 7 {
        let weekday = (weekday + days_in_year(year)) % 7;
        if (!first_weekday && weekday < 4) || (first_weekday && weekday == 0) {
            return 1;
        }
    }
    days / 7 + 1
}

/// MySQL's `DATE_FORMAT` specifiers.
fn date_format(dt: NaiveDateTime, format: &str) -> String {
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = chars.next() else {
            out.push('%');
            break;
        };
        let hour12 = if dt.hour().is_multiple_of(12) {
            12
        } else {
            dt.hour() % 12
        };
        let piece = match spec {
            'a' => dt.format("%a").to_string(),
            'b' => dt.format("%b").to_string(),
            'c' => dt.month().to_string(),
            'D' => {
                let d = dt.day();
                let suffix = match (d % 10, d % 100) {
                    (_, 11..=13) => "th",
                    (1, _) => "st",
                    (2, _) => "nd",
                    (3, _) => "rd",
                    _ => "th",
                };
                format!("{d}{suffix}")
            }
            'd' => format!("{:02}", dt.day()),
            'e' => dt.day().to_string(),
            'f' => format!("{:06}", dt.nanosecond() / 1000),
            'H' => format!("{:02}", dt.hour()),
            'h' | 'I' => format!("{hour12:02}"),
            'i' => format!("{:02}", dt.minute()),
            'j' => format!("{:03}", dt.ordinal()),
            'k' => dt.hour().to_string(),
            'l' => hour12.to_string(),
            'M' => dt.format("%B").to_string(),
            'm' => format!("{:02}", dt.month()),
            'p' => (if dt.hour() < 12 { "AM" } else { "PM" }).to_string(),
            'r' => format!(
                "{hour12:02}:{:02}:{:02} {}",
                dt.minute(),
                dt.second(),
                if dt.hour() < 12 { "AM" } else { "PM" }
            ),
            'S' | 's' => format!("{:02}", dt.second()),
            'T' => dt.format("%H:%M:%S").to_string(),
            'U' => format!("{:02}", week(dt.date(), 0)),
            'u' => format!("{:02}", week(dt.date(), 1)),
            'V' => format!("{:02}", week(dt.date(), 2)),
            'v' => format!("{:02}", week(dt.date(), 3)),
            'W' => dt.format("%A").to_string(),
            'w' => dt.weekday().num_days_from_sunday().to_string(),
            'X' | 'x' => dt.format("%G").to_string(),
            'Y' => format!("{:04}", dt.year()),
            'y' => format!("{:02}", dt.year() % 100),
            '%' => "%".to_string(),
            other => other.to_string(),
        };
        out.push_str(&piece);
    }
    out
}

fn part(
    index: usize,
) -> impl Fn(&mut rusqlite::functions::Context<'_>) -> rusqlite::Result<Value> + Send + 'static {
    move |ctx| {
        Ok(text(ctx, 0)
            .and_then(|s| components(&s))
            .map_or(Value::Null, |c| Value::Integer(c[index])))
    }
}

fn on_date(
    f: impl Fn(NaiveDateTime) -> Value + Send + 'static,
) -> impl Fn(&mut rusqlite::functions::Context<'_>) -> rusqlite::Result<Value> + Send + 'static {
    move |ctx| {
        Ok(text(ctx, 0)
            .and_then(|s| parse(&s))
            .map_or(Value::Null, |(dt, _)| f(dt)))
    }
}

pub(crate) fn register(conn: &Connection) -> rusqlite::Result<()> {
    let f = deterministic();
    for name in [
        "now",
        "sysdate",
        "utc_timestamp",
        "current_timestamp",
        "localtimestamp",
    ] {
        conn.create_scalar_function(name, 0, volatile(), |_| {
            Ok(Value::Text(Utc::now().format(DATETIME).to_string()))
        })?;
    }
    for name in ["curdate", "utc_date", "current_date"] {
        conn.create_scalar_function(name, 0, volatile(), |_| {
            Ok(Value::Text(Utc::now().format("%Y-%m-%d").to_string()))
        })?;
    }
    for name in ["curtime", "current_time"] {
        conn.create_scalar_function(name, 0, volatile(), |_| {
            Ok(Value::Text(Utc::now().format("%H:%M:%S").to_string()))
        })?;
    }
    conn.create_scalar_function("unix_timestamp", 0, volatile(), |_| {
        Ok(Value::Integer(Utc::now().timestamp()))
    })?;
    conn.create_scalar_function("unix_timestamp", 1, f, |ctx| {
        let Some(s) = text(ctx, 0) else {
            return Ok(Value::Null);
        };
        if is_zero(&s) {
            return Ok(Value::Integer(0));
        }
        Ok(parse(&s).map_or(Value::Null, |(dt, _)| {
            Value::Integer(dt.and_utc().timestamp())
        }))
    })?;
    for n_arg in [1, 2] {
        conn.create_scalar_function("from_unixtime", n_arg, f, move |ctx| {
            let Some(n) = int(ctx, 0) else {
                return Ok(Value::Null);
            };
            let Some(dt) = chrono::DateTime::from_timestamp(n, 0).map(|d| d.naive_utc()) else {
                return Ok(Value::Null);
            };
            if n_arg == 1 {
                return Ok(Value::Text(dt.format(DATETIME).to_string()));
            }
            Ok(text(ctx, 1).map_or(Value::Null, |fmt| Value::Text(date_format(dt, &fmt))))
        })?;
    }
    conn.create_scalar_function("date_format", 2, f, |ctx| {
        match (text(ctx, 0), text(ctx, 1)) {
            (Some(d), Some(fmt)) => {
                Ok(parse(&d).map_or(Value::Null, |(dt, _)| Value::Text(date_format(dt, &fmt))))
            }
            _ => Ok(Value::Null),
        }
    })?;
    for (name, negate) in [
        ("speedyspec_date_add", false),
        ("speedyspec_date_sub", true),
    ] {
        conn.create_scalar_function(name, 3, f, move |ctx| {
            match (text(ctx, 0), text(ctx, 1), text(ctx, 2)) {
                (Some(d), Some(amount), Some(unit)) => {
                    Ok(date_add(&d, &amount, &unit, negate).map_or(Value::Null, Value::Text))
                }
                _ => Ok(Value::Null),
            }
        })?;
    }
    conn.create_scalar_function("date", 1, f, |ctx| {
        let Some(s) = text(ctx, 0) else {
            return Ok(Value::Null);
        };
        if is_zero(&s) {
            return Ok(Value::Text("0000-00-00".into()));
        }
        Ok(parse(&s).map_or(Value::Null, |(dt, _)| {
            Value::Text(dt.format("%Y-%m-%d").to_string())
        }))
    })?;
    conn.create_scalar_function("year", 1, f, part(0))?;
    conn.create_scalar_function("month", 1, f, part(1))?;
    for name in ["dayofmonth", "day"] {
        conn.create_scalar_function(name, 1, f, part(2))?;
    }
    conn.create_scalar_function("hour", 1, f, part(3))?;
    conn.create_scalar_function("minute", 1, f, part(4))?;
    conn.create_scalar_function("second", 1, f, part(5))?;
    conn.create_scalar_function(
        "dayofweek",
        1,
        f,
        on_date(|dt| Value::Integer(dt.weekday().num_days_from_sunday() as i64 + 1)),
    )?;
    conn.create_scalar_function(
        "weekday",
        1,
        f,
        on_date(|dt| Value::Integer(dt.weekday().num_days_from_monday() as i64)),
    )?;
    conn.create_scalar_function(
        "dayofyear",
        1,
        f,
        on_date(|dt| Value::Integer(dt.ordinal() as i64)),
    )?;
    conn.create_scalar_function(
        "quarter",
        1,
        f,
        on_date(|dt| Value::Integer((dt.month0() / 3 + 1) as i64)),
    )?;
    conn.create_scalar_function(
        "last_day",
        1,
        f,
        on_date(|dt| {
            last_day_of(dt.year(), dt.month()).map_or(Value::Null, |d| {
                Value::Text(d.format("%Y-%m-%d").to_string())
            })
        }),
    )?;
    conn.create_scalar_function("datediff", 2, f, |ctx| {
        match (
            text(ctx, 0).and_then(|s| parse(&s)),
            text(ctx, 1).and_then(|s| parse(&s)),
        ) {
            (Some((a, _)), Some((b, _))) => Ok(Value::Integer((a.date() - b.date()).num_days())),
            _ => Ok(Value::Null),
        }
    })?;
    for n_arg in [1, 2] {
        conn.create_scalar_function("week", n_arg, f, move |ctx| {
            let mode = if n_arg == 2 {
                int(ctx, 1).unwrap_or(0)
            } else {
                0
            };
            Ok(text(ctx, 0)
                .and_then(|s| parse(&s))
                .map_or(Value::Null, |(dt, _)| Value::Integer(week(dt.date(), mode))))
        })?;
    }
    Ok(())
}
