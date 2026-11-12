use crate::{
    engine::{Engine, Timeframe},
    feed::{Aggregator, Tick},
    model::Strategy,
    trading::{Candle, Side, Signal},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Change {
    pub signal: Signal,
    pub previous: Option<crate::trading::SignalState>,
    pub basis: serde_json::Value,
}
pub struct Batch {
    pub bars: Vec<(Timeframe, Candle)>,
    pub changes: Vec<Change>,
    pub weekly: Vec<(Option<Side>, Option<Side>)>,
}
pub fn process(
    e: &mut Engine,
    a: &mut Aggregator,
    t: &Tick,
    c: &Strategy,
    allow: bool,
) -> Result<Batch, String> {
    let step = c
        .instruments
        .iter()
        .find(|i| i.symbol == t.symbol)
        .ok_or("unknown instrument")?
        .price_tick;
    let before = e.signals();
    let bars = a.push(t.clone())?;
    let mut weekly = Vec::new();
    for (tf, b) in &bars {
        if allow {
            let prev = e.trend(Timeframe::W);
            e.closed(*tf, b.clone(), c)?;
            let current = e.trend(Timeframe::W);
            if *tf == Timeframe::W && prev != current {
                weekly.push((prev, current));
            }
        } else {
            e.seed(*tf, b.clone(), c)?;
        }
    }
    let q = crate::trading::Quote {
        time: t.millis / 1000,
        price: t.price,
    };
    if allow {
        e.quote(q, step, c)?;
    } else {
        e.track(q);
    }
    e.namespace_signals(&t.symbol);
    let changes = e
        .signals()
        .into_iter()
        .filter_map(|signal| {
            let prev = before.iter().find(|s| s.id == signal.id);
            if prev.is_some_and(|s| s.state == signal.state) {
                return None;
            }
            let basis = if prev.is_none() {
                e.signal_basis(&signal.id)
            } else {
                serde_json::Value::Null
            };
            Some(Change {
                previous: prev.map(|s| s.state),
                signal,
                basis,
            })
        })
        .collect();
    Ok(Batch {
        bars,
        changes,
        weekly,
    })
}
