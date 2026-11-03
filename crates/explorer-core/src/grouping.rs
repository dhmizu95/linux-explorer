//! "Group by" buckets with Explorer's names and order.

use std::time::SystemTime;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::entry::Entry;
use crate::filetype::type_description;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum GroupBy {
    #[default]
    None,
    Name,
    Modified,
    Type,
    Size,
}

impl GroupBy {
    pub const ALL: [GroupBy; 5] = [GroupBy::Name, GroupBy::Modified, GroupBy::Type, GroupBy::Size, GroupBy::None];

    pub fn label(self) -> &'static str {
        match self {
            GroupBy::None => "(None)",
            GroupBy::Name => "Name",
            GroupBy::Modified => "Date modified",
            GroupBy::Type => "Type",
            GroupBy::Size => "Size",
        }
    }
}

/// Group label and a rank used to order groups.
pub fn group_of(e: &Entry, by: GroupBy, today: NaiveDate) -> (i64, String) {
    match by {
        GroupBy::None => (0, String::new()),
        GroupBy::Name => name_group(&e.name),
        GroupBy::Type => (0, type_description(e)),
        GroupBy::Size => size_group(e),
        GroupBy::Modified => date_group(e.modified, today),
    }
}

fn name_group(name: &str) -> (i64, String) {
    let c = name.chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or(' ');
    match c {
        '0'..='9' => (0, "0 - 9".into()),
        'A'..='H' => (1, "A - H".into()),
        'I'..='P' => (2, "I - P".into()),
        'Q'..='Z' => (3, "Q - Z".into()),
        _ => (4, "Other".into()),
    }
}

/// Explorer's size buckets (Windows 10/11).
pub fn size_bucket(bytes: u64) -> (i64, &'static str) {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    match bytes {
        0 => (0, "Empty (0 KB)"),
        b if b < 16 * KB => (1, "Tiny (0 - 16 KB)"),
        b if b < MB => (2, "Small (16 KB - 1 MB)"),
        b if b < 128 * MB => (3, "Medium (1 - 128 MB)"),
        b if b < GB => (4, "Large (128 MB - 1 GB)"),
        b if b < 4 * GB => (5, "Huge (1 - 4 GB)"),
        _ => (6, "Gigantic (> 4 GB)"),
    }
}

fn size_group(e: &Entry) -> (i64, String) {
    if e.is_dir() {
        return (-1, "Folders".into());
    }
    let (r, s) = size_bucket(e.size);
    (r, s.into())
}

/// Date buckets. Rank orders newest first.
pub fn date_group(t: Option<SystemTime>, today: NaiveDate) -> (i64, String) {
    let Some(t) = t else { return (99, "Unknown".into()) };
    let d: DateTime<Local> = t.into();
    let d = d.date_naive();
    let label = date_bucket(d, today);
    let rank = DATE_BUCKETS.iter().position(|b| *b == label).unwrap_or(98) as i64;
    (rank, label.to_string())
}

const DATE_BUCKETS: [&str; 9] = [
    "In the future",
    "Today",
    "Yesterday",
    "Earlier this week",
    "Last week",
    "Earlier this month",
    "Last month",
    "Earlier this year",
    "A long time ago",
];

pub fn date_bucket(d: NaiveDate, today: NaiveDate) -> &'static str {
    if d > today {
        return "In the future";
    }
    if d == today {
        return "Today";
    }
    if d == today - Duration::days(1) {
        return "Yesterday";
    }
    // Weeks start on Sunday in Explorer's en-US locale.
    let week_start = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
    if d >= week_start {
        return "Earlier this week";
    }
    if d >= week_start - Duration::days(7) {
        return "Last week";
    }
    let month_start = today.with_day(1).unwrap();
    if d >= month_start {
        return "Earlier this month";
    }
    let last_month_start = (month_start - Duration::days(1)).with_day(1).unwrap();
    if d >= last_month_start {
        return "Last month";
    }
    if d.year() == today.year() {
        return "Earlier this year";
    }
    "A long time ago"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn date_buckets() {
        // Tuesday, 3 Nov 2026.
        let today = d(2026, 11, 3);
        assert_eq!(date_bucket(d(2026, 11, 4), today), "In the future");
        assert_eq!(date_bucket(today, today), "Today");
        assert_eq!(date_bucket(d(2026, 11, 2), today), "Yesterday");
        assert_eq!(date_bucket(d(2026, 11, 1), today), "Earlier this week"); // Sunday
        assert_eq!(date_bucket(d(2026, 10, 31), today), "Last week");
        assert_eq!(date_bucket(d(2026, 10, 25), today), "Last week");
        assert_eq!(date_bucket(d(2026, 10, 24), today), "Last month");
        assert_eq!(date_bucket(d(2026, 9, 30), today), "Earlier this year");
        assert_eq!(date_bucket(d(2025, 12, 31), today), "A long time ago");
    }

    #[test]
    fn earlier_this_month_distinct_from_last_week() {
        let today = d(2026, 11, 28); // Saturday
        assert_eq!(date_bucket(d(2026, 11, 10), today), "Earlier this month");
    }

    #[test]
    fn size_buckets() {
        assert_eq!(size_bucket(0).1, "Empty (0 KB)");
        assert_eq!(size_bucket(100).1, "Tiny (0 - 16 KB)");
        assert_eq!(size_bucket(20 * 1024).1, "Small (16 KB - 1 MB)");
        assert_eq!(size_bucket(5 << 20).1, "Medium (1 - 128 MB)");
        assert_eq!(size_bucket(200 << 20).1, "Large (128 MB - 1 GB)");
        assert_eq!(size_bucket(2 << 30).1, "Huge (1 - 4 GB)");
        assert_eq!(size_bucket(5 << 30).1, "Gigantic (> 4 GB)");
    }

    #[test]
    fn name_groups() {
        assert_eq!(name_group("apple").1, "A - H");
        assert_eq!(name_group("Kiwi").1, "I - P");
        assert_eq!(name_group("zebra").1, "Q - Z");
        assert_eq!(name_group("42").1, "0 - 9");
        assert_eq!(name_group("_x").1, "Other");
    }
}
