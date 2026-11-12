use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, io::Write, process::Command};
#[test]
fn offline_run_is_reproducible_and_refuses_overwrite() {
    let root = std::env::temp_dir().join(format!("forex-cli-{}", std::process::id()));
    fs::create_dir_all(root.join("raw")).unwrap();
    fs::create_dir_all(root.join("prepared")).unwrap();
    let mut cfg: serde_json::Value =
        serde_json::from_slice(&fs::read("../strategy/fvg-bot-rules.json").unwrap()).unwrap();
    cfg["candles"]["price_basis"] = json!("bid"); // The synthetic archive contains Bid/Ask.
    for tf in ["W", "D", "H4"] {
        cfg["context"]["minimum_bars"][tf] = json!(1);
    }
    cfg["instruments"].as_array_mut().unwrap().truncate(1);
    fs::write(root.join("rules.json"), cfg.to_string()).unwrap();
    let start = forex_core::prices::utc("2025-12-01T00:00:00Z").unwrap();
    let end = forex_core::prices::utc("2026-01-02T00:00:00Z").unwrap();
    let mut csv=String::from("timestamp_utc,timestamp_ms,bar_end_utc,bid_open,bid_high,bid_low,bid_close,ask_open,ask_high,ask_low,ask_close,quote_count,gap_before_minutes\n");
    for t in (start..end).step_by(60) {
        csv.push_str(&format!(
            "{},{},{},1.1,1.1,1.1,1.1,1.10001,1.10001,1.10001,1.10001,1,0\n",
            forex_core::prices::iso(t).unwrap(),
            t * 1000,
            forex_core::prices::iso(t + 60).unwrap()
        ));
    }
    fs::write(root.join("prepared/EURUSD.csv"), &csv).unwrap();
    let mhash = format!("{:x}", Sha256::digest(csv.as_bytes()));
    let archive = root.join("raw/EURUSD-T-202601.zip");
    let mut z = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    z.start_file(
        "DAT_ASCII_EURUSD_T_202601.csv",
        zip::write::FileOptions::default(),
    )
    .unwrap();
    z.write_all(b"20260101 190000001,1.1,1.10001,0\n20260102 190000000,1.1,1.10001,0\n")
        .unwrap();
    z.finish().unwrap();
    let hash = format!("{:x}", Sha256::digest(fs::read(&archive).unwrap()));
    fs::write(
        root.join("manifest.json"),
        json!({"files":[{"month":"2026-01","file":"raw/EURUSD-T-202601.zip","sha256":hash}]})
            .to_string(),
    )
    .unwrap();
    fs::write(root.join("quality-report.json"),json!({"source":"Synthetic test feed","timezone_conversion":"fixed EST UTC-05:00 to UTC; no DST","timestamp_reversals":[],"outputs":{"EURUSD.csv":{"sha256":mhash,"bytes":csv.len()}}}).to_string()).unwrap();
    fs::write(
        root.join("sources.json"),
        json!({"EURUSD":[root.to_str().unwrap()]}).to_string(),
    )
    .unwrap();
    let run = |name: &str| {
        Command::new(env!("CARGO_BIN_EXE_forex-backtest"))
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
                root.join(name).to_str().unwrap(),
                "--no-pdf",
            ])
            .output()
            .unwrap()
    };
    let a = run("a");
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    let b = run("b");
    assert!(b.status.success());
    assert_eq!(
        fs::read(root.join("a/results.json")).unwrap(),
        fs::read(root.join("b/results.json")).unwrap()
    );
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("a/results.json")).unwrap()).unwrap();
    assert_eq!(result["summary"]["closed"], 0);
    assert_eq!(
        result["sources"]["EURUSD"]["files"][0]["source"],
        "Synthetic test feed"
    );
    assert_eq!(result["coverage"]["EURUSD"]["ticks"], 1);
    assert!(!run("a").status.success());
    assert!(root.join("a/strategy.json").exists());
    fs::remove_dir_all(root).unwrap();
}
