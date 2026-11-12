use crate::{engine::Timeframe, trading::Candle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tick {
    pub symbol: String,
    pub millis: i64,
    pub price: i64,
    pub source: String,
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
    #[serde(default)]
    minute_end: Option<i64>,
    open: BTreeMap<Timeframe, OpenBar>,
}
impl Aggregator {
    pub fn last_quote_millis(&self) -> Option<i64> {
        self.last.as_ref().map(|t| t.millis)
    }

    pub fn push(&mut self, t: Tick) -> Result<Vec<(Timeframe, Candle)>, String> {
        if t.millis < 0 || t.price <= 0 || t.source.is_empty() {
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
        if let Some(end) = self.minute_end.take() {
            if t.millis >= end * 1000 {
                for b in self.open.values_mut() {
                    if b.bar.end > end && t.millis > end * 1000 {
                        b.complete = false;
                    }
                }
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
                    old.bar.high = old.bar.high.max(t.price);
                    old.bar.low = old.bar.low.min(t.price);
                    old.bar.close = t.price;
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
                        open: t.price,
                        high: t.price,
                        low: t.price,
                        close: t.price,
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
        if minute.gap_before_minutes > 0 {
            let covered = self.minute_end.unwrap_or(minute.timestamp);
            for b in self.open.values_mut() {
                if b.bar.end > covered {
                    b.complete = false;
                }
            }
        }
        let bars = self.push(Tick {
            symbol: symbol.into(),
            source: source.into(),
            millis: minute.timestamp * 1000,
            price: minute.price[0],
        })?;
        for open in self.open.values_mut() {
            open.bar.high = open.bar.high.max(minute.price[1]);
            open.bar.low = open.bar.low.min(minute.price[2]);
            open.bar.close = minute.price[3];
        }
        self.minute_end = Some(minute.timestamp + 60);
        Ok(bars)
    }
    pub fn closed_before(&self, at: i64) -> Vec<(Timeframe, Candle)> {
        self.open
            .iter()
            .filter(|(_, o)| {
                o.complete && o.bar.end <= at && self.minute_end.is_none_or(|end| end >= o.bar.end)
            })
            .map(|(tf, o)| (*tf, o.bar.clone()))
            .collect()
    }
    pub fn disconnected(&mut self) {
        for b in self.open.values_mut() {
            b.complete = false;
        }
    }
}
