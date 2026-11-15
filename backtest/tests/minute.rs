use forex_backtest::history::{intrabar_order, synthetic_ticks, Order};
use forex_core::prices::Minute;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, process::Command};

const T: i64 = 1_770_192_000; // 2026-02-04T08:00:00Z

fn minute(o: i64, h: i64, l: i64, c: i64) -> Minute {
    Minute {
        timestamp: T,
        price: [o, h, l, c],
        quotes: 1,
        gap_before_minutes: 0,
    }
}

fn trade(side: &str, state: &str, entry: i64, stop: i64, tp: i64) -> Value {
    json!({"symbol":"EURUSD","side":side,"state":state,"entry":entry,"stop":stop,"take_profit":tp})
}

#[test]
fn four_index_quotes_inside_the_minute() {
    let m = minute(1_100_000, 1_100_500, 1_099_000, 1_100_200);
    let ticks = synthetic_ticks(&m, "EURUSD", "src", Order::LowFirst);
    let times: Vec<_> = ticks.iter().map(|t| t.millis - T * 1000).collect();
    assert_eq!(times, [0, 20_000, 40_000, 59_000]);
    let prices: Vec<_> = ticks.iter().map(|t| t.price).collect();
    assert_eq!(prices, [1_100_000, 1_099_000, 1_100_500, 1_100_200]);
    assert!(ticks.iter().all(|t| t.source == "src"));
    let high_first: Vec<_> = synthetic_ticks(&m, "EURUSD", "src", Order::HighFirst)
        .iter()
        .map(|t| t.price)
        .collect();
    assert_eq!(high_first, [1_100_000, 1_100_500, 1_099_000, 1_100_200]);
}

#[test]
fn active_long_with_reachable_stop_goes_low_first() {
    let m = minute(1_100_000, 1_102_000, 1_098_000, 1_101_000);
    let trades = [trade("Long", "Active", 1_100_000, 1_099_000, 1_101_500)];
    assert_eq!(
        intrabar_order(&m, &trades, "EURUSD"),
        (Order::LowFirst, false)
    );
}

#[test]
fn active_short_with_reachable_stop_goes_high_first() {
    let m = minute(1_100_000, 1_101_050, 1_098_000, 1_099_000);
    let trades = [trade("Short", "Active", 1_100_000, 1_101_000, 1_098_500)];
    assert_eq!(
        intrabar_order(&m, &trades, "EURUSD"),
        (Order::HighFirst, false)
    );
}

#[test]
fn pending_long_prefers_cancel_when_only_target_is_reachable() {
    let m = minute(1_100_000, 1_102_000, 1_098_000, 1_101_000);
    let trades = [trade("Long", "Pending", 1_099_000, 1_090_000, 1_101_500)];
    assert_eq!(
        intrabar_order(&m, &trades, "EURUSD"),
        (Order::HighFirst, false)
    );
    let both = [trade("Long", "Pending", 1_099_000, 1_098_500, 1_101_500)];
    assert_eq!(
        intrabar_order(&m, &both, "EURUSD"),
        (Order::LowFirst, false)
    );
}

#[test]
fn untouched_or_foreign_signals_keep_natural_order_and_conflicts_are_flagged() {
    let up = minute(1_100_000, 1_100_500, 1_099_500, 1_100_400);
    let down = minute(1_100_000, 1_100_500, 1_099_500, 1_099_600);
    let far = [
        trade("Long", "Active", 1_100_000, 1_090_000, 1_120_000),
        json!({"symbol":"GBPUSD","side":"Long","state":"Active","entry":1_100_000,"stop":1_099_900,"take_profit":1_200_000}),
        trade("Long", "Stop", 1_100_000, 1_099_900, 1_100_100),
    ];
    assert_eq!(
        intrabar_order(&up, &far, "EURUSD"),
        (Order::LowFirst, false)
    );
    assert_eq!(
        intrabar_order(&down, &far, "EURUSD"),
        (Order::HighFirst, false)
    );
    let mixed = [
        trade("Long", "Active", 1_100_000, 1_099_600, 1_110_000),
        trade("Short", "Active", 1_100_000, 1_100_300, 1_090_000),
    ];
    assert_eq!(
        intrabar_order(&down, &mixed, "EURUSD"),
        (Order::LowFirst, true)
    );
}

#[test]
fn backtest_runs_on_mt5_minutes_without_tick_archives() {
    let root = std::env::temp_dir().join(format!("forex-minute-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("prepared")).unwrap();
    let mut cfg: Value =
        serde_json::from_slice(&fs::read("../strategy/fvg-bot-rules.json").unwrap()).unwrap();
    for tf in ["W", "D", "H4"] {
        cfg["context"]["minimum_bars"][tf] = json!(1);
    }
    cfg["instruments"].as_array_mut().unwrap().truncate(1);
    fs::write(root.join("rules.json"), cfg.to_string()).unwrap();
    let start = forex_core::prices::utc("2025-12-01T00:00:00Z").unwrap();
    let end = forex_core::prices::utc("2026-01-03T00:00:00Z").unwrap();
    let mut outputs = serde_json::Map::new();
    let mut month = String::new();
    let mut csv = String::new();
    let header = "timestamp_utc,timestamp_ms,bar_end_utc,open,high,low,close,quote_count,gap_before_minutes\n";
    let mut flush = |month: &str, csv: &str| {
        if csv.is_empty() {
            return;
        }
        let name = format!("EURUSD-M1-index-{month}.csv");
        fs::write(root.join("prepared").join(&name), csv).unwrap();
        outputs.insert(
            name,
            json!({"bytes":csv.len(),"sha256":format!("{:x}",Sha256::digest(csv.as_bytes()))}),
        );
    };
    for t in (start..end).step_by(60) {
        let iso = forex_core::prices::iso(t).unwrap();
        if iso[..7] != month {
            flush(&month, &csv);
            month = iso[..7].to_owned();
            csv = header.to_owned();
        }
        csv.push_str(&format!(
            "{iso},{},{},1.100000,1.100200,1.099800,1.100100,3,0\n",
            t * 1000,
            forex_core::prices::iso(t + 60).unwrap()
        ));
    }
    flush(&month, &csv);
    fs::write(
        root.join("quality-report.json"),
        json!({"symbol":"EURUSD","source":"MT5 test export","price_basis":"index","timezone":"UTC","origin":"mt5_m1_bars","gaps":[],"missing_months":[],"outputs":outputs}).to_string(),
    )
    .unwrap();
    fs::write(
        root.join("sources.json"),
        json!({"EURUSD":[root.to_str().unwrap()]}).to_string(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_forex-backtest"))
        .args([
            "--strategy",
            root.join("rules.json").to_str().unwrap(),
            "--data-config",
            root.join("sources.json").to_str().unwrap(),
            "--from",
            "2026-01-02",
            "--to",
            "2026-01-03",
            "--output",
            root.join("out").to_str().unwrap(),
            "--no-pdf",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(root.join("out/results.json")).unwrap()).unwrap();
    assert_eq!(result["coverage"]["EURUSD"]["ticks"], 4 * 1440);
    assert_eq!(result["coverage"]["EURUSD"]["order_conflicts"], 0);
    assert!(result["execution"]["intrabar"]
        .as_str()
        .unwrap()
        .contains("synthetic"));
    assert_eq!(
        result["sources"]["EURUSD"]["files"][0]["source"],
        "MT5 test export"
    );
    fs::remove_dir_all(root).unwrap();
}
