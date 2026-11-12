use crate::{
    examples,
    history::{self, Dataset},
    stats, Ledger,
};
use forex_core::{
    engine::Engine,
    feed::{Aggregator, Tick},
    model::Strategy,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::mpsc::{sync_channel, Receiver},
    thread,
};
pub struct Options {
    pub strategy: PathBuf,
    pub data_config: PathBuf,
    pub from: i64,
    pub to: i64,
    pub symbols: Vec<String>,
    pub output: PathBuf,
    pub pdf: bool,
}
pub fn date(s: &str) -> Result<i64, String> {
    let s = if s.len() == 10 {
        format!("{s}T00:00:00Z")
    } else {
        s.to_owned()
    };
    forex_core::prices::utc(&s)?
        .checked_mul(1000)
        .ok_or("date overflow".into())
}
fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
struct Market {
    engine: Engine,
    agg: Aggregator,
    rx: Receiver<Result<Tick, String>>,
    head: Option<Tick>,
    gaps: Vec<(i64, i64)>,
    gap_index: usize,
    coverage: Value,
    bars: BTreeMap<String, Vec<Value>>,
}
fn next(rx: &Receiver<Result<Tick, String>>) -> Result<Option<Tick>, String> {
    match rx.recv() {
        Ok(t) => t.map(Some),
        Err(_) => Ok(None),
    }
}
pub fn run(o: Options) -> Result<(), String> {
    if o.from >= o.to {
        return Err("from must precede to".into());
    }
    if o.output.exists() {
        return Err("output directory already exists".into());
    }
    let bytes = fs::read(&o.strategy).map_err(|e| e.to_string())?;
    let cfg: Strategy = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    forex_core::validate(&cfg)?;
    let selected: Vec<_> = cfg
        .instruments
        .iter()
        .filter(|i| o.symbols.is_empty() || o.symbols.contains(&i.symbol))
        .map(|i| i.symbol.clone())
        .collect();
    if selected.is_empty() || o.symbols.iter().any(|s| !selected.contains(s)) {
        return Err("unknown or empty instrument selection".into());
    }
    let mut uniq = o.symbols.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != o.symbols.len() {
        return Err("duplicate instrument".into());
    }
    let source_cfg = history::json_file(&o.data_config)?;
    let mut markets = Vec::new();
    let mut sources = BTreeMap::new();
    let mut initial = BTreeMap::new();
    let floor = o.from.div_euclid(60000) * 60000;
    for symbol in &selected {
        eprintln!("Loading historical context: {symbol}");
        let roots = source_cfg[symbol]
            .as_array()
            .ok_or_else(|| format!("missing data roots for {symbol}"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(PathBuf::from)
                    .ok_or("invalid data root".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut data = Dataset::open(roots, symbol, floor, o.to)?;
        let (engine, agg, counts) = data.prepare(symbol, floor, &cfg)?;
        initial.insert(symbol.clone(), counts);
        for a in &data.archives {
            history::verify(&a.path, &a.sha256)?;
            data.provenance
                .push(json!({"archive":a.path,"sha256":a.sha256}));
        }
        sources.insert(symbol.clone(),json!({"files":data.provenance,"excluded_intervals":data.gaps.iter().filter(|(a,b)|*a<o.to&&*b>o.from).collect::<Vec<_>>()}));
        let gaps: Vec<_> = data
            .gaps
            .iter()
            .copied()
            .filter(|(a, b)| *a < o.to && *b > floor)
            .collect();
        let worker_gaps = data.gaps.clone();
        let worker_source = data.source.clone();
        let archives = data.archives;
        let symbol_worker = symbol.clone();
        let to = o.to;
        let (tx, rx) = sync_channel(1024);
        thread::spawn(move || {
            let mut last = None;
            for a in archives {
                let result = history::read_archive(
                    &a.path,
                    &symbol_worker,
                    &worker_source,
                    &a.sha256,
                    &worker_gaps,
                    |t| {
                        if last.is_some_and(|time| time > t.millis) {
                            return Err("archive boundary time reversal".into());
                        }
                        last = Some(t.millis);
                        if t.millis >= floor && t.millis < to {
                            tx.send(Ok(t)).map_err(|_| "consumer closed")?;
                        }
                        Ok(())
                    },
                );
                if let Err(e) = result {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        });
        let head = next(&rx)?;
        markets.push(Market {
            engine,
            agg,
            rx,
            head,
            gaps,
            gap_index: 0,
            coverage: json!({"ticks":0,"first":null,"last":null}),
            bars: BTreeMap::new(),
        });
    }
    fs::create_dir_all(o.output.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    fs::create_dir(&o.output).map_err(|e| e.to_string())?;
    fs::write(o.output.join("strategy.json"), &bytes).map_err(|e| e.to_string())?;
    let mut journal =
        BufWriter::new(File::create(o.output.join("events.jsonl")).map_err(|e| e.to_string())?);
    let mut ledger = Ledger::default();
    let mut sequence = 0u64;
    loop {
        let chosen = markets
            .iter()
            .enumerate()
            .filter_map(|(i, m)| m.head.as_ref().map(|t| (t.millis, i)))
            .min();
        let Some((_, i)) = chosen else { break };
        let m = &mut markets[i];
        let t = m.head.take().unwrap();
        let symbol = &selected[i];
        while m.gap_index < m.gaps.len() && m.gaps[m.gap_index].0 <= t.millis {
            let (a, b) = m.gaps[m.gap_index];
            m.agg.disconnected();
            ledger.gap(symbol, a, b);
            writeln!(
                journal,
                "{}",
                json!({"kind":"gap","symbol":symbol,"from":a,"to":b})
            )
            .map_err(|e| e.to_string())?;
            m.gap_index += 1;
        }
        let batch = forex_core::processing::process(
            &mut m.engine,
            &mut m.agg,
            &t,
            &cfg,
            t.millis >= o.from,
        )?;
        if t.millis >= o.from {
            sequence += 1;
            ledger.observe(symbol, &t, &batch.changes);
            m.coverage["ticks"] = json!(m.coverage["ticks"].as_u64().unwrap() + 1);
            if m.coverage["first"].is_null() {
                m.coverage["first"] = json!(t.millis)
            }
            m.coverage["last"] = json!(t.millis);
            for (tf, b) in batch.bars {
                m.bars.entry(format!("{tf:?}")).or_default().push(json!(b));
            }
            for event in batch.changes {
                writeln!(journal,"{}",json!({"sequence":sequence,"millis":t.millis,"symbol":symbol,"price":t.price,"change":event})).map_err(|e|e.to_string())?;
            }
        }
        m.head = next(&m.rx)?;
    }
    for (i, m) in markets.iter_mut().enumerate() {
        while m.gap_index < m.gaps.len() {
            let (a, b) = m.gaps[m.gap_index];
            ledger.gap(&selected[i], a, b);
            writeln!(
                journal,
                "{}",
                json!({"kind":"gap","symbol":selected[i],"from":a,"to":b})
            )
            .map_err(|e| e.to_string())?;
            m.gap_index += 1;
        }
    }
    journal.flush().map_err(|e| e.to_string())?;
    if markets.iter().any(|m| m.coverage["ticks"] == 0) {
        return Err(
            "no observed ticks for a selected instrument; incomplete output retained".into(),
        );
    }
    let mut by_symbol = BTreeMap::new();
    let mut monthly: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for symbol in &selected {
        let trades: Vec<_> = ledger
            .trades
            .iter()
            .filter(|t| t["symbol"] == *symbol)
            .cloned()
            .collect();
        by_symbol.insert(symbol.clone(), stats(&trades));
    }
    for tr in &ledger.trades {
        if let Some(ms) = tr["exit_at"].as_i64() {
            let month = chrono::DateTime::from_timestamp_millis(ms)
                .ok_or("invalid exit date")?
                .format("%Y-%m")
                .to_string();
            monthly.entry(month).or_default().push(tr.clone());
        }
    }
    let monthly: BTreeMap<_, _> = monthly.iter().map(|(m, t)| (m.clone(), stats(t))).collect();
    let affected: Vec<_> = ledger
        .trades
        .iter()
        .filter(|t| !t["gaps"].as_array().unwrap().is_empty())
        .cloned()
        .collect();
    let mut chosen_examples = Vec::new();
    for i in examples(&ledger.trades) {
        let trade = ledger.trades[i].clone();
        let symbol = trade["symbol"].as_str().unwrap();
        let idx = selected.iter().position(|s| s == symbol).unwrap();
        let end = trade["exit_at"].as_i64().unwrap_or(o.to) / 1000;
        let start = trade["signal_at"].as_i64().unwrap() / 1000;
        let mut charts = BTreeMap::new();
        for tf in ["W", "D", "H4"] {
            let mut candles = trade["basis"]["frames"][tf]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some(after) = markets[idx].bars.get(tf) {
                for b in after {
                    let at = b["end"].as_i64().unwrap();
                    if at > start
                        && at <= end + 86400
                        && !candles.iter().any(|c| c["end"] == b["end"])
                    {
                        candles.push(b.clone());
                    }
                }
            }
            charts.insert(tf, json!(candles));
        }
        chosen_examples.push(json!({"trade":trade,"charts":charts}));
    }
    let coverage: BTreeMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(i, s)| (s.clone(), markets[i].coverage.clone()))
        .collect();
    let result = json!({"schema_version":1,"engine_version":env!("CARGO_PKG_VERSION"),"program_sha256":history::hash(&std::env::current_exe().map_err(|e|e.to_string())?)?,"strategy_sha256":format!("{:x}",Sha256::digest(&bytes)),"from":o.from,"to":o.to,"symbols":selected,"sample_seed":20261110,"initial_context":initial,"sources":sources,"coverage":coverage,"summary":ledger.summary(),"by_symbol":by_symbol,"monthly":monthly,"gap_affected_summary":stats(&affected),"trades":ledger.trades,"examples":chosen_examples,"execution":{"market":"first observed index price","limit":"level without improvement","stop":"observed index price","target":"level without improvement","costs":"not modeled","gaps":"observed ticks only; affected trades flagged","drawdown":"closed trades only"}});
    write_json(&o.output.join("results.json"), &result)?;
    let mut csv = csv::Writer::from_path(o.output.join("trades.csv")).map_err(|e| e.to_string())?;
    let fields = [
        "id",
        "symbol",
        "side",
        "state",
        "signal_at",
        "entry_at",
        "entry",
        "stop",
        "target",
        "take_profit",
        "exit_at",
        "exit",
        "result_r",
        "unrealized_r",
        "marked_at",
    ];
    csv.write_record(fields).map_err(|e| e.to_string())?;
    for t in result["trades"].as_array().unwrap() {
        csv.write_record(fields.map(|f| {
            if t[f].is_null() {
                String::new()
            } else if let Some(s) = t[f].as_str() {
                s.to_string()
            } else {
                t[f].to_string()
            }
        }))
        .map_err(|e| e.to_string())?;
    }
    csv.flush().map_err(|e| e.to_string())?;
    if o.pdf {
        report(&o.output)?;
    }
    println!(
        "Completed: {} closed signals; results: {}",
        result["summary"]["closed"],
        o.output.display()
    );
    Ok(())
}
pub fn report(dir: &Path) -> Result<(), String> {
    let status = std::process::Command::new("python3")
        .arg(
            std::env::var("FOREX_REPORT_SCRIPT")
                .unwrap_or_else(|_| "backtest/report/render.py".into()),
        )
        .arg(dir)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("PDF failed; calculation artifacts preserved for report-only retry".into())
    }
}
