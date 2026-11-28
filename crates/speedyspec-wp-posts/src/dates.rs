//! WordPress's post date rules: `wp_resolve_post_date()` (post.php, 7.2-RC2 lines 5761-5784) up to the
//! `wp_checkdate` filter, and the dates and status `wp_insert_post()` derives from the resolved date (5020-5050).
//!
//! The time functions are a port ([`Clock`], Strategy), so the rules read no global clock or timezone. PHP strings
//! are bytes: the dates are `&[u8]`, and only the status and post type names are text.

use speedyspec_php_serialize::{PhpKey, PhpValue};
use speedyspec_wp_posts_contracts::{Clock, PostDateTime, PostError, PostStatuses};

use crate::post_status_names;

/// `MINUTE_IN_SECONDS`.
const MINUTE_IN_SECONDS: i64 = 60;

/// `wp_resolve_post_date()` up to `wp_checkdate`'s filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPostDate {
    /// The date does not start `YYYY-M-D`: the function returns false before any filter runs.
    NoDate,
    /// The date (whole, as resolved) and whether `checkdate()` accepts its year, month and day; the
    /// `wp_checkdate` filter may still change that verdict.
    Checked {
        /// The resolved date, whole: the pattern is anchored only at its start.
        date: Vec<u8>,
        /// `checkdate( $month, $day, $year )`.
        valid: bool,
    },
}

impl ResolvedPostDate {
    /// The function's answer with no `wp_checkdate` callback: the date when valid, else `None` (false).
    pub fn unfiltered(self) -> Option<Vec<u8>> {
        match self {
            Self::Checked { date, valid: true } => Some(date),
            _ => None,
        }
    }
}

/// `empty( $date ) || '0000-00-00 00:00:00' === $date` for a string: `""`, `"0"` and the zero date.
fn empty_date(date: &[u8]) -> bool {
    date.is_empty() || date == b"0" || date == PostDateTime::ZERO.as_str().as_bytes()
}

/// `wp_resolve_post_date( $post_date, $post_date_gmt )` without the `wp_checkdate` filter: an empty local date
/// becomes now when the GMT date is empty too, else the GMT date in local time; then
/// `/^(\d{4})-(\d{1,2})-(\d{1,2})/` and `checkdate()`.
///
/// Dates are bytes here, as PHP strings are, while [`PostDateTime`] holds a `String`: a caller converts between
/// them, lossily for bytes that are not UTF-8.
///
/// `Err` is the clock's failure (an option filter that throws while it reads the time zone).
pub fn resolve_post_date(
    post_date: &[u8],
    post_date_gmt: &[u8],
    clock: &dyn Clock,
) -> Result<ResolvedPostDate, PostError> {
    let date = if !empty_date(post_date) {
        post_date.to_vec()
    } else if empty_date(post_date_gmt) {
        clock.now_local()?.into_bytes()
    } else {
        clock.date_from_gmt(post_date_gmt)?
    };
    Ok(match date_parts(&date) {
        None => ResolvedPostDate::NoDate,
        Some((year, month, day)) => ResolvedPostDate::Checked {
            valid: gregorian_date_valid(month, day, year),
            date,
        },
    })
}

/// `preg_match( '/^(\d{4})-(\d{1,2})-(\d{1,2})/', … )`: ASCII digits (no `u` flag), greedy, anchored at the start
/// only. Returns `(year, month, day)`.
fn date_parts(date: &[u8]) -> Option<(i64, i64, i64)> {
    let digits = |from: usize, most: usize| -> usize {
        date[from.min(date.len())..]
            .iter()
            .take(most)
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let number = |from: usize, len: usize| -> i64 {
        date[from..from + len]
            .iter()
            .fold(0, |n, b| n * 10 + i64::from(b - b'0'))
    };
    if digits(0, 4) != 4 || date.get(4) != Some(&b'-') {
        return None;
    }
    // `\d{1,2}` greedy, backtracking to one digit when two are not followed by `-`.
    let month_len = (1..=digits(5, 2))
        .rev()
        .find(|len| date.get(5 + len) == Some(&b'-'))?;
    let day_at = 5 + month_len + 1;
    let day_len = digits(day_at, 2);
    if day_len == 0 {
        return None;
    }
    Some((number(0, 4), number(5, month_len), number(day_at, day_len)))
}

/// PHP's `checkdate()`: year 1..=32767, month 1..=12, the day within the month, leap years Gregorian.
pub fn gregorian_date_valid(month: i64, day: i64, year: i64) -> bool {
    if !(1..=32767).contains(&year) || !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

/// What [`insert_post_dates`] reads from `wp_insert_post()`.
#[derive(Debug, Clone, Copy)]
pub struct InsertDateInput<'a> {
    /// The date `wp_resolve_post_date()` resolved.
    pub post_date: &'a [u8],
    /// `$postarr['post_date_gmt']`, as given.
    pub post_date_gmt: &'a [u8],
    /// `$post_status`.
    pub status: &'a str,
    /// `$post_type`.
    pub post_type: &'a str,
    /// `$update`.
    pub update: bool,
}

/// The dates and status `wp_insert_post()` derives from the resolved date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertDates {
    /// `$post_date_gmt`.
    pub date_gmt: Vec<u8>,
    /// `$post_modified`.
    pub modified: Vec<u8>,
    /// `$post_modified_gmt`.
    pub modified_gmt: Vec<u8>,
    /// `$post_status`, after the `publish`/`future` swap.
    pub status: String,
}

