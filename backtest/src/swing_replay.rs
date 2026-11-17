//! Replays MetaTrader 5 M1 bars through the swing rule set and writes the trade list.
//! Used to reconcile the Rust rules with the reference prototype, not to tune parameters.
use chrono::NaiveDateTime;
use forex_core::swing::{dollar_index, Bar, Event, Kind, Ohlc, Swing, SwingConfig, Tf, TradeState};
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
/// Synthetic dollar index from six minute series (EURUSD, USDJPY, GBPUSD, USDCAD,
/// USDSEK, USDCHF), each sorted by time. Only minutes present in all six are kept.
pub fn build_dollar_index(pairs: &[Vec<Minute>]) -> Vec<Minute> {
    assert_eq!(pairs.len(), 6, "six pairs");
    let mut at = [0usize; 6];
    let mut out = vec![];
    while (0..6).all(|k| at[k] < pairs[k].len()) {
        let t = (0..6)
            .map(|k| pairs[k][at[k]].time)
            .max()
            .unwrap_or_default();
        if (0..6).all(|k| pairs[k][at[k]].time == t) {
            let ohlc = std::array::from_fn(|k| {
                let m = pairs[k][at[k]];
                Ohlc {
                    open: m.open,
                    high: m.high,
                    low: m.low,
                    close: m.close,
                }
            });
            let x = dollar_index(&ohlc);
            out.push(Minute {
                time: t,
                open: x.open,
                high: x.high,
                low: x.low,
                close: x.close,
            });
            for a in &mut at {
                *a += 1;
            }
        } else {
            for k in 0..6 {
                if pairs[k][at[k]].time < t {
                    at[k] += 1;
                }
            }
        }
    }
    out
}
fn read_minutes(csv: &Path) -> Result<Vec<Minute>, String> {
    let file = std::fs::File::open(csv).map_err(|_| "cannot read MT5 export")?;
    let mut out = vec![];
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|_| "cannot read MT5 export")?;
        if n == 0 && line.starts_with('<') {
            continue;
        }
        out.push(parse_row(line.trim_end()).map_err(|e| format!("line {}: {e}", n + 1))?);
    }
    Ok(out)
}
fn mt5_row(m: &Minute) -> String {
    let d = chrono::DateTime::from_timestamp(m.time, 0).unwrap_or_default();
    format!(
        "{}\t{}\t{:?}\t{:?}\t{:?}\t{:?}\t0\t0\t0",
        d.format("%Y.%m.%d"),
        d.format("%H:%M:%S"),
        m.open,
        m.high,
        m.low,
        m.close
    )
}
/// Writes the synthetic dollar index of six MT5 exports as an MT5-style export.
pub fn build(inputs: &[&Path], output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("output already exists".into());
    }
    let pairs = inputs
        .iter()
        .map(|p| read_minutes(p))
        .collect::<Result<Vec<_>, _>>()?;
    let index = build_dollar_index(&pairs);
    let mut out =
        std::io::BufWriter::new(std::fs::File::create(output).map_err(|_| "cannot create output")?);
    for m in &index {
        writeln!(out, "{}", mt5_row(m)).map_err(|_| "cannot write output")?;
    }
    println!("{}", serde_json::json!({"minutes": index.len()}));
    Ok(())
}
fn load_config(path: &Path) -> Result<SwingConfig, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "cannot read swing config")?)
        .map_err(|e| format!("invalid swing config: {e}"))
}
/// Runs the rule set over an index export and returns its events sorted by time.
fn index_events(csv: &Path, config: &Path) -> Result<Vec<(i64, Event, i8)>, String> {
    let mut swing = Swing::new(load_config(config)?);
    let mut bars = Bars::default();
    let mut events = vec![];
    for m in read_minutes(csv)? {
        for (tf, bar) in bars.push(&m) {
            swing.close(tf, bar, m.time);
        }
        swing.minute(m.time, m.high, m.low);
        events.extend(swing.take_events());
    }
    events.sort();
    Ok(events)
}
fn stamp(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}
pub fn run(
    csv: &Path,
    config: &Path,
    output: &Path,
    index: Option<(&Path, &Path)>,
) -> Result<(), String> {
    if output.exists() {
        return Err("output already exists".into());
    }
    let events = match index {
        Some((csv, config)) => index_events(csv, config)?,
        None => vec![],
    };
    let mut swing = Swing::new(load_config(config)?);
    let mut bars = Bars::default();
    let mut known = 0;
    for m in read_minutes(csv)? {
        for (tf, bar) in bars.push(&m) {
            swing.close(tf, bar, m.time);
        }
        while known < events.len() && events[known].0 <= m.time {
            known += 1;
        }
        swing.reference_minute(m.time, m.close, &events[..known]);
        swing.minute(m.time, m.high, m.low);
    }
    write_trades(&swing, output)
}
/// Writes the trade list and prints a summary.
pub fn write_trades(swing: &Swing, output: &Path) -> Result<(), String> {
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
                Kind::Range => "range",
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
                TradeState::ReferenceExit => "dxy_close",
            },
            t.r.map(|r| format!("{r:?}")).unwrap_or_default(),
            t.exit_t.map(stamp).unwrap_or_default(),
        )
        .map_err(|_| "cannot write output")?;
    }
    println!(
        "{}",
        serde_json::json!({"trades": swing.trades().len(), "closed": closed, "net_r": net, "bars": {"W": swing.frame(Tf::W).len(), "D": swing.frame(Tf::D).len(), "H4": swing.frame(Tf::H4).len()}})
    );
    Ok(())
}
