use crate::model::Strategy;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candle {
    pub end: i64,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Long,
    Short,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    Fvg,
    Gap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneState {
    Alive,
    Tested,
    Reacted,
    Inverted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub side: Side,
    pub low: i64,
    pub high: i64,
    pub formed_at: i64,
    pub kind: ZoneKind,
    pub state: ZoneState,
}
impl Zone {
    pub fn new(side: Side, low: i64, high: i64, formed_at: i64, kind: ZoneKind) -> Self {
        Self {
            side,
            low,
            high,
            formed_at,
            kind,
            state: ZoneState::Alive,
        }
    }
    pub fn observe(&mut self, b: &Candle, atr: f64, margin: f64) {
        if b.end <= self.formed_at || matches!(self.state, ZoneState::Reacted | ZoneState::Inverted)
        {
            return;
        }
        let tested = match self.side {
            Side::Long => b.low <= self.high,
            Side::Short => b.high >= self.low,
        };
        if tested {
            self.state = ZoneState::Tested;
        }
        if self.state != ZoneState::Tested {
            return;
        }
        let reacted = match self.side {
            Side::Long => b.close > self.high,
            Side::Short => b.close < self.low,
        };
        if reacted {
            self.state = ZoneState::Reacted;
            return;
        }
        let distance = match self.side {
            Side::Long => self.low - b.close,
            Side::Short => b.close - self.high,
        };
        if distance > 0 && distance as f64 >= atr * margin {
            self.state = ZoneState::Inverted;
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fractal {
    pub price: i64,
    pub confirmed_at: i64,
    pub upper: bool,
    pub swept: bool,
}
pub fn atr(bars: &[Candle], period: usize) -> Option<f64> {
    if period == 0 || bars.len() < period {
        return None;
    }
    let mut total = 0i128;
    for b in &bars[bars.len() - period..] {
        if b.low <= 0 || b.high < b.low {
            return None;
        }
        total += i128::from(b.high) - i128::from(b.low);
    }
    Some(total as f64 / period as f64)
}
pub fn fvg(bars: &[Candle], atr: f64, threshold: f64) -> Option<Zone> {
    if bars.len() < 3 {
        return None;
    }
    let a = &bars[bars.len() - 3];
    let c = bars.last()?;
    if c.low > a.high {
        zone(
            Side::Long,
            a.high,
            c.low,
            c.end,
            ZoneKind::Fvg,
            atr,
            threshold,
        )
    } else if c.high < a.low {
        zone(
            Side::Short,
            c.high,
            a.low,
            c.end,
            ZoneKind::Fvg,
            atr,
            threshold,
        )
    } else {
        None
    }
}
pub fn gap(bars: &[Candle], atr: f64, threshold: f64) -> Option<Zone> {
    if bars.len() < 2 {
        return None;
    }
    let a = &bars[bars.len() - 2];
    let b = bars.last()?;
    match b.open.cmp(&a.close) {
        std::cmp::Ordering::Greater => zone(
            Side::Long,
            a.close,
            b.open,
            b.end,
            ZoneKind::Gap,
            atr,
            threshold,
        ),
        std::cmp::Ordering::Less => zone(
            Side::Short,
            b.open,
            a.close,
            b.end,
            ZoneKind::Gap,
            atr,
            threshold,
        ),
        std::cmp::Ordering::Equal => None,
    }
}
fn zone(
    side: Side,
    low: i64,
    high: i64,
    time: i64,
    kind: ZoneKind,
    atr: f64,
    threshold: f64,
) -> Option<Zone> {
    if low <= 0
        || high <= low
        || !atr.is_finite()
        || atr < 0.0
        || !threshold.is_finite()
        || threshold < 0.0
        || ((high - low) as f64) < atr * threshold
    {
        return None;
    }
    Some(Zone::new(side, low, high, time, kind))
}
pub fn fractals(bars: &[Candle]) -> Vec<Fractal> {
    if bars.len() < 3 {
        return vec![];
    }
    let a = &bars[bars.len() - 3];
    let b = &bars[bars.len() - 2];
    let c = &bars[bars.len() - 1];
    let mut out = vec![];
    if b.high > a.high && b.high > c.high {
        out.push(Fractal {
            price: b.high,
            confirmed_at: c.end,
            upper: true,
            swept: false,
        })
    }
    if b.low < a.low && b.low < c.low {
        out.push(Fractal {
            price: b.low,
            confirmed_at: c.end,
            upper: false,
            swept: false,
        })
    }
    out
}
#[allow(clippy::too_many_arguments)]
pub fn select_stop(
    side: Side,
    lo: i64,
    hi: i64,
    fractals: &[Fractal],
    bars: &[Candle],
    atr: f64,
    tick: i64,
    cfg: &Strategy,
) -> Result<i64, String> {
    if tick <= 0 || !atr.is_finite() || atr < 0.0 || lo <= 0 || hi < lo {
        return Err("invalid stop inputs".into());
    }
    let selected = fractals
        .iter()
        .filter(|f| match side {
            Side::Long => !f.upper && f.price < lo,
            Side::Short => f.upper && f.price > hi,
        })
        .max_by_key(|f| f.confirmed_at)
        .map(|f| f.price);
    let base = if let Some(p) = selected {
        p
    } else {
        let n = cfg.parameters.stop_fallback_h4_bars.value as usize;
        if n == 0 || bars.len() < n {
            return Err("insufficient fallback bars".into());
        }
        let slice = &bars[bars.len() - n..];
        match side {
            Side::Long => slice.iter().map(|b| b.low).min(),
            Side::Short => slice.iter().map(|b| b.high).max(),
        }
        .ok_or("no fallback bars")?
    };
    let buffer = (atr * cfg.parameters.stop_buffer_atr.value / tick as f64).ceil();
    if !buffer.is_finite() || buffer > i64::MAX as f64 / tick as f64 {
        return Err("stop buffer overflow".into());
    }
    let raw = match side {
        Side::Long => i128::from(base) - buffer as i128 * i128::from(tick),
        Side::Short => i128::from(base) + buffer as i128 * i128::from(tick),
    };
    let stop = round_tick(raw, tick, side == Side::Short)?;
    if stop <= 0 {
        return Err("invalid stop".into());
    }
    Ok(stop)
}
fn round_tick(value: i128, tick: i64, up: bool) -> Result<i64, String> {
    let tick = i128::from(tick);
    let q = value.div_euclid(tick) + i128::from(up && value.rem_euclid(tick) != 0);
    i64::try_from(q * tick).map_err(|_| "price overflow".into())
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Quote {
    pub time: i64,
    pub bid: i64,
    pub ask: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalState {
    Pending,
    Active,
    Cancelled,
    Stop,
    TakeProfit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub side: Side,
    pub entry: i64,
    pub stop: i64,
    pub target: i64,
    pub take_profit: i64,
    pub state: SignalState,
    pub last_quote_time: i64,
}
impl Signal {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        side: Side,
        q: Quote,
        stop: i64,
        target: i64,
        tick: i64,
        cfg: &Strategy,
    ) -> Result<Self, String> {
        Self::with_cap(
            id,
            side,
            q,
            stop,
            target,
            tick,
            cfg,
            cfg.parameters.rr_max_trend.value,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn with_cap(
        id: &str,
        side: Side,
        q: Quote,
        stop: i64,
        target: i64,
        tick: i64,
        cfg: &Strategy,
        maximum: f64,
    ) -> Result<Self, String> {
        if !q.valid() || tick <= 0 || stop <= 0 || target <= 0 {
            return Err("invalid execution inputs".into());
        }
        let sign = if side == Side::Long { 1i128 } else { -1i128 };
        let market = if side == Side::Long { q.ask } else { q.bid };
        let risk = sign * (i128::from(market) - i128::from(stop));
        let reward = sign * (i128::from(target) - i128::from(market));
        if risk <= 0 || reward <= 0 {
            return Err("stop or target behind entry".into());
        }
        let (min_num, min_den) = rr_ratio(cfg.parameters.rr_min.value)?;
        let (max_num, max_den) = rr_ratio(maximum)?;
        let (entry, state) = if reward * min_den >= min_num * risk {
            (market, SignalState::Active)
        } else {
            let numerator = i128::from(target) * min_den + min_num * i128::from(stop);
            let denominator = min_den + min_num;
            let scaled_denominator = denominator * i128::from(tick);
            let units = numerator.div_euclid(scaled_denominator)
                + i128::from(side == Side::Short && numerator.rem_euclid(scaled_denominator) != 0);
            (
                i64::try_from(units * i128::from(tick)).map_err(|_| "entry overflow")?,
                SignalState::Pending,
            )
        };
        let risk = sign * (i128::from(entry) - i128::from(stop));
        let reward = sign * (i128::from(target) - i128::from(entry));
        if risk <= 0 || reward * min_den < min_num * risk {
            return Err("unrepresentable minimum RR entry".into());
        }
        let take_profit = if reward * max_den > max_num * risk {
            let distance = (risk * max_num / (max_den * i128::from(tick))) * i128::from(tick);
            if distance * min_den < risk * min_num {
                return Err("unrepresentable RR cap".into());
            }
            i64::try_from(i128::from(entry) + sign * distance).map_err(|_| "target overflow")?
        } else {
            target
        };
        Ok(Self {
            id: id.into(),
            side,
            entry,
            stop,
            target,
            take_profit,
            state,
            last_quote_time: q.time,
        })
    }
    pub fn observe(&mut self, q: Quote) {
        if !q.valid()
            || q.time < self.last_quote_time
            || !matches!(self.state, SignalState::Pending | SignalState::Active)
        {
            return;
        }
        self.last_quote_time = q.time;
        let exit = if self.side == Side::Long {
            q.bid
        } else {
            q.ask
        };
        let entry = if self.side == Side::Long {
            q.ask
        } else {
            q.bid
        };
        let stopped = if self.side == Side::Long {
            exit <= self.stop
        } else {
            exit >= self.stop
        };
        let reached = if self.side == Side::Long {
            exit >= self.take_profit
        } else {
            exit <= self.take_profit
        };
        if self.state == SignalState::Pending {
            if stopped || reached {
                self.state = SignalState::Cancelled;
                return;
            }
            if (self.side == Side::Long && entry <= self.entry)
                || (self.side == Side::Short && entry >= self.entry)
            {
                self.state = SignalState::Active
            }
        } else if stopped {
            self.state = SignalState::Stop
        } else if reached {
            self.state = SignalState::TakeProfit
        }
    }
}

impl Quote {
    pub fn valid(self) -> bool {
        self.bid > 0 && self.ask >= self.bid
    }
}

/// Exact decimal RR within a bounded scale, keeping i128 products safe for i64 prices.
pub fn rr_ratio(value: f64) -> Result<(i128, i128), String> {
    if !value.is_finite() || value <= 0.0 || value > 1_000_000.0 {
        return Err("invalid RR".into());
    }
    let text = value.to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    if fraction.len() > 9 {
        return Err("RR precision exceeds nine decimals".into());
    }
    let den = 10_i128.pow(fraction.len() as u32);
    let num = whole.parse::<i128>().map_err(|_| "invalid RR")? * den
        + if fraction.is_empty() {
            0
        } else {
            fraction.parse::<i128>().map_err(|_| "invalid RR")?
        };
    Ok((num, den))
}