/// `wp_insert_post()` lines 5020-5050, after `wp_resolve_post_date()`: the GMT date (given, else derived from the
/// local date unless the status floats, then zero), the modified dates (now on update or for a zero date, else the
/// post's), and `publish` scheduled as `future` a minute or more ahead, `future` published when less than a minute
/// ahead, except for attachments. A date `strtotime()` cannot read counts as 0, as PHP's `false` does.
///
/// Dates are bytes here, as PHP strings are, while [`PostDateTime`] holds a `String`: a caller converts between
/// them, lossily for bytes that are not UTF-8.
///
/// `Err` is the clock's failure, where WordPress's exception would stop `wp_insert_post()`.
pub fn insert_post_dates(
    input: &InsertDateInput<'_>,
    statuses: &dyn PostStatuses,
    clock: &dyn Clock,
) -> Result<InsertDates, PostError> {
    let date_gmt = if !empty_date(input.post_date_gmt) {
        input.post_date_gmt.to_vec()
    } else if status_floats(input.status, statuses) {
        PostDateTime::ZERO.as_str().as_bytes().to_vec()
    } else {
        clock.gmt_from_date(input.post_date)?
    };

    let (modified, modified_gmt) =
        if input.update || input.post_date == PostDateTime::ZERO.as_str().as_bytes() {
            (
                clock.now_local()?.into_bytes(),
                clock.now_gmt().into_bytes(),
            )
        } else {
            (input.post_date.to_vec(), date_gmt.clone())
        };

    let mut status = input.status.to_owned();
    if input.post_type != "attachment" && (status == "publish" || status == "future") {
        let now = clock.now_gmt();
        let ahead =
            clock.timestamp(&date_gmt).unwrap_or(0) - clock.timestamp(now.as_bytes()).unwrap_or(0);
        if status == "publish" && ahead >= MINUTE_IN_SECONDS {
            status = "future".to_owned();
        } else if status == "future" && ahead < MINUTE_IN_SECONDS {
            status = "publish".to_owned();
        }
    }

    Ok(InsertDates {
        date_gmt,
        modified,
        modified_gmt,
        status,
    })
}

