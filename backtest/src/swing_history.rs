//! Swing rules over prepared minutes, with bars from the bot's own aggregator.
use crate::history::{json_file, read_minutes};
use forex_core::{
    engine::Timeframe,
    feed::Aggregator,
    swing::{grid_time, rules_bars, Bar, Swing, Tf},
    trading::Candle,
};
use std::path::{Path, PathBuf};

/// Aggregator bars converted for the rules and ordered by close time, then W, D, H4.
pub fn to_rules(bars: &[(Timeframe, Candle, bool)]) -> Vec<(Tf, Bar)> {
    rules_bars(bars)
}
pub fn run(root: &Path, symbol: &str, config: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("output already exists".into());
    }
    let quality = json_file(&root.join("quality-report.json"))?;
    let source = quality["source"]
        .as_str()
        .ok_or("missing source")?
        .to_owned();
    let mut files: Vec<(PathBuf, serde_json::Value)> = quality["outputs"]
        .as_object()
        .ok_or("missing prepared outputs")?
        .iter()
        .filter(|(name, _)| {
            name.starts_with(symbol) && name.ends_with(".csv") && name.contains("M1")
        })
        .map(|(name, meta)| (root.join("prepared").join(name), meta.clone()))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let cfg =
        serde_json::from_slice(&std::fs::read(config).map_err(|_| "cannot read swing config")?)
            .map_err(|e| format!("invalid swing config: {e}"))?;
    let mut swing = Swing::new(cfg);
    let mut aggregator = Aggregator::default();
    read_minutes(&files, i64::MIN, i64::MAX, |m| {
        let bars = aggregator.minute_all(symbol, &source, &m)?;
        let now = grid_time(m.timestamp);
        for (t, bar) in to_rules(&bars) {
            swing.close(t, bar, now);
        }
        swing.minute(now, m.price[1] as f64 / 1e6, m.price[2] as f64 / 1e6);
        Ok(())
    })?;
    crate::swing_replay::write_trades(&swing, output)
}
