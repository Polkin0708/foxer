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

/// End of the market week: if `end` is Friday 17:00 New York, the next expected
/// quote time is Sunday 17:00 New York; otherwise `end` itself.
pub fn after_market_closure(end: i64) -> i64 {
    let Some(local) = Utc.timestamp_opt(end, 0).single() else {
        return end;
    };
    let local = local.with_timezone(&New_York);
    if local.weekday() != chrono::Weekday::Fri || local.hour() != 17 || local.minute() != 0 {
        return end;
    }
    let sunday = local.date_naive() + Duration::days(2);
    sunday
        .and_hms_opt(17, 0, 0)
        .and_then(|l| New_York.from_local_datetime(&l).earliest())
        .map_or(end, |t| t.timestamp())
}
