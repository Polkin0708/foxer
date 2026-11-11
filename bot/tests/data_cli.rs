use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forex-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("prepared")).unwrap();
        fs::create_dir_all(root.join("strategy")).unwrap();
        fs::write(
            root.join("strategy/fvg-bot-rules.json"),
            include_str!("../../strategy/fvg-bot-rules.json"),
        )
        .unwrap();
        Self { root }
    }
    fn prepare(&self, rows: &[(&str, i64, &str, &str)]) {
        let mut csv="timestamp_utc,timestamp_ms,bar_end_utc,bid_open,bid_high,bid_low,bid_close,ask_open,ask_high,ask_low,ask_close,quote_count,gap_before_minutes\n".to_owned();
        for (time, ms, end, price) in rows {
            csv.push_str(&format!("{time},{ms},{end},{price},{price},{price},{price},1.200000,1.200000,1.200000,1.200000,1,0\n"));
        }
        fs::write(self.root.join("prepared/EURUSD-M1-bid-ask-2026.csv"), &csv).unwrap();
        let report = json!({"source":"HistData Generic ASCII Tick","timezone_conversion":"fixed EST UTC-05:00 to UTC; no DST","timestamp_reversals":[],"missing_months":[],"outputs":{"EURUSD-M1-bid-ask-2026.csv":{"bytes":csv.len(),"sha256":format!("{:x}",Sha256::digest(csv.as_bytes()))}}});
        fs::write(self.root.join("quality-report.json"), report.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_forex-bot"))
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
    fn import(&self) -> Output {
        self.run(&[
            "import-histdata",
            self.root.join("state.sqlite").to_str().unwrap(),
            self.root.to_str().unwrap(),
        ])
    }
    fn inspect(&self, time: &str) -> Value {
        let out = self.run(&[
            "inspect-db",
            self.root.join("state.sqlite").to_str().unwrap(),
            time,
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn rows() -> Vec<(&'static str, i64, &'static str, &'static str)> {
    vec![
        (
            "2026-01-04T21:59:00Z",
            1767563940000,
            "2026-01-04T22:00:00Z",
            "1.100001",
        ),
        (
            "2026-01-04T22:00:00Z",
            1767564000000,
            "2026-01-04T22:01:00Z",
            "1.110001",
        ),
        (
            "2026-01-05T01:59:00Z",
            1767578340000,
            "2026-01-05T02:00:00Z",
            "1.120001",
        ),
    ]
}
#[test]
fn import_survives_restart_is_idempotent_and_keeps_exact_prices() {
    let f = Fixture::new();
    f.prepare(&rows());
    let out = f.import();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(f.import().status.success());
    let r = f.inspect("2026-01-12T22:00:00Z");
    assert_eq!(r["m1_count"], 3);
    assert_eq!(r["imports"].as_array().unwrap().len(), 1);
    let h = r["bars"]["H4"].as_array().unwrap();
    assert_eq!(h.len(), 2);
    assert_eq!(h[1]["open"], 1110001);
    assert_eq!(h[1]["close"], 1120001);
    assert_eq!(h[1]["observed_minutes"], 2);
    assert_eq!(h[1]["source"], "HistData Generic ASCII Tick");
}
#[test]
fn midnight_weekly_and_h4_boundaries_do_not_look_ahead() {
    let f = Fixture::new();
    f.prepare(&rows());
    assert!(f.import().status.success());
    let r = f.inspect("2026-01-04T22:00:00Z");
    for tf in ["H4", "D", "W"] {
        let a = r["bars"][tf].as_array().unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["close"], 1100001);
        assert_eq!(a[0]["end_utc"], "2026-01-04T22:00:00Z");
    }
    let r = f.inspect("2026-01-05T01:59:59Z");
    assert_eq!(r["bars"]["H4"].as_array().unwrap().len(), 1);
    let r = f.inspect("2026-01-05T02:00:00Z");
    assert_eq!(r["bars"]["H4"].as_array().unwrap().len(), 2);
}
#[test]
fn summer_uses_new_york_dst_grid() {
    let f = Fixture::new();
    f.prepare(&[(
        "2026-07-05T20:59:00Z",
        1783285140000,
        "2026-07-05T21:00:00Z",
        "1.100001",
    )]);
    assert!(f.import().status.success());
    let r = f.inspect("2026-07-05T21:00:00Z");
    for tf in ["H4", "D", "W"] {
        assert_eq!(r["bars"][tf][0]["end_utc"], "2026-07-05T21:00:00Z");
    }
}
#[test]
fn checksum_failure_does_not_change_existing_database() {
    let f = Fixture::new();
    f.prepare(&rows());
    assert!(f.import().status.success());
    fs::write(
        f.root.join("prepared/EURUSD-M1-bid-ask-2026.csv"),
        "corrupt",
    )
    .unwrap();
    assert_eq!(f.import().status.code(), Some(1));
    assert_eq!(f.inspect("2026-01-12T22:00:00Z")["m1_count"], 3);
}
#[test]
fn conflicting_overlap_rolls_back_entire_import() {
    let f = Fixture::new();
    let original = rows();
    f.prepare(&original[1..2]);
    assert!(f.import().status.success());
    let mut changed = rows();
    changed[1].3 = "1.119999";
    f.prepare(&changed);
    assert_eq!(f.import().status.code(), Some(1));
    assert_eq!(f.inspect("2026-01-12T22:00:00Z")["m1_count"], 1);
}
#[test]
fn invalid_time_order_and_price_are_not_repaired() {
    for mode in 0..4 {
        let f = Fixture::new();
        let mut bad = rows();
        match mode {
            0 => bad[1].1 += 1,
            1 => bad.reverse(),
            2 => bad[1].3 = "1.1000001",
            _ => bad[1].3 = "1.300000",
        };
        f.prepare(&bad);
        assert_eq!(f.import().status.code(), Some(1));
    }
}
#[test]
fn future_schema_is_not_overwritten() {
    let f = Fixture::new();
    let conn = rusqlite::Connection::open(f.root.join("state.sqlite")).unwrap();
    conn.execute_batch("PRAGMA user_version=99;").unwrap();
    drop(conn);
    f.prepare(&rows());
    assert_eq!(f.import().status.code(), Some(1));
    let conn = rusqlite::Connection::open(f.root.join("state.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        99
    );
}
#[test]
fn imports_same_timestamp_for_different_pairs_without_mixing_history() {
    let f = Fixture::new();
    f.prepare(&rows());
    assert!(f.import().status.success());
    let prepared = f.root.join("prepared");
    let original = fs::read(prepared.join("EURUSD-M1-bid-ask-2026.csv")).unwrap();
    let mut report: Value =
        serde_json::from_slice(&fs::read(f.root.join("quality-report.json")).unwrap()).unwrap();
    let metadata = report["outputs"]["EURUSD-M1-bid-ask-2026.csv"].clone();
    fs::write(prepared.join("GBPUSD-M1-bid-ask-2026.csv"), original).unwrap();
    report["symbol"] = json!("GBPUSD");
    report["outputs"] = json!({"GBPUSD-M1-bid-ask-2026.csv":metadata});
    fs::write(f.root.join("quality-report.json"), report.to_string()).unwrap();
    let db = f.root.join("state.sqlite");
    assert!(forex_bot::data::import_histdata_symbol(&db, &f.root, "USDJPY").is_err());
    assert_eq!(
        forex_bot::data::import_histdata_symbol(&db, &f.root, "GBPUSD").unwrap()
            ["inserted_minutes"],
        3
    );
    assert_eq!(
        forex_bot::data::import_histdata_symbol(&db, &f.root, "GBPUSD").unwrap()
            ["inserted_minutes"],
        0
    );
    let cfg = forex_bot::load(&f.root.join("strategy/fvg-bot-rules.json")).unwrap();
    for symbol in ["EURUSD", "GBPUSD"] {
        let v = forex_bot::data::inspect_symbol(&db, "2026-01-12T22:00:00Z", &cfg, symbol).unwrap();
        assert_eq!(v["m1_count"], 3);
        assert_eq!(v["imports"].as_array().unwrap().len(), 1);
    }
    assert_eq!(
        forex_bot::data::inspect_symbol(&db, "2026-01-12T22:00:00Z", &cfg, "USDJPY").unwrap()
            ["m1_count"],
        0
    );
}
