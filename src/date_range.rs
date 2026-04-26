use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};

#[derive(Debug, Clone)]
pub struct DateRange {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

impl DateRange {
    pub fn parse(from: Option<&str>, to: Option<&str>) -> Result<Self> {
        let now = Local::now();
        let from_local = match from {
            Some(value) => parse_boundary(value, false)?,
            None => now.date_naive().and_hms_opt(0, 0, 0).unwrap(),
        };
        let to_local = match to {
            Some(value) => parse_boundary(value, true)?,
            None => now.naive_local(),
        };
        let from_dt = Local
            .from_local_datetime(&from_local)
            .single()
            .or_else(|| Local.from_local_datetime(&from_local).earliest())
            .context("failed to resolve from date")?
            .with_timezone(&Utc);
        let to_dt = Local
            .from_local_datetime(&to_local)
            .single()
            .or_else(|| Local.from_local_datetime(&to_local).latest())
            .context("failed to resolve to date")?
            .with_timezone(&Utc);
        Ok(Self {
            from: from_dt,
            to: to_dt,
        })
    }

    pub fn contains_millis(&self, millis: i64) -> bool {
        DateTime::<Utc>::from_timestamp_millis(millis)
            .map(|dt| dt >= self.from && dt <= self.to)
            .unwrap_or(false)
    }
}

fn parse_boundary(value: &str, end_of_day: bool) -> Result<chrono::NaiveDateTime> {
    let normalized = value.trim().to_lowercase();
    let today = Local::now().date_naive();
    let date = match normalized.as_str() {
        "today" => today,
        "yesterday" => today - Duration::days(1),
        _ => NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
            .with_context(|| format!("expected date like YYYY-MM-DD, got {value}"))?,
    };
    let time = if end_of_day {
        date.and_hms_milli_opt(23, 59, 59, 999).unwrap()
    } else {
        date.and_hms_opt(0, 0, 0).unwrap()
    };
    Ok(time)
}
