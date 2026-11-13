//! MetaTrader 5 M1 bar export -> prepared index minutes for import-history
//! and the minute backtest. Server time is America/New_York + 7h; the index
//! price is the exported (bid) OHLC.
use chrono::{Datelike, Duration, NaiveDateTime, TimeZone, Weekday};
use chrono_tz::America::New_York;
use forex_core::prices::{iso, price};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

pub const ORIGIN: &str = "mt5_m1_bars";
pub const TIMEZONE_CONVERSION: &str = "MT5 server time = America/New_York +7h to UTC";
const HEADER: [&str; 9] = [
    "<DATE>",
    "<TIME>",
    "<OPEN>",
    "<HIGH>",
    "<LOW>",
    "<CLOSE>",
    "<TICKVOL>",
    "<VOL>",
    "<SPREAD>",
];
const CSV_HEADER: &str =
    "timestamp_utc,timestamp_ms,bar_end_utc,open,high,low,close,quote_count,gap_before_minutes\n";

fn fmt_price(p: i64) -> String {
    format!("{}.{:06}", p / 1_000_000, p % 1_000_000)
}

fn server_time(date: &str, time: &str) -> Result<(NaiveDateTime, i64)> {
    let server = NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y.%m.%d %H:%M:%S")
        .map_err(|_| "invalid date or time")?;
    if server.and_utc().timestamp().rem_euclid(60) != 0 {
        return Err("bar time is not a whole minute".into());
    }
    let local = server - Duration::hours(7);
    let utc = New_York
        .from_local_datetime(&local)
        .single()
        .ok_or("nonexistent or ambiguous server time")?
        .timestamp();
    Ok((server, utc))
}

fn is_weekend_gap(prev: NaiveDateTime, next: NaiveDateTime) -> bool {
    prev.weekday() == Weekday::Fri
        && matches!(next.weekday(), Weekday::Sun | Weekday::Mon)
        && next - prev <= Duration::days(4)
}

fn month_of(ts: i64) -> Result<String> {
    Ok(iso(ts)?[..7].to_owned())
}

pub fn prepare_mt5(export: &Path, root: &Path, symbol: &str, source: &str) -> Result<Value> {
    if !crate::valid_symbol(symbol) {
        return Err("invalid instrument".into());
    }
    if source.trim().is_empty() {
        return Err("missing data source".into());
    }
    if root.exists()
        && fs::read_dir(root)
            .map_err(|_| "cannot read output directory")?
            .next()
            .is_some()
    {
        return Err("output directory is not empty".into());
    }
    let bytes = fs::read(export).map_err(|_| "cannot read MT5 export")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "MT5 export is not UTF-8/ASCII")?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    if header != HEADER {
        return Err("unexpected MT5 export header".into());
    }

    let mut months: BTreeMap<String, String> = BTreeMap::new();
    let mut gaps = String::from("gap_start_utc,gap_end_utc,missing_minutes\n");
    let mut gap_list = Vec::new();
    let mut prev: Option<(NaiveDateTime, i64)> = None;
    let mut first = None;
    let mut rows = 0u64;
    for (n, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let at = |e: &str| format!("line {}: {e}", n + 2);
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 9 {
            return Err(at("expected 9 tab-separated fields"));
        }
        let (server, ts) = server_time(f[0], f[1]).map_err(|e| at(&e))?;
        let mut bid = [0i64; 4];
        for (i, v) in f[2..6].iter().enumerate() {
            bid[i] = price(v).map_err(|e| at(&e))?;
        }
        let [o, h, l, c] = bid;
        if h < o || h < c || l > o || l > c {
            return Err(at("invalid OHLC"));
        }
        let ticks: u64 = f[6].parse().map_err(|_| at("invalid tick volume"))?;
        if ticks == 0 {
            return Err(at("empty minute"));
        }
        let gap_before = match prev {
            None => 0,
            Some((pserver, pts)) => {
                if ts <= pts {
                    return Err(at("unordered or duplicate minute"));
                }
                let missing = ((ts - pts) / 60 - 1) as u64;
                if ts - pts > 3600 && !is_weekend_gap(pserver, server) {
                    gaps.push_str(&format!("{},{},{missing}\n", iso(pts + 60)?, iso(ts)?));
                    gap_list.push(json!({"from_ms": (pts + 60) * 1000, "to_ms": ts * 1000}));
                }
                // Minutes without ticks are absent from MT5 exports; only real
                // gaps (over 60 minutes) break candle completeness.
                if ts - pts > 3600 {
                    missing
                } else {
                    0
                }
            }
        };
        let out = months
            .entry(month_of(ts)?)
            .or_insert_with(|| CSV_HEADER.to_owned());
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{ticks},{gap_before}\n",
            iso(ts)?,
            ts * 1000,
            iso(ts + 60)?,
            fmt_price(bid[0]),
            fmt_price(bid[1]),
            fmt_price(bid[2]),
            fmt_price(bid[3]),
        ));
        first.get_or_insert(ts);
        prev = Some((server, ts));
        rows += 1;
    }
    let (first, last) = match (first, prev) {
        (Some(a), Some((_, b))) => (a, b),
        _ => return Err("no minutes in MT5 export".into()),
    };

    let mut missing_months = Vec::new();
    let (mut y, mut m): (i32, u32) = {
        let s = month_of(first)?;
        (s[..4].parse().unwrap_or(0), s[5..7].parse().unwrap_or(0))
    };
    let last_month = month_of(last)?;
    loop {
        let key = format!("{y:04}-{m:02}");
        if key > last_month {
            break;
        }
        if !months.contains_key(&key) {
            missing_months.push(key);
        }
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }

    fs::create_dir_all(root.join("prepared")).map_err(|_| "cannot create output directory")?;
    let mut outputs = serde_json::Map::new();
    let mut write = |name: String, body: &str| -> Result<()> {
        fs::write(root.join("prepared").join(&name), body)
            .map_err(|_| "cannot write prepared CSV")?;
        outputs.insert(
            name,
            json!({"bytes": body.len(), "sha256": format!("{:x}", Sha256::digest(body.as_bytes()))}),
        );
        Ok(())
    };
    for (month, body) in &months {
        write(format!("{symbol}-M1-index-{month}.csv"), body)?;
    }
    write("gaps.csv".into(), &gaps)?;
    let report = json!({
        "symbol": symbol,
        "source": source,
        "price_basis": "index",
        "timezone": "UTC",
        "origin": ORIGIN,
        "timezone_conversion": TIMEZONE_CONVERSION,
        "index_model": "exported bid OHLC used as the index price",
        "gaps": gap_list,
        "missing_months": missing_months,
        "input": {
            "file": export.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "sha256": format!("{:x}", Sha256::digest(&bytes)),
            "rows": rows,
        },
        "first_minute_utc": iso(first)?,
        "last_minute_utc": iso(last)?,
        "weekday_gaps_over_60_minutes": gap_list.len(),
        "outputs": outputs,
    });
    fs::write(
        root.join("quality-report.json"),
        serde_json::to_vec_pretty(&report).map_err(|_| "report serialization")?,
    )
    .map_err(|_| "cannot write quality report")?;
    Ok(
        json!({"rows": rows, "months": months.len(), "weekday_gaps_over_60_minutes": gap_list.len(),
        "missing_months": report["missing_months"], "first_minute_utc": report["first_minute_utc"],
        "last_minute_utc": report["last_minute_utc"]}),
    )
}
