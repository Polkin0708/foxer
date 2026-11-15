use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const HEADER: &str =
    "<DATE>\t<TIME>\t<OPEN>\t<HIGH>\t<LOW>\t<CLOSE>\t<TICKVOL>\t<VOL>\t<SPREAD>\r\n";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forex-mt5-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("strategy")).unwrap();
        fs::write(
            root.join("strategy/fvg-bot-rules.json"),
            include_str!("../../strategy/fvg-bot-rules.json"),
        )
        .unwrap();
        Self { root }
    }
    fn export(&self, rows: &[&str]) -> PathBuf {
        let mut text = HEADER.to_owned();
        for row in rows {
            text.push_str(row);
            text.push_str("\r\n");
        }
        let path = self.root.join("export.csv");
        fs::write(&path, text).unwrap();
        path
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_forex-bot"))
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
    fn prepare(&self, rows: &[&str]) -> Output {
        let export = self.export(rows);
        self.run(&[
            "prepare-mt5",
            export.to_str().unwrap(),
            self.root.join("hist").to_str().unwrap(),
            "EURUSD",
            "MT5 broker export (test)",
        ])
    }
    fn prepared(&self, month: &str) -> String {
        fs::read_to_string(
            self.root
                .join(format!("hist/prepared/EURUSD-M1-index-{month}.csv")),
        )
        .unwrap()
    }
    fn report(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("hist/quality-report.json")).unwrap())
            .unwrap()
    }
}

fn row(date: &str, time: &str, o: &str, h: &str, l: &str, c: &str, ticks: u32) -> String {
    format!("{date}\t{time}\t{o}\t{h}\t{l}\t{c}\t{ticks}\t0\t0")
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn converts_server_time_to_utc_in_winter_and_summer() {
    let f = Fixture::new();
    let a = row(
        "2026.02.02",
        "00:00:00",
        "1.08000",
        "1.08010",
        "1.07990",
        "1.08005",
        5,
    );
    let b = row(
        "2026.07.06",
        "00:00:00",
        "1.17000",
        "1.17010",
        "1.16990",
        "1.17005",
        5,
    );
    ok(&f.prepare(&[&a, &b]));
    assert!(f
        .prepared("2026-02")
        .contains("2026-02-01T22:00:00Z,1769983200000,2026-02-01T22:01:00Z,"));
    assert!(f
        .prepared("2026-07")
        .contains("2026-07-05T21:00:00Z,1783285200000,2026-07-05T21:01:00Z,"));
    let report = f.report();
    assert_eq!(
        report["timezone_conversion"],
        "MT5 server time = America/New_York +7h to UTC"
    );
    assert_eq!(report["source"], "MT5 broker export (test)");
    assert_eq!(report["symbol"], "EURUSD");
}

#[test]
fn exported_ohlc_becomes_index_price_and_server_spread_is_ignored() {
    let f = Fixture::new();
    let mut a = row(
        "2026.02.02",
        "00:00:00",
        "1.08000",
        "1.08010",
        "1.07990",
        "1.08005",
        5,
    );
    a = a.replace("\t0\t0", "\t0\t37");
    ok(&f.prepare(&[&a]));
    assert!(f
        .prepared("2026-02")
        .contains(",1.080000,1.080100,1.079900,1.080050,5,0"));
    assert!(!f.prepared("2026-02").contains("ask"));
}

#[test]
fn short_tickless_gaps_are_not_data_loss_and_weekday_gaps_are_listed() {
    let f = Fixture::new();
    let rows = [
        row("2026.02.06", "10:00:00", "1.1", "1.1", "1.1", "1.1", 1),
        row("2026.02.06", "10:03:00", "1.1", "1.1", "1.1", "1.1", 1),
        row("2026.02.06", "12:04:00", "1.1", "1.1", "1.1", "1.1", 1),
        row("2026.02.09", "00:00:00", "1.1", "1.1", "1.1", "1.1", 1),
    ];
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    ok(&f.prepare(&refs));
    let csv = f.prepared("2026-02");
    assert!(csv.contains("2026-02-06T08:03:00Z,1770364980000,2026-02-06T08:04:00Z,1.100000,1.100000,1.100000,1.100000,1,0"));
    assert!(csv.contains("2026-02-06T10:04:00Z,1770372240000,2026-02-06T10:05:00Z,1.100000,1.100000,1.100000,1.100000,1,120"));
    assert_eq!(
        f.report()["gaps"],
        serde_json::json!([{"from_ms": 1770364980000i64 + 60_000, "to_ms": 1770372240000i64}])
    );
    let gaps = fs::read_to_string(f.root.join("hist/prepared/gaps.csv")).unwrap();
    assert_eq!(
        gaps,
        "gap_start_utc,gap_end_utc,missing_minutes\n2026-02-06T08:04:00Z,2026-02-06T10:04:00Z,120\n"
    );
}

#[test]
fn reports_months_without_any_minute() {
    let f = Fixture::new();
    let a = row("2026.01.15", "10:00:00", "1.1", "1.1", "1.1", "1.1", 1);
    let b = row("2026.03.16", "10:00:00", "1.1", "1.1", "1.1", "1.1", 1);
    ok(&f.prepare(&[&a, &b]));
    assert_eq!(f.report()["missing_months"], serde_json::json!(["2026-02"]));
}

#[test]
fn rejects_unordered_rows_bad_ohlc_empty_minutes_and_bad_header() {
    let cases: [Vec<String>; 3] = [
        vec![
            row("2026.02.04", "10:01:00", "1.1", "1.1", "1.1", "1.1", 1),
            row("2026.02.04", "10:00:00", "1.1", "1.1", "1.1", "1.1", 1),
        ],
        vec![row("2026.02.04", "10:00:00", "1.1", "1.0", "1.1", "1.1", 1)],
        vec![row("2026.02.04", "10:00:00", "1.1", "1.1", "1.1", "1.1", 0)],
    ];
    for rows in cases {
        let f = Fixture::new();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let out = f.prepare(&refs);
        assert!(!out.status.success(), "accepted invalid rows {rows:?}");
        assert!(!f.root.join("hist/quality-report.json").exists());
    }
    let f = Fixture::new();
    fs::write(f.root.join("bad.csv"), "DATE,TIME,OPEN\n").unwrap();
    let out = f.run(&[
        "prepare-mt5",
        f.root.join("bad.csv").to_str().unwrap(),
        f.root.join("hist").to_str().unwrap(),
        "EURUSD",
        "x",
    ]);
    assert!(!out.status.success());
}

#[test]
fn refuses_to_write_into_non_empty_directory_or_with_empty_source() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("hist")).unwrap();
    fs::write(f.root.join("hist/keep.txt"), "x").unwrap();
    let a = row("2026.02.04", "10:00:00", "1.1", "1.1", "1.1", "1.1", 1);
    assert!(!f.prepare(&[&a]).status.success());
    assert_eq!(
        fs::read_to_string(f.root.join("hist/keep.txt")).unwrap(),
        "x"
    );
    let g = Fixture::new();
    let export = g.export(&[&a]);
    let out = g.run(&[
        "prepare-mt5",
        export.to_str().unwrap(),
        g.root.join("hist").to_str().unwrap(),
        "EURUSD",
        " ",
    ]);
    assert!(!out.status.success());
}

