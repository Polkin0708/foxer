use crate::{engine::Timeframe, trading::Candle};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tick {
    pub symbol: String,
    pub millis: i64,
    pub bid: i64,
    pub ask: i64,
    pub source: String,
}
pub fn parse_tick(v: &Value) -> Result<Tick, String> {
    if !v["symbol"].as_str().is_some_and(crate::valid_symbol) {
        return Err("unexpected quote symbol".into());
    }
    let price = |key: &str| {
        crate::prices::price(&v[key].as_number().ok_or("invalid quote price")?.to_string())
    };
    let bid = price("bid")?;
    let ask = price("ask")?;
    let t =
        chrono::DateTime::parse_from_rfc3339(v["timestamp"].as_str().ok_or("missing quote time")?)
            .map_err(|_| "invalid quote time")?;
    if t.offset().local_minus_utc() != 0 || t.timestamp_subsec_nanos() % 1_000_000 != 0 {
        return Err("quote timestamp must be UTC milliseconds".into());
    }
    let source = v["source"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("missing quote source")?
        .to_owned();
    if bid <= 0 || ask < bid {
        return Err("invalid quote spread".into());
    }
    Ok(Tick {
        symbol: v["symbol"].as_str().unwrap().into(),
        millis: t.timestamp_millis(),
        bid,
        ask,
        source,
    })
}
pub fn parse_frames(text: &str) -> Result<Vec<Tick>, String> {
    let mut out = vec![];
    for raw in text.split('\u{1e}').filter(|s| !s.is_empty()) {
        let v: Value = serde_json::from_str(raw).map_err(|_| "invalid SignalR frame")?;
        if v.get("error").is_some() || v["type"] == 7 {
            return Err("feed closed or subscription failed".into());
        }
        if v["type"] == 1 && v["target"] == "ReceiveTick" {
            let args = v["arguments"]
                .as_array()
                .filter(|a| a.len() == 1)
                .ok_or("invalid tick arguments")?;
            out.push(parse_tick(&args[0])?);
        }
    }
    Ok(out)
}
#[derive(Clone, Serialize, Deserialize)]
struct OpenBar {
    bar: Candle,
    complete: bool,
    #[serde(skip)]
    start: Option<i64>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Aggregator {
    last: Option<Tick>,
    open: BTreeMap<Timeframe, OpenBar>,
}
impl Aggregator {
    pub fn last_quote_millis(&self) -> Option<i64> {
        self.last.as_ref().map(|t| t.millis)
    }

    pub fn push(&mut self, t: Tick) -> Result<Vec<(Timeframe, Candle)>, String> {
        if t.millis < 0 || t.bid <= 0 || t.ask < t.bid || t.source.is_empty() {
            return Err("invalid tick".into());
        }
        if let Some(last) = &self.last {
            if last == &t {
                return Ok(vec![]);
            }
            if t.millis < last.millis {
                return Err("out of order tick".into());
            }
            if t.symbol != last.symbol {
                return Err("quote symbol changed".into());
            }
            if t.source != last.source {
                return Err("quote source changed".into());
            }
        }
        let mut closed = vec![];
        let sec = t.millis / 1000;
        for tf in [Timeframe::W, Timeframe::D, Timeframe::H4] {
            let (start, end) = match self.open.get(&tf) {
                Some(b) if b.start.is_some_and(|s| sec >= s) && sec < b.bar.end => {
                    (b.start.unwrap(), b.bar.end)
                }
                _ => crate::calendar::bounds(sec, tf)?,
            };
            if let Some(old) = self.open.get_mut(&tf) {
                if old.bar.end == end {
                    old.bar.high = old.bar.high.max(t.bid);
                    old.bar.low = old.bar.low.min(t.bid);
                    old.bar.close = t.bid;
                    continue;
                }
                if old.complete {
                    closed.push((tf, old.bar.clone()));
                }
            }
            let complete = self
                .open
                .get(&tf)
                .map(|o| o.bar.end == start)
                .unwrap_or(t.millis == start * 1000);
            self.open.insert(
                tf,
                OpenBar {
                    bar: Candle {
                        end,
                        open: t.bid,
                        high: t.bid,
                        low: t.bid,
                        close: t.bid,
                    },
                    complete,
                    start: Some(start),
                },
            );
        }
        self.last = Some(t);
        Ok(closed)
    }
    pub fn minute(
        &mut self,
        symbol: &str,
        source: &str,
        minute: &crate::prices::Minute,
    ) -> Result<Vec<(Timeframe, Candle)>, String> {
        let bars = self.push(Tick {
            symbol: symbol.into(),
            source: source.into(),
            millis: minute.timestamp * 1000,
            bid: minute.bid[0],
            ask: minute.ask[0],
        })?;
        for open in self.open.values_mut() {
            open.bar.high = open.bar.high.max(minute.bid[1]);
            open.bar.low = open.bar.low.min(minute.bid[2]);
            open.bar.close = minute.bid[3];
        }
        Ok(bars)
    }
    pub fn closed_before(&self, at: i64) -> Vec<(Timeframe, Candle)> {
        self.open
            .iter()
            .filter(|(_, o)| o.complete && o.bar.end <= at)
            .map(|(tf, o)| (*tf, o.bar.clone()))
            .collect()
    }
    pub fn disconnected(&mut self) {
        for b in self.open.values_mut() {
            b.complete = false;
        }
    }
}
