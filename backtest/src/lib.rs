use forex_core::{
    feed::Tick,
    processing::Change,
    trading::{Side, SignalState},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
pub mod history;
pub mod runner;
#[derive(Default, Serialize, Deserialize)]
pub struct Ledger {
    pub trades: Vec<Value>,
    #[serde(skip)]
    indices: BTreeMap<String, usize>,
}
impl Ledger {
    pub fn gap(&mut self, symbol: &str, start: i64, end: i64) {
        for tr in &mut self.trades {
            if tr["symbol"] == symbol && (tr["state"] == "Active" || tr["state"] == "Pending") {
                tr["gaps"].as_array_mut().unwrap().push(json!([start, end]));
            }
        }
    }
    pub fn observe(&mut self, symbol: &str, t: &Tick, changes: &[Change]) {
        for change in changes {
            let s = &change.signal;
            let key = format!("{symbol}:{}", s.id);
            let i=*self.indices.entry(key).or_insert_with(||{let i=self.trades.len(); self.trades.push(json!({"id":s.id,"symbol":symbol,"side":s.side,"state":s.state,"entry":s.entry,"stop":s.stop,"target":s.target,"take_profit":s.take_profit,"signal_at":t.millis,"entry_at":null,"exit_at":null,"result_r":null,"unrealized_r":null,"basis":change.basis,"gaps":[]}));i});
            let tr = &mut self.trades[i];
            tr["state"] = json!(s.state);
            if s.state == SignalState::Active && tr["entry_at"].is_null() {
                tr["entry_at"] = json!(t.millis);
            }
            if matches!(s.state, SignalState::Stop | SignalState::TakeProfit) {
                let price = if s.state == SignalState::TakeProfit {
                    s.take_profit
                } else {
                    t.price
                };
                let sign = if s.side == Side::Long { 1.0 } else { -1.0 };
                tr["exit"] = json!(price);
                tr["exit_at"] = json!(t.millis);
                tr["result_r"] =
                    json!(sign * (price - s.entry) as f64 / (s.entry - s.stop).abs() as f64);
                tr["unrealized_r"] = Value::Null;
            }
            if s.state == SignalState::Cancelled {
                tr["cancelled_at"] = json!(t.millis);
            }
        }
        for tr in &mut self.trades {
            if tr["symbol"] == symbol && tr["state"] == "Active" {
                let long = tr["side"] == "Long";
                let mark = t.price;
                let entry = tr["entry"].as_i64().unwrap();
                let stop = tr["stop"].as_i64().unwrap();
                tr["unrealized_r"] = json!(
                    (if long { 1.0 } else { -1.0 }) * (mark - entry) as f64
                        / (entry - stop).abs() as f64
                );
                tr["marked_at"] = json!(t.millis);
                tr["mark"] = json!(mark);
            }
        }
    }
    pub fn summary(&self) -> Value {
        stats(&self.trades)
    }
}
pub fn stats(trades: &[Value]) -> Value {
    let mut closed: Vec<_> = trades
        .iter()
        .filter(|t| t["result_r"].is_number())
        .collect();
    closed.sort_by_key(|t| {
        (
            t["exit_at"].as_i64().unwrap_or(0),
            t["symbol"].as_str().unwrap_or(""),
            t["id"].as_str().unwrap_or(""),
        )
    });
    let (mut net, mut peak, mut dd, mut gain, mut loss, mut duration) =
        (0.0f64, 0.0f64, 0.0f64, 0.0, 0.0, 0.0);
    let mut curve = Vec::new();
    for tr in &closed {
        let r = tr["result_r"].as_f64().unwrap();
        net += r;
        peak = peak.max(net);
        dd = dd.max(peak - net);
        if r > 0.0 {
            gain += r
        } else {
            loss -= r
        };
        duration +=
            (tr["exit_at"].as_i64().unwrap() - tr["entry_at"].as_i64().unwrap()) as f64 / 1000.0;
        curve.push(json!({"time":tr["exit_at"],"net_r":net,"drawdown_r":peak-net}));
    }
    let n = closed.len();
    let wins = closed
        .iter()
        .filter(|t| t["result_r"].as_f64().unwrap() > 0.0)
        .count();
    json!({"closed":n,"wins":wins,"net_r":net,"mean_r":if n>0{Some(net/n as f64)}else{None},"win_rate":if n>0{Some(wins as f64/n as f64)}else{None},"profit_factor":if loss>0.0{Some(gain/loss)}else{None},"mean_duration_seconds":if n>0{Some(duration/n as f64)}else{None},"max_closed_drawdown_r":dd,"curve":curve,"open":trades.iter().filter(|t|t["state"]=="Active").count(),"pending":trades.iter().filter(|t|t["state"]=="Pending").count(),"cancelled":trades.iter().filter(|t|t["state"]=="Cancelled").count(),"gap_affected":trades.iter().filter(|t|!t["gaps"].as_array().unwrap().is_empty()).count()})
}
pub fn examples(trades: &[Value]) -> Vec<usize> {
    let mut all: Vec<_> = trades
        .iter()
        .enumerate()
        .filter(|(_, t)| t["result_r"].is_number())
        .map(|(i, _)| i)
        .collect();
    all.sort_by(|&a, &b| {
        trades[b]["result_r"]
            .as_f64()
            .unwrap()
            .total_cmp(&trades[a]["result_r"].as_f64().unwrap())
            .then(a.cmp(&b))
    });
    let mut out = Vec::new();
    for i in all.iter().take(3).chain(all.iter().rev().take(3)) {
        if !out.contains(i) {
            out.push(*i);
        }
    }
    let mut rest: Vec<_> = all.into_iter().filter(|i| !out.contains(i)).collect();
    let mut seed = 20261110u64;
    for i in (1..rest.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        rest.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    out.extend(rest.into_iter().take(3));
    out
}