#[test]
fn prepared_history_imports_builds_ny_candles_and_reimport_is_idempotent() {
    let f = Fixture::new();
    let a = row(
        "2026.02.02",
        "00:00:00",
        "1.08000",
        "1.08010",
        "1.07990",
        "1.08005",
        5,
    );
    let b = row(
        "2026.02.02",
        "03:59:00",
        "1.08005",
        "1.08100",
        "1.08000",
        "1.08050",
        5,
    );
    let c = row(
        "2026.02.02",
        "04:00:00",
        "1.08050",
        "1.08060",
        "1.08040",
        "1.08055",
        5,
    );
    ok(&f.prepare(&[&a, &b, &c]));
    let db = f.root.join("state.sqlite");
    let import = || {
        f.run(&[
            "import-history",
            db.to_str().unwrap(),
            f.root.join("hist").to_str().unwrap(),
        ])
    };
    let first = import();
    ok(&first);
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["inserted_minutes"], 3);
    assert_eq!(first["source"], "MT5 broker export (test)");
    let second = import();
    ok(&second);
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["inserted_minutes"], 0);
    let out = f.run(&["inspect-db", db.to_str().unwrap(), "2026-02-02T03:00:00Z"]);
    ok(&out);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let h4 = &v["bars"]["H4"][0];
    assert_eq!(h4["start_utc"], "2026-02-01T22:00:00Z");
    assert_eq!(h4["end_utc"], "2026-02-02T02:00:00Z");
    assert_eq!(h4["open"], 1_080_000);
    assert_eq!(h4["high"], 1_081_000);
    assert_eq!(h4["close"], 1_080_500);
    assert_eq!(h4["observed_minutes"], 2);
}
