use sha2::{Digest, Sha256};
use std::{fs, io::Write};
fn fixture(name: &str, body: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("forex-{}-{name}.zip", std::process::id()));
    let mut z = zip::ZipWriter::new(fs::File::create(&p).unwrap());
    z.start_file(
        "GBPUSD-index-202601.csv",
        zip::write::FileOptions::default(),
    )
    .unwrap();
    z.write_all(format!("timestamp_ms,price\n{body}").as_bytes())
        .unwrap();
    z.finish().unwrap();
    p
}
#[test]
fn reads_index_milliseconds_deduplicates_and_quarantines() {
    let p=fixture("good","1767333600001,1.20000\n1767333600001,1.20000\n1767333600002,1.20001\n1767333660000,1.20000\n");
    let hash = format!("{:x}", Sha256::digest(fs::read(&p).unwrap()));
    let start = forex_core::prices::utc("2026-01-02T06:00:00Z").unwrap() * 1000;
    let mut ticks = Vec::new();
    forex_backtest::history::read_archive(
        &p,
        "GBPUSD",
        "Synthetic archive",
        &hash,
        &[(start + 60000, start + 120000)],
        |t| {
            ticks.push(t);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ticks.len(), 2);
    assert_eq!(ticks[0].source, "Synthetic archive");
    assert_eq!(ticks[0].millis, start + 1);
    assert_eq!(ticks[1].millis, start + 2);
    assert_eq!(serde_json::to_value(&ticks[0]).unwrap()["price"], 1200000);
    fs::remove_file(p).unwrap();
}
#[test]
fn checksum_and_reversed_quotes_fail() {
    let p = fixture("bad", "1767333600002,1.2\n1767333600001,1.2\n");
    assert!(forex_backtest::history::read_archive(
        &p,
        "GBPUSD",
        "Synthetic archive",
        "wrong",
        &[],
        |_| Ok(())
    )
    .is_err());
    let hash = format!("{:x}", Sha256::digest(fs::read(&p).unwrap()));
    assert!(forex_backtest::history::read_archive(
        &p,
        "GBPUSD",
        "Synthetic archive",
        &hash,
        &[],
        |_| Ok(())
    )
    .is_err());
    fs::remove_file(p).unwrap();
}

#[test]
fn rejects_missing_or_mixed_provenance() {
    use forex_backtest::history::Dataset;
    use serde_json::json;
    let root = std::env::temp_dir().join(format!("provenance-{}", std::process::id()));
    let a = root.join("a");
    let b = root.join("b");
    for path in [&a, &b] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("manifest.json"), r#"{"files":[]}"#).unwrap();
    }
    let write = |path: &std::path::Path, source: serde_json::Value| {
        fs::write(
            path.join("quality-report.json"),
            json!({"source":source,"timezone":"UTC","price_basis":"index","gaps":[]}).to_string(),
        )
        .unwrap();
    };
    for source in [json!(null), json!(""), json!("  ")] {
        write(&a, source);
        assert_eq!(
            Dataset::open(vec![a.clone()], "EURUSD", 0, 1)
                .err()
                .unwrap(),
            "missing data source"
        );
    }
    write(&a, json!("Source A"));
    write(&b, json!("Source B"));
    assert_eq!(
        Dataset::open(vec![a, b], "EURUSD", 0, 1).err().unwrap(),
        "mixed history sources for instrument"
    );
    fs::remove_dir_all(root).unwrap();
}
