//! Shared wall-clock candle boundaries. UTC is used for storage and event ordering.
use crate::engine::Timeframe;
use chrono::{Datelike, Duration, TimeZone, Timelike, Utc};
use chrono_tz::America::New_York;

pub fn bounds(timestamp: i64, tf: Timeframe) -> Result<(i64, i64), String> {
    let utc = Utc
        .timestamp_opt(timestamp, 0)
        .single()
        .ok_or("invalid calendar time")?;
    let date = utc.with_timezone(&New_York).date_naive();
    let mut points = Vec::new();
    // Include previous/next Sunday. Repeated local 01:00 uses its first occurrence;
    // the autumn candle is longer in UTC, never two indistinguishable local bars.
    for offset in -8..=8 {
        let day = date
            .checked_add_signed(Duration::days(offset))
            .ok_or("calendar overflow")?;
        if tf == Timeframe::W && day.weekday() != chrono::Weekday::Sun {
            continue;
        }
        let hours: &[u32] = if tf == Timeframe::H4 {
            &[1, 5, 9, 13, 17, 21]
        } else {
            &[17]
        };
        for &hour in hours {
            let local = day.and_hms_opt(hour, 0, 0).ok_or("invalid candle hour")?;
            let point = New_York
                .from_local_datetime(&local)
                .earliest()
                .ok_or("nonexistent candle boundary")?;
            points.push(point.timestamp());
        }
    }
    points.sort_unstable();
    let start = points
        .iter()
        .rev()
        .find(|&&t| t <= timestamp)
        .ok_or("missing candle start")?;
    let end = points
        .iter()
        .find(|&&t| t > timestamp)
        .ok_or("missing candle end")?;
    Ok((*start, *end))
}
pub fn entry_window(started: i64, policy: &str) -> Result<i64, String> {
    match policy {
        "plan_start_minus_24h" => started.checked_sub(86400).ok_or("time overflow".into()),
        "plan_day_open" => {
            bounds(started.checked_sub(1).ok_or("time overflow")?, Timeframe::D).map(|b| b.0)
        }
        _ => Err("unsupported entry window".into()),
    }
}

/// Quote time expected after `end`: if `end` falls inside the weekly market
/// closure (Friday from 16:00 to Sunday 17:00 America/New_York), the next
/// Sunday 17:00; otherwise `end` itself.
pub fn after_market_closure(end: i64) -> i64 {
    let Some(utc) = Utc.timestamp_opt(end, 0).single() else {
        return end;
    };
    let local = utc.with_timezone(&New_York);
    let days_to_sunday = match local.weekday() {
        chrono::Weekday::Fri if local.hour() >= 16 => 2,
        chrono::Weekday::Sat => 1,
        chrono::Weekday::Sun if local.hour() < 17 => 0,
        _ => return end,
    };
    (local.date_naive() + Duration::days(days_to_sunday))
        .and_hms_opt(17, 0, 0)
        .and_then(|l| New_York.from_local_datetime(&l).earliest())
        .map_or(end, |t| t.timestamp())
}
