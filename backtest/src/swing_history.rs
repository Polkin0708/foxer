//! Swing rules over prepared minutes, with bars from the bot's own aggregator.
use crate::history::{json_file, read_minutes};
use forex_core::{
    engine::Timeframe,
    feed::Aggregator,
    swing::{grid_time, Bar, Swing, Tf},
    trading::Candle,
};
use std::path::{Path, PathBuf};

const SCALE: f64 = 1_000_000.0;
fn tf(t: Timeframe) -> Tf {
    match t {
        Timeframe::W => Tf::W,
        Timeframe::D => Tf::D,
        Timeframe::H4 => Tf::H4,
    }
}
/// Aggregator bars converted for the rules and ordered by close time, then W, D, H4.
pub fn to_rules(bars: &[(Timeframe, Candle)]) -> Vec<(Tf, Bar)> {
    let mut out: Vec<(Tf, Bar)> = bars
        .iter()
        .map(|(t, c)| {
            (
                tf(*t),
                Bar {
                    end: grid_time(c.end),
                    open: c.open as f64 / SCALE,
                    high: c.high as f64 / SCALE,
                    low: c.low as f64 / SCALE,
                    close: c.close as f64 / SCALE,
                },
            )
        })
        .collect();
    out.sort_by(|a, b| (a.1.end, a.0).cmp(&(b.1.end, b.0)));
    out
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
        let bars = aggregator.minute(symbol, &source, &m)?;
        let now = grid_time(m.timestamp);
        for (t, bar) in to_rules(&bars) {
            swing.close(t, bar, now);
        }
        swing.minute(now, m.price[1] as f64 / SCALE, m.price[2] as f64 / SCALE);
        Ok(())
    })?;
    crate::swing_replay::write_trades(&swing, output)
}
