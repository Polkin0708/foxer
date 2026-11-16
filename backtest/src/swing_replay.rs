//! Replays MetaTrader 5 M1 bars through the swing rule set and writes the trade list.
//! Used to reconcile the Rust rules with the reference prototype, not to tune parameters.
use chrono::NaiveDateTime;
use forex_core::swing::{Bar, Kind, Swing, SwingConfig, Tf, TradeState};
use std::io::{BufRead, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minute {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}
/// One row of an MT5 export: date, time, open, high, low, close, ... separated by tabs.
pub fn parse_row(line: &str) -> Result<Minute, String> {
    let f: Vec<&str> = line.split('\t').collect();
    if f.len() < 6 {
        return Err("short MT5 row".into());
    }
    let time = NaiveDateTime::parse_from_str(&format!("{} {}", f[0], f[1]), "%Y.%m.%d %H:%M:%S")
        .map_err(|_| "invalid MT5 time")?
        .and_utc()
        .timestamp();
    let price = |s: &str| s.parse::<f64>().map_err(|_| "invalid MT5 price".to_owned());
    Ok(Minute {
        time,
        open: price(f[2])?,
        high: price(f[3])?,
        low: price(f[4])?,
        close: price(f[5])?,
    })
}
const HOUR4: i64 = 14_400;
const DAY: i64 = 86_400;
const WEEK: i64 = 7 * DAY;
/// Seconds from the Monday that starts the epoch week to 1970-01-01 (a Thursday).
const MONDAY_SHIFT: i64 = 3 * DAY;
fn bin_end(tf: Tf, t: i64) -> i64 {
    match tf {
        Tf::H4 => (t.div_euclid(HOUR4) + 1) * HOUR4,
        Tf::D => (t.div_euclid(DAY) + 1) * DAY,
        Tf::W => ((t + MONDAY_SHIFT).div_euclid(WEEK) + 1) * WEEK - MONDAY_SHIFT,
    }
}
/// Open H4, D and W bars on the source grid. Bins without minutes produce no bar.
#[derive(Default)]
pub struct Bars {
    open: [Option<Bar>; 3],
}
impl Bars {
    /// Adds a minute; returns the bars it closed, ordered by end time, then W, D, H4.
    pub fn push(&mut self, m: &Minute) -> Vec<(Tf, Bar)> {
        let mut closed = vec![];
        for (slot, tf) in [Tf::W, Tf::D, Tf::H4].into_iter().enumerate() {
            let end = bin_end(tf, m.time);
            match &mut self.open[slot] {
                Some(b) if b.end == end => {
                    b.high = b.high.max(m.high);
                    b.low = b.low.min(m.low);
                    b.close = m.close;
                }
                current => {
                    if let Some(b) = current.take() {
                        closed.push((tf, b));
                    }
                    *current = Some(Bar {
                        end,
                        open: m.open,
                        high: m.high,
                        low: m.low,
                        close: m.close,
                    });
                }
            }
        }
        closed.sort_by_key(|(tf, b)| (b.end, *tf));
        closed
    }
}
fn stamp(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}
pub fn run(csv: &Path, config: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("output already exists".into());
    }
    let cfg: SwingConfig =
        serde_json::from_slice(&std::fs::read(config).map_err(|_| "cannot read swing config")?)
            .map_err(|e| format!("invalid swing config: {e}"))?;
    let mut swing = Swing::new(cfg);
    let mut bars = Bars::default();
    let file = std::fs::File::open(csv).map_err(|_| "cannot read MT5 export")?;
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|_| "cannot read MT5 export")?;
        if n == 0 && line.starts_with('<') {
            continue;
        }
        let m = parse_row(line.trim_end()).map_err(|e| format!("line {}: {e}", n + 1))?;
        for (tf, bar) in bars.push(&m) {
            swing.close(tf, bar, m.time);
        }
        swing.minute(m.time, m.high, m.low);
    }
    let mut out = std::fs::File::create(output).map_err(|_| "cannot create output")?;
    writeln!(
        out,
        "kind,dir,signal_t,fill_t,entry,stop,tp,rr,state,r,exit_t"
    )
    .map_err(|_| "cannot write output")?;
    let (mut closed, mut net) = (0, 0.0);
    for t in swing.trades() {
        if let Some(r) = t.r {
            closed += 1;
            net += r;
        }
        writeln!(
            out,
            "{},{},{},{},{:?},{:?},{:?},{:?},{},{},{}",
            match t.kind {
                Kind::Trend => "trend",
                Kind::Correction => "correction",
            },
            t.dir,
            stamp(t.signal_t),
            t.fill_t.map(stamp).unwrap_or_default(),
            t.entry,
            t.stop,
            t.tp,
            t.rr,
            match t.state {
                TradeState::Pending => "pending",
                TradeState::Active => "active",
                TradeState::Stop => "sl",
                TradeState::Target => "tp",
                TradeState::Cancelled => "cancelled",
            },
            t.r.map(|r| format!("{r:?}")).unwrap_or_default(),
            t.exit_t.map(stamp).unwrap_or_default(),
        )
        .map_err(|_| "cannot write output")?;
    }
    println!(
        "{}",
        serde_json::json!({"trades": swing.trades().len(), "closed": closed, "net_r": net})
    );
    Ok(())
}
