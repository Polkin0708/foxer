//! Persistent, source-labelled historical context. No historical trade replay.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const APP_ID: i64 = 0x46584731;
type Result<T> = std::result::Result<T, String>;
fn db_error(_: rusqlite::Error) -> String {
    "database operation failed".into()
}
pub fn open(path: &Path) -> Result<Connection> {
    let db = Connection::open(path).map_err(db_error)?;
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_error)?;
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_error)?;
    let app: i64 = db
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(db_error)?;
    if app == APP_ID && version == 4 {
        return Ok(db);
    }
    let tables: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if version != 0 || app != 0 || tables != 0 {
        return Err("unsupported or foreign database schema".into());
    }
    db.execute_batch("BEGIN;
    CREATE TABLE entities(seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,key TEXT NOT NULL,payload TEXT NOT NULL,UNIQUE(kind,key));
    CREATE TABLE imports(symbol TEXT NOT NULL,digest TEXT NOT NULL,report TEXT NOT NULL,PRIMARY KEY(symbol,digest));
    CREATE TABLE inbox(id TEXT PRIMARY KEY,payload TEXT NOT NULL);
    CREATE TABLE minutes(symbol TEXT NOT NULL,source TEXT NOT NULL,timestamp INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(symbol,source,timestamp));
    CREATE TABLE outbox(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,payload TEXT NOT NULL,delivered INTEGER NOT NULL DEFAULT 0 CHECK(delivered IN (0,1)));
    PRAGMA application_id=1180190513; PRAGMA user_version=4; COMMIT;").map_err(db_error)?;
    Ok(db)
}
pub use forex_core::prices::{iso, parse_record, price, utc, Minute};
pub fn import_history(path: &Path, root: &Path) -> Result<Value> {
    import_history_symbol(path, root, "EURUSD")
}
pub fn import_history_symbol(path: &Path, root: &Path, symbol: &str) -> Result<Value> {
    if !crate::valid_symbol(symbol) {
        return Err("invalid instrument".into());
    }
    let report_bytes =
        fs::read(root.join("quality-report.json")).map_err(|_| "cannot read quality report")?;
    let report: Value =
        serde_json::from_slice(&report_bytes).map_err(|_| "invalid quality report")?;
    if report
        .get("symbol")
        .and_then(Value::as_str)
        .unwrap_or("EURUSD")
        != symbol
    {
        return Err("history symbol mismatch".into());
    }
    let source = report["source"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("missing data source")?;
    if report["timezone"] != "UTC"
        || report["price_basis"] != "index"
        || !report["gaps"].is_array()
        || !report["missing_months"].is_array()
    {
        return Err("unsupported source or missing quality information".into());
    }
    let outputs = report["outputs"]
        .as_object()
        .ok_or("missing output checksums")?;
    let mut files = Vec::new();
    for (name, meta) in outputs {
        if name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || !(name == "gaps.csv"
                || (name.starts_with(&format!("{symbol}-M1-index-")) && name.ends_with(".csv")))
        {
            return Err("invalid prepared filename".into());
        }
        let bytes =
            fs::read(root.join("prepared").join(name)).map_err(|_| "cannot read prepared CSV")?;
        if meta["bytes"].as_u64() != Some(bytes.len() as u64)
            || meta["sha256"].as_str() != Some(&format!("{:x}", Sha256::digest(&bytes)))
        {
            return Err("prepared CSV checksum mismatch".into());
        }
        // Parse exactly the bytes whose checksum was verified, not a second read.
        if name != "gaps.csv" {
            files.push(bytes)
        }
    }
    if files.is_empty() {
        return Err("no prepared M1 files".into());
    }
    let mut db = open(path)?;
    let tx = db.transaction().map_err(db_error)?;
    let mut inserted = 0;
    for bytes in files {
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        let headers = reader.headers().map_err(|_| "invalid CSV header")?.clone();
        let mut previous = None;
        for record in reader.records() {
            let m = parse_record(&headers, &record.map_err(|_| "invalid CSV record")?)?;
            if previous.is_some_and(|p| m.timestamp <= p) {
                return Err("unordered or duplicate minute".into());
            }
            previous = Some(m.timestamp);
            let payload = serde_json::to_string(&m).map_err(|_| "minute serialization")?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT payload FROM minutes WHERE symbol=?1 AND source=?2 AND timestamp=?3",
                    params![symbol, source, m.timestamp],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if let Some(old) = old {
                if old != payload {
                    return Err("conflicting existing minute".into());
                }
            } else {
                tx.execute(
                    "INSERT INTO minutes VALUES(?1,?2,?3,?4)",
                    params![symbol, source, m.timestamp, payload],
                )
                .map_err(db_error)?;
                inserted += 1;
            }
        }
    }
    let digest = format!("{:x}", Sha256::digest(&report_bytes));
    tx.execute(
        "INSERT OR IGNORE INTO imports VALUES(?1,?2,?3)",
        params![symbol, digest, report.to_string()],
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(json!({"inserted_minutes":inserted,"source":source,"quality_report_sha256":digest}))
}
#[derive(Debug, Serialize)]
pub struct Bar {
    pub source: String,
    pub start_utc: String,
    pub end_utc: String,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
    pub observed_minutes: u64,
}
/// Constant UTC+3 grid. Sunday 22:00 UTC is Monday 01:00 Moscow.
pub fn bucket(timestamp: i64, seconds: i64, anchor: i64) -> i64 {
    (timestamp - anchor).div_euclid(seconds) * seconds + anchor
}
pub fn inspect(path: &Path, as_of: &str, strategy: &crate::model::Strategy) -> Result<Value> {
    inspect_symbol(path, as_of, strategy, "EURUSD")
}
pub fn inspect_symbol(
    path: &Path,
    as_of: &str,
    strategy: &crate::model::Strategy,
    symbol: &str,
) -> Result<Value> {
    if !path.is_file() {
        return Err("database does not exist".into());
    }
    let now = utc(as_of)?;
    let db = open(path)?;
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM minutes WHERE symbol=?1",
            [symbol],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let mut statement = db
        .prepare("SELECT source,payload FROM minutes WHERE timestamp<=?1 AND symbol=?2 ORDER BY source,timestamp")
        .map_err(db_error)?;
    let mut rows = statement
        .query(params![
            now.checked_sub(60).ok_or("invalid time range")?,
            symbol
        ])
        .map_err(db_error)?;
    let mut aggregates: [BTreeMap<(String, i64), Bar>; 3] =
        std::array::from_fn(|_| BTreeMap::new());
    let mut intervals = [(i64::MAX, i64::MIN); 3];
    while let Some(row) = rows.next().map_err(db_error)? {
        let source: String = row.get(0).map_err(db_error)?;
        let payload: String = row.get(1).map_err(db_error)?;
        let minute: Minute = serde_json::from_str(&payload).map_err(|_| "corrupt stored minute")?;
        for (i, tf) in [
            crate::engine::Timeframe::H4,
            crate::engine::Timeframe::D,
            crate::engine::Timeframe::W,
        ]
        .into_iter()
        .enumerate()
        {
            if minute.timestamp < intervals[i].0 || minute.timestamp >= intervals[i].1 {
                intervals[i] = crate::calendar::bounds(minute.timestamp, tf)?;
            }
            let (start, end) = intervals[i];
            if end > now {
                continue;
            }
            let key = (source.clone(), start);
            if let Some(bar) = aggregates[i].get_mut(&key) {
                bar.high = bar.high.max(minute.price[1]);
                bar.low = bar.low.min(minute.price[2]);
                bar.close = minute.price[3];
                bar.observed_minutes += 1;
            } else {
                aggregates[i].insert(
                    key,
                    Bar {
                        source: source.clone(),
                        start_utc: iso(start)?,
                        end_utc: iso(end)?,
                        open: minute.price[0],
                        high: minute.price[1],
                        low: minute.price[2],
                        close: minute.price[3],
                        observed_minutes: 1,
                    },
                );
            }
        }
    }
    let mut bars = BTreeMap::new();
    for ((name, limit), a) in [
        ("H4", strategy.context.bars_analyzed.h4),
        ("D", strategy.context.bars_analyzed.d),
        ("W", strategy.context.bars_analyzed.w),
    ]
    .into_iter()
    .zip(aggregates)
    {
        let skip = a
            .len()
            .saturating_sub(usize::try_from(limit).unwrap_or(usize::MAX));
        bars.insert(name, a.into_values().skip(skip).collect::<Vec<_>>());
    }
    let mut stmt = db
        .prepare("SELECT digest,report FROM imports WHERE symbol=?1 ORDER BY digest")
        .map_err(db_error)?;
    let imports = stmt
        .query_map([symbol], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(db_error)?
        .map(|r| {
            let (digest, report) = r.map_err(db_error)?;
            let report: Value =
                serde_json::from_str(&report).map_err(|_| "corrupt stored quality report")?;
            Ok(json!({"sha256":digest,"report":report}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"symbol":symbol,"schema_version":4,"price_scale":1000000,"timezone":"UTC","m1_count":count,"bars":bars,"imports":imports}),
    )
}
