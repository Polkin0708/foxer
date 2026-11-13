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
    for tf in ["W", "D", "H4"] {
        cfg["context"]["minimum_bars"][tf] = json!(1);
    }
    cfg["instruments"].as_array_mut().unwrap().truncate(1);
    fs::write(root.join("rules.json"), cfg.to_string()).unwrap();
    let start = forex_core::prices::utc("2025-12-01T00:00:00Z").unwrap();
    let end = forex_core::prices::utc("2026-01-02T00:00:00Z").unwrap();
    let mut csv=String::from("timestamp_utc,timestamp_ms,bar_end_utc,open,high,low,close,quote_count,gap_before_minutes\n");
    for t in (start..end).step_by(60) {
        csv.push_str(&format!(
            "{},{},{},1.1,1.1,1.1,1.1,1,0\n",
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
        "EURUSD-index-202601.csv",
        zip::write::FileOptions::default(),
    )
    .unwrap();
    z.write_all(b"timestamp_ms,price\n1767312000001,1.1\n1767398400000,1.1\n")
        .unwrap();
    z.finish().unwrap();
    let hash = format!("{:x}", Sha256::digest(fs::read(&archive).unwrap()));
    fs::write(
        root.join("manifest.json"),
        json!({"files":[{"month":"2026-01","file":"raw/EURUSD-T-202601.zip","sha256":hash}]})
            .to_string(),
    )
    .unwrap();
    fs::write(root.join("quality-report.json"),json!({"source":"Synthetic test feed","timezone":"UTC","price_basis":"index","gaps":[],"outputs":{"EURUSD.csv":{"sha256":mhash,"bytes":csv.len()}}}).to_string()).unwrap();
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
    let quality = fs::read(root.join("quality-report.json")).unwrap();
    let mut no_context: serde_json::Value = serde_json::from_slice(&quality).unwrap();
    no_context["outputs"] = json!({});
    fs::write(root.join("quality-report.json"), no_context.to_string()).unwrap();
    let missing = run("missing-context");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("insufficient W bars"));
    fs::write(root.join("quality-report.json"), quality).unwrap();
    let mut z = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    z.start_file(
        "EURUSD-index-202601.csv",
        zip::write::FileOptions::default(),
    )
    .unwrap();
    z.write_all(b"timestamp_ms,price\n").unwrap();
    z.finish().unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    manifest["files"][0]["sha256"] =
        json!(format!("{:x}", Sha256::digest(fs::read(&archive).unwrap())));
    fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();
    let empty = run("missing-quotes");
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("no observed ticks"));
    fs::remove_dir_all(root).unwrap();
}