/// `in_array( $post_status, get_post_stati( array( 'date_floating' => true ) ), true )`.
fn status_floats(status: &str, statuses: &dyn PostStatuses) -> bool {
    let args = [(PhpKey::Str(b"date_floating".to_vec()), PhpValue::Bool(true))];
    post_status_names(statuses, &args, "and")
        .iter()
        .any(|(_, name)| matches!(name, PhpValue::Str(s) if s.as_slice() == status.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use speedyspec_php_serialize::{PhpKey, PhpValue};
    use speedyspec_wp_posts_contracts::Clock;

    use super::*;
    use crate::PostStatusRegistry;

    const NOW_LOCAL: &str = "2026-11-25 19:18:50";
    const NOW_GMT: &str = "2026-11-26 01:18:50";
    const NOW_TS: i64 = 1_795_655_930;

    /// A fixed clock: now is [`NOW_LOCAL`] / [`NOW_GMT`], GMT is local plus six hours for the dates the tests
    /// use, and the timestamps are a table (anything else is `strtotime()`'s false).
    struct Fixed {
        to_gmt: HashMap<&'static str, &'static str>,
        timestamps: HashMap<&'static str, i64>,
    }

    impl Fixed {
        fn new() -> Self {
            Self {
                to_gmt: HashMap::from([
                    ("2024-03-01 06:00:00", "2024-03-01 12:00:00"),
                    ("2026-11-25 19:19:50", "2026-11-26 01:19:50"),
                    ("2026-11-25 19:17:50", "2026-11-26 01:17:50"),
                ]),
                timestamps: HashMap::from([
                    (NOW_GMT, NOW_TS),
                    ("2026-11-26 01:19:50", NOW_TS + 60),
                    ("2026-11-26 01:19:49", NOW_TS + 59),
                    ("2026-11-26 01:17:50", NOW_TS - 60),
                    ("2026-11-26 01:18:49", NOW_TS - 1),
                ]),
            }
        }
    }

    impl Clock for Fixed {
        fn now_local(&self) -> Result<String, PostError> {
            Ok(NOW_LOCAL.to_owned())
        }
        fn now_gmt(&self) -> String {
            NOW_GMT.to_owned()
        }
        fn date_from_gmt(&self, gmt: &[u8]) -> Result<Vec<u8>, PostError> {
            panic!(
                "get_date_from_gmt({:?}) unexpected",
                String::from_utf8_lossy(gmt)
            )
        }
        fn gmt_from_date(&self, local: &[u8]) -> Result<Vec<u8>, PostError> {
            let local = std::str::from_utf8(local).unwrap();
            Ok(self.to_gmt[local].as_bytes().to_vec())
        }
        fn timestamp(&self, datetime: &[u8]) -> Option<i64> {
            self.timestamps
                .get(std::str::from_utf8(datetime).ok()?)
                .copied()
        }
    }

    fn statuses() -> PostStatusRegistry {
        let mut statuses = PostStatusRegistry::new();
        let floating = || vec![(PhpKey::Str(b"date_floating".to_vec()), PhpValue::Bool(true))];
        statuses.register(b"draft", floating());
        statuses.register(b"pending", floating());
        statuses.register(b"publish", Vec::new());
        statuses.register(b"future", Vec::new());
        statuses.register(b"private", Vec::new());
        statuses
    }

    fn insert(
        post_date: &str,
        post_date_gmt: &str,
        status: &str,
        post_type: &str,
        update: bool,
    ) -> InsertDates {
        insert_post_dates(
            &InsertDateInput {
                post_date: post_date.as_bytes(),
                post_date_gmt: post_date_gmt.as_bytes(),
                status,
                post_type,
                update,
            },
            &statuses(),
            &Fixed::new(),
        )
        .unwrap()
    }

    fn resolve(date: &str) -> Option<Vec<u8>> {
        resolve_post_date(date.as_bytes(), b"", &Fixed::new())
            .unwrap()
            .unfiltered()
    }

    #[test]
    fn zero_string_means_now() {
        for empty in ["", "0", "0000-00-00 00:00:00"] {
            assert_eq!(
                resolve(empty),
                Some(NOW_LOCAL.as_bytes().to_vec()),
                "{empty:?}"
            );
        }
        // "00" is not empty.
        assert_eq!(resolve("00"), None);
    }

    #[test]
    fn unanchored_end_keeps_the_text() {
        assert_eq!(
            resolve("2024-1-5 garbage"),
            Some(b"2024-1-5 garbage".to_vec())
        );
        assert_eq!(resolve("2024-01-0512"), Some(b"2024-01-0512".to_vec()));
        assert_eq!(resolve(" 2024-01-01"), None);
        assert_eq!(resolve("24-01-05"), None);
        assert_eq!(resolve("2024-123-1"), None);
        assert_eq!(resolve("２０２４-01-01"), None);
        assert!(matches!(
            resolve_post_date(b"abc", b"", &Fixed::new()).unwrap(),
            ResolvedPostDate::NoDate
        ));
    }

    #[test]
    fn year_zero_and_feb_29_2023_invalid() {
        assert_eq!(resolve("0000-01-01 00:00:00"), None);
        assert_eq!(resolve("2023-02-29 10:00:00"), None);
        assert!(matches!(
            resolve_post_date(b"2023-02-29", b"", &Fixed::new()).unwrap(),
            ResolvedPostDate::Checked { valid: false, .. }
        ));
        assert_eq!(
            resolve("2024-02-29 10:00:00"),
            Some(b"2024-02-29 10:00:00".to_vec())
        );
    }

    #[test]
    fn checkdate_bounds() {
        assert!(gregorian_date_valid(12, 31, 32767));
        assert!(!gregorian_date_valid(1, 1, 32768));
        assert!(!gregorian_date_valid(1, 1, 0));
        assert!(gregorian_date_valid(2, 29, 2000));
        assert!(!gregorian_date_valid(2, 29, 1900));
        assert!(!gregorian_date_valid(0, 1, 2024));
        assert!(!gregorian_date_valid(13, 1, 2024));
        assert!(!gregorian_date_valid(4, 31, 2024));
        assert!(!gregorian_date_valid(1, 0, 2024));
    }

    #[test]
    fn gmt_defaults_from_local_unless_floating() {
        let dates = insert("2024-03-01 06:00:00", "", "private", "post", false);
        assert_eq!(dates.date_gmt, b"2024-03-01 12:00:00");
        let dates = insert(
            "2024-03-01 06:00:00",
            "0000-00-00 00:00:00",
            "private",
            "post",
            false,
        );
        assert_eq!(dates.date_gmt, b"2024-03-01 12:00:00");
        let dates = insert(
            "2024-03-01 06:00:00",
            "2024-03-01 13:00:00",
            "draft",
            "post",
            false,
        );
        assert_eq!(dates.date_gmt, b"2024-03-01 13:00:00");
        assert_eq!(dates.modified, b"2024-03-01 06:00:00");
        assert_eq!(dates.modified_gmt, b"2024-03-01 13:00:00");
    }

    #[test]
    fn floating_status_keeps_zero_gmt() {
        for status in ["draft", "pending"] {
            let dates = insert("2024-03-01 06:00:00", "0", status, "post", false);
            assert_eq!(dates.date_gmt, b"0000-00-00 00:00:00", "{status}");
            assert_eq!(dates.modified_gmt, b"0000-00-00 00:00:00", "{status}");
        }
    }

    #[test]
    fn update_sets_modified_to_now() {
        let dates = insert(
            "2024-03-01 06:00:00",
            "2024-03-01 12:00:00",
            "draft",
            "post",
            true,
        );
        assert_eq!(dates.modified, NOW_LOCAL.as_bytes());
        assert_eq!(dates.modified_gmt, NOW_GMT.as_bytes());
        let dates = insert(
            "0000-00-00 00:00:00",
            "2024-03-01 12:00:00",
            "draft",
            "post",
            false,
        );
        assert_eq!(dates.modified, NOW_LOCAL.as_bytes());
        assert_eq!(dates.modified_gmt, NOW_GMT.as_bytes());
    }

    #[test]
    fn publish_in_the_future_becomes_future() {
        let at = |gmt| insert("2026-11-25 19:00:00", gmt, "publish", "post", false).status;
        assert_eq!(at("2026-11-26 01:19:50"), "future");
        assert_eq!(at("2026-11-26 01:19:49"), "publish");
        assert_eq!(at(NOW_GMT), "publish");
    }

    #[test]
    fn future_in_the_past_becomes_publish() {
        let at = |gmt| insert("2026-11-25 19:00:00", gmt, "future", "post", false).status;
        assert_eq!(at("2026-11-26 01:19:49"), "publish");
        assert_eq!(at("2026-11-26 01:18:49"), "publish");
        assert_eq!(at("2026-11-26 01:19:50"), "future");
    }

    #[test]
    fn attachments_keep_their_status() {
        let dates = insert(
            "2026-11-25 19:00:00",
            "2026-11-26 01:19:50",
            "publish",
            "attachment",
            false,
        );
        assert_eq!(dates.status, "publish");
        let dates = insert(
            "2026-11-25 19:00:00",
            "2026-11-26 01:17:50",
            "future",
            "attachment",
            false,
        );
        assert_eq!(dates.status, "future");
    }

    #[test]
    fn swap_uses_the_derived_gmt() {
        // No GMT given: get_gmt_from_date() of the local date decides.
        let dates = insert("2026-11-25 19:19:50", "", "publish", "post", false);
        assert_eq!(dates.date_gmt, b"2026-11-26 01:19:50");
        assert_eq!(dates.status, "future");
        let dates = insert("2026-11-25 19:17:50", "", "future", "post", false);
        assert_eq!(dates.date_gmt, b"2026-11-26 01:17:50");
        assert_eq!(dates.status, "publish");
    }

    #[test]
    fn other_statuses_are_not_swapped() {
        for status in ["draft", "private"] {
            let dates = insert(
                "2026-11-25 19:19:50",
                "2026-11-26 01:19:50",
                status,
                "post",
                false,
            );
            assert_eq!(dates.status, status);
        }
    }

    #[test]
    fn unparsable_gmt_counts_as_zero() {
        // strtotime( 'garbage' ) is false, so false - now is far below a minute.
        let dates = insert("2026-11-25 19:00:00", "garbage", "future", "post", false);
        assert_eq!(dates.status, "publish");
        let dates = insert("2026-11-25 19:00:00", "garbage", "publish", "post", false);
        assert_eq!(dates.status, "publish");
        assert_eq!(dates.date_gmt, b"garbage");
    }
}
