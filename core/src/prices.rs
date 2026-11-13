use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
type Result<T> = std::result::Result<T, String>;
pub fn utc(s: &str) -> Result<i64> {
    let d = DateTime::parse_from_rfc3339(s).map_err(|_| "invalid UTC timestamp")?;
    if d.offset().local_minus_utc() != 0 || d.timestamp_subsec_nanos() != 0 {
        return Err("expected whole UTC seconds".into());
    }
    Ok(d.timestamp())
}
pub fn iso(t: i64) -> Result<String> {
    Ok(DateTime::<Utc>::from_timestamp(t, 0)
        .ok_or("invalid time range")?
        .to_rfc3339_opts(SecondsFormat::Secs, true))
}
pub fn price(s: &str) -> Result<i64> {
    let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("invalid price precision or format".into());
    }
    let whole: i64 = whole.parse().map_err(|_| "invalid price range")?;
    let fraction: i64 = format!("{fraction:0<6}")
        .parse()
        .map_err(|_| "invalid price fraction")?;
    let p = whole
        .checked_mul(1_000_000)
        .and_then(|n| n.checked_add(fraction))
        .ok_or("invalid price range")?;
    if p <= 0 {
        return Err("nonpositive price".into());
    }
    Ok(p)
}
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Minute {
    pub timestamp: i64,
    pub price: [i64; 4],

    pub quotes: u64,
    pub gap_before_minutes: u64,
}
pub fn parse_record(headers: &csv::StringRecord, row: &csv::StringRecord) -> Result<Minute> {
    let expected = [
        "timestamp_utc",
        "timestamp_ms",
        "bar_end_utc",
        "open",
        "high",
        "low",
        "close",
        "quote_count",
        "gap_before_minutes",
    ];
    if headers.iter().ne(expected) || row.len() != expected.len() {
        return Err("invalid index minute columns".into());
    }
    let get = |name: &str| {
        headers
            .iter()
            .position(|h| h == name)
            .and_then(|i| row.get(i))
            .ok_or_else(|| "missing CSV column".to_owned())
    };
    let timestamp = utc(get("timestamp_utc")?)?;
    let ms: i64 = get("timestamp_ms")?
        .parse()
        .map_err(|_| "invalid timestamp_ms")?;
    if timestamp.rem_euclid(60) != 0
        || timestamp.checked_mul(1000) != Some(ms)
        || timestamp.checked_add(60) != Some(utc(get("bar_end_utc")?)?)
    {
        return Err("inconsistent minute timestamps".into());
    }
    let mut ohlc = [0; 4];
    for (i, field) in ["open", "high", "low", "close"].iter().enumerate() {
        ohlc[i] = price(get(field)?)?;
    }
    let [o, h, l, c] = ohlc;
    if h < o || h < c || l > o || l > c || l > h {
        return Err("invalid OHLC".into());
    }
    let quotes = get("quote_count")?
        .parse()
        .map_err(|_| "invalid quote count")?;
    if quotes == 0 {
        return Err("empty minute".into());
    }
    Ok(Minute {
        timestamp,
        price: ohlc,

        quotes,
        gap_before_minutes: get("gap_before_minutes")?
            .parse()
            .map_err(|_| "invalid gap count")?,
    })
}
