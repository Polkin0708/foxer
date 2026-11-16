//! "Swing" rule set (requirements: Правила «свинг»). Pure calculation: closed bars and
//! minute extremes in, plans and trades out. Prices are floating point; time is seconds
//! on the source bar grid.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tf {
    W,
    D,
    H4,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub end: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}
/// One minute of an instrument without its time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ohlc {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}
/// ICE dollar index weights in the order EURUSD, USDJPY, GBPUSD, USDCAD, USDSEK, USDCHF.
pub const DOLLAR_INDEX_WEIGHTS: [f64; 6] = [-0.576, 0.136, -0.119, 0.091, 0.042, 0.036];
pub const DOLLAR_INDEX_CONSTANT: f64 = 50.14348112;
/// Synthetic dollar index for one minute of the six pairs (order of DOLLAR_INDEX_WEIGHTS).
pub fn dollar_index(pairs: &[Ohlc; 6]) -> Ohlc {
    let base = DOLLAR_INDEX_CONSTANT.ln();
    let (mut o, mut c, mut h, mut l) = (base, base, base, base);
    for (p, w) in pairs.iter().zip(DOLLAR_INDEX_WEIGHTS) {
        o += w * p.open.ln();
        c += w * p.close.ln();
        let (up, down) = if w > 0.0 {
            (p.high, p.low)
        } else {
            (p.low, p.high)
        };
        h += w * up.ln();
        l += w * down.ln();
    }
    let (open, close) = (o.exp(), c.exp());
    Ohlc {
        open,
        high: h.exp().max(open.max(close)),
        low: l.exp().min(open.min(close)),
        close,
    }
}
/// Event recorded by a rule set; the exit by the index reads them from the index run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Event {
    Confirm,
    Sweep,
    Target,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceExit {
    /// Index direction matching a trade direction (-1 for EURUSD).
    pub sign: i8,
    /// Number of the latest known index events that are examined.
    pub recent_events: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeProfit {
    Fixed,
    Target,
}
/// Range trading parameters (requirements: Торговля в боковике).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RangeConfig {
    pub enabled: bool,
    pub min_h4_bars: usize,
    pub window_min: usize,
    pub window_max: usize,
    pub height_min_atr_d: f64,
    pub height_max_atr_d: f64,
    pub impulse_bars: usize,
    pub impulse_min_atr_d: f64,
    pub impulse_min_ratio: f64,
    pub break_fraction: f64,
    /// Minimal wick beyond a border, in price units.
    pub wick_min: f64,
    pub max_stops_per_border: u32,
}
impl Default for RangeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_h4_bars: 32,
            window_min: 12,
            window_max: 60,
            height_min_atr_d: 0.4,
            height_max_atr_d: 1.5,
            impulse_bars: 12,
            impulse_min_atr_d: 1.5,
            impulse_min_ratio: 1.2,
            break_fraction: 0.5,
            wick_min: 0.001,
            max_stops_per_border: 2,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SwingConfig {
    pub atr_period: usize,
    pub rr_fixed: f64,
    pub take_profit: TakeProfit,
    pub rr_target_cap: f64,
    pub plan_days: usize,
    pub stop_lookback_h4_bars: usize,
    pub stop_buffer_atr: f64,
    /// Correction plans confirm on H4 when no live daily zone opposes them before the target.
    pub correction_h4_confirm_without_opposing_zone: bool,
    /// Hours of day (source grid) of H4 closes at which an entry may be taken.
    pub entry_close_hours: Vec<i64>,
    /// Execution cost in price units, paid against the trade on entry.
    pub cost: f64,
    /// Bars before this time only build context.
    pub start: i64,
    pub range: RangeConfig,
    pub reference_exit: Option<ReferenceExit>,
}
impl Default for SwingConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,
            rr_fixed: 2.0,
            take_profit: TakeProfit::Fixed,
            rr_target_cap: 5.0,
            plan_days: 10,
            stop_lookback_h4_bars: 30,
            stop_buffer_atr: 0.05,
            correction_h4_confirm_without_opposing_zone: true,
            entry_close_hours: vec![0, 8, 12, 16, 20],
            cost: 0.0,
            start: 0,
            range: RangeConfig::default(),
            reference_exit: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fractal {
    pub idx: usize,
    pub price: f64,
    pub swept: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gap {
    pub dir: i8,
    pub lo: f64,
    pub hi: f64,
    pub idx: usize,
    pub alive: bool,
    pub inverted_at: Option<usize>,
}
/// Closed bars of one timeframe with fractals, gaps and ATR.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Frame {
    atr_period: usize,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub fr_hi: Vec<Fractal>,
    pub fr_lo: Vec<Fractal>,
    pub fvg: Vec<Gap>,
}
impl Frame {
    pub fn new(atr_period: usize) -> Self {
        Self {
            atr_period,
            ..Self::default()
        }
    }
    pub fn len(&self) -> usize {
        self.close.len()
    }
    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }
    pub fn push(&mut self, b: &Bar) {
        self.high.push(b.high);
        self.low.push(b.low);
        self.close.push(b.close);
        let i = self.close.len() - 1;
        for f in &mut self.fr_hi {
            if !f.swept && b.high > f.price {
                f.swept = true;
            }
        }
        for f in &mut self.fr_lo {
            if !f.swept && b.low < f.price {
                f.swept = true;
            }
        }
        for z in &mut self.fvg {
            if z.alive && ((z.dir > 0 && b.close < z.lo) || (z.dir < 0 && b.close > z.hi)) {
                z.alive = false;
                z.inverted_at = Some(i);
            }
        }
        if i >= 2 {
            let (h, l) = (&self.high, &self.low);
            if h[i - 1] > h[i - 2] && h[i - 1] > h[i] {
                self.fr_hi.push(Fractal {
                    idx: i - 1,
                    price: h[i - 1],
                    swept: false,
                });
            }
            if l[i - 1] < l[i - 2] && l[i - 1] < l[i] {
                self.fr_lo.push(Fractal {
                    idx: i - 1,
                    price: l[i - 1],
                    swept: false,
                });
            }
            if l[i] > h[i - 2] {
                self.fvg.push(Gap {
                    dir: 1,
                    lo: h[i - 2],
                    hi: l[i],
                    idx: i,
                    alive: true,
                    inverted_at: None,
                });
            }
            if h[i] < l[i - 2] {
                self.fvg.push(Gap {
                    dir: -1,
                    lo: h[i],
                    hi: l[i - 2],
                    idx: i,
                    alive: true,
                    inverted_at: None,
                });
            }
        }
    }
    /// Mean bar range of the last `atr_period` bars. The summation order matches the
    /// reference prototype so that results are reproducible bit for bit.
    pub fn atr(&self) -> Option<f64> {
        let n = self.high.len();
        if n < 2 {
            return None;
        }
        let from = n.saturating_sub(self.atr_period);
        let a: Vec<f64> = (from..n).map(|k| self.high[k] - self.low[k]).collect();
        let sum = if a.len() < 8 {
            a.iter().fold(0.0, |s, v| s + v)
        } else {
            let mut r = [0.0; 8];
            r.copy_from_slice(&a[..8]);
            let whole = a.len() - a.len() % 8;
            let mut k = 8;
            while k < whole {
                for j in 0..8 {
                    r[j] += a[k + j];
                }
                k += 8;
            }
            let mut s = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
            for v in &a[whole..] {
                s += v;
            }
            s
        };
        Some(sum / a.len() as f64)
    }
    /// +1 up, -1 down, 0 none.
    pub fn structure(&self) -> i8 {
        let (nh, nl) = (self.fr_hi.len(), self.fr_lo.len());
        if nh < 2 || nl < 2 {
            return 0;
        }
        let (h2, h1) = (self.fr_hi[nh - 2].price, self.fr_hi[nh - 1].price);
        let (l2, l1) = (self.fr_lo[nl - 2].price, self.fr_lo[nl - 1].price);
        if h1 > h2 && l1 > l2 {
            1
        } else if h1 < h2 && l1 < l2 {
            -1
        } else {
            0
        }
    }
    fn latest_live_gap_behind(&self, dir: i8, price: f64) -> Option<&Gap> {
        self.fvg.iter().rev().find(|z| {
            z.alive
                && z.dir == dir
                && if dir > 0 {
                    z.hi <= price
                } else {
                    z.lo >= price
                }
        })
    }
    fn nearest_unswept_fractal(&self, dir: i8, price: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        if dir > 0 {
            for f in self.fr_hi.iter().filter(|f| !f.swept && f.price > price) {
                if best.is_none_or(|b| f.price < b) {
                    best = Some(f.price);
                }
            }
        } else {
            for f in self.fr_lo.iter().filter(|f| !f.swept && f.price < price) {
                if best.is_none_or(|b| f.price > b) {
                    best = Some(f.price);
                }
            }
        }
        best
    }
    fn nearest_fractal_beyond(&self, dir: i8, level: f64, lookback: usize) -> Option<f64> {
        let n = self.close.len() as i64;
        let recent = |f: &&Fractal| f.idx as i64 >= n - lookback as i64;
        let mut best: Option<f64> = None;
        if dir > 0 {
            for f in self.fr_lo.iter().filter(recent).filter(|f| f.price < level) {
                if best.is_none_or(|b| f.price > b) {
                    best = Some(f.price);
                }
            }
        } else {
            for f in self.fr_hi.iter().filter(recent).filter(|f| f.price > level) {
                if best.is_none_or(|b| f.price < b) {
                    best = Some(f.price);
                }
            }
        }
        best
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Trend,
    Correction,
    Range,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RangeBox {
    id: u64,
    top: f64,
    bot: f64,
    /// Stops taken from the bottom (index 0, longs) and the top (index 1, shorts).
    stops: [u32; 2],
}
fn border(dir: i8) -> usize {
    usize::from(dir < 0)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PlanState {
    WaitConfirm,
    WaitEntry,
    Active,
    Done,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Plan {
    kind: Kind,
    dir: i8,
    poi: (f64, f64),
    target: f64,
    conf_tf: Tf,
    state: PlanState,
    created_day: usize,
    h4_idx: usize,
    #[serde(default)]
    target_hit: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeState {
    Pending,
    Active,
    Stop,
    Target,
    Cancelled,
    ReferenceExit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    pub kind: Kind,
    pub dir: i8,
    pub entry: f64,
    pub stop: f64,
    pub tp: f64,
    pub rr: f64,
    pub signal_t: i64,
    pub fill_t: Option<i64>,
    pub state: TradeState,
    pub r: Option<f64>,
    pub exit_t: Option<i64>,
    plan: Option<usize>,
    range_box: Option<u64>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizing {
    pub entry: f64,
    pub tp: f64,
    pub rr: f64,
    pub limit: bool,
}
/// Entry price, take profit and order type for a signal at `close` with a fixed `stop`.
pub fn size_entry(dir: i8, close: f64, stop: f64, target: f64, c: &SwingConfig) -> Option<Sizing> {
    let d = f64::from(dir);
    let risk = (close - stop) * d;
    if risk <= 0.0 {
        return None;
    }
    if (target - close) * d / risk >= c.rr_fixed {
        let (tp, rr) = match c.take_profit {
            TakeProfit::Fixed => (close + c.rr_fixed * risk * d, c.rr_fixed),
            TakeProfit::Target => {
                let cap = close + c.rr_target_cap * risk * d;
                let tp = if dir > 0 {
                    target.min(cap)
                } else {
                    target.max(cap)
                };
                (tp, (tp - close) * d / risk)
            }
        };
        return Some(Sizing {
            entry: close,
            tp,
            rr,
            limit: false,
        });
    }
    let entry = (target + c.rr_fixed * stop) / (1.0 + c.rr_fixed);
    if (entry - stop) * d <= 0.0 || (dir > 0 && entry > close) || (dir < 0 && entry < close) {
        return None;
    }
    Some(Sizing {
        entry,
        tp: target,
        rr: c.rr_fixed,
        limit: true,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Swing {
    cfg: SwingConfig,
    w: Frame,
    d: Frame,
    h4: Frame,
    plans: Vec<Plan>,
    trades: Vec<Trade>,
    d_count: usize,
    rbox: Option<RangeBox>,
    boxes: u64,
    last_impulse: Option<i64>,
    events: Vec<(i64, Event, i8)>,
}
impl Swing {
    pub fn new(cfg: SwingConfig) -> Self {
        let n = cfg.atr_period;
        Self {
            cfg,
            w: Frame::new(n),
            d: Frame::new(n),
            h4: Frame::new(n),
            plans: vec![],
            trades: vec![],
            d_count: 0,
            rbox: None,
            boxes: 0,
            last_impulse: None,
            events: vec![],
        }
    }
    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }
    /// Events recorded so far: (time, event, direction).
    pub fn events(&self) -> &[(i64, Event, i8)] {
        &self.events
    }
    /// Current range borders (top, bottom).
    pub fn range_box(&self) -> Option<(f64, f64)> {
        self.rbox.as_ref().map(|b| (b.top, b.bot))
    }
    pub fn frame(&self, tf: Tf) -> &Frame {
        match tf {
            Tf::W => &self.w,
            Tf::D => &self.d,
            Tf::H4 => &self.h4,
        }
    }
    /// Registers a plan unless the same idea is still open. Returns whether it was added.
    pub fn add_plan(
        &mut self,
        kind: Kind,
        dir: i8,
        poi: (f64, f64),
        target: f64,
        conf_tf: Tf,
    ) -> bool {
        if self
            .plans
            .iter()
            .any(|p| p.kind == kind && p.dir == dir && p.poi == poi && p.state != PlanState::Done)
        {
            return false;
        }
        let mut conf_tf = conf_tf;
        if self.cfg.correction_h4_confirm_without_opposing_zone && conf_tf == Tf::D {
            let opposed = self.d.close.last().is_some_and(|&px| {
                self.d.fvg.iter().any(|z| {
                    z.alive
                        && z.dir == -dir
                        && if dir > 0 {
                            px <= z.lo && z.lo < target
                        } else {
                            target < z.hi && z.hi <= px
                        }
                })
            });
            if !opposed {
                conf_tf = Tf::H4;
            }
        }
        self.plans.push(Plan {
            kind,
            dir,
            poi,
            target,
            conf_tf,
            state: PlanState::WaitConfirm,
            created_day: self.d_count,
            h4_idx: self.h4.len(),
            target_hit: false,
        });
        true
    }
    /// A bar of `tf` closed; `now` is the time at which the close is observed.
    pub fn close(&mut self, tf: Tf, b: Bar, now: i64) {
        match tf {
            Tf::W => self.w.push(&b),
            Tf::D => self.d.push(&b),
            Tf::H4 => self.h4.push(&b),
        }
        if now < self.cfg.start {
            return;
        }
        if tf == Tf::D {
            self.d_count += 1;
        }
        if tf == Tf::W {
            self.weekly_plans(&b);
        }
        if tf == Tf::H4 && self.cfg.range.enabled && self.h4.len() > self.cfg.range.min_h4_bars {
            self.range_step(&b);
        }
        for k in 0..self.plans.len() {
            self.advance(k, tf, &b);
        }
    }
    fn weekly_plans(&mut self, b: &Bar) {
        let s = self.w.structure();
        if s == 0 {
            return;
        }
        let zone = self
            .w
            .latest_live_gap_behind(s, b.close)
            .map(|z| (z.lo, z.hi));
        let target = self.w.nearest_unswept_fractal(s, b.close);
        if let (Some((lo, hi)), Some(target)) = (zone, target) {
            if (s > 0 && b.low <= hi) || (s < 0 && b.high >= lo) {
                self.add_plan(Kind::Trend, s, (lo, hi), target, Tf::H4);
            }
        }
        let n = self.w.len();
        let (prev_high, prev_low) = (self.w.high[n - 2], self.w.low[n - 2]);
        let fractals = if s > 0 { &self.w.fr_hi } else { &self.w.fr_lo };
        let passed: Vec<f64> = fractals
            .iter()
            .filter(|f| {
                f.swept
                    && if s > 0 {
                        b.high > f.price && f.price >= prev_high
                    } else {
                        b.low < f.price && f.price <= prev_low
                    }
            })
            .map(|f| f.price)
            .collect();
        for price in passed {
            if let Some((lo, hi)) = zone {
                let target = if s > 0 { hi } else { lo };
                self.add_plan(Kind::Correction, -s, (price, price), target, Tf::D);
            }
        }
    }
    fn advance(&mut self, k: usize, tf: Tf, b: &Bar) {
        let p = self.plans[k].clone();
        if p.state == PlanState::Done {
            return;
        }
        if self.d_count as i64 - p.created_day as i64 > self.cfg.plan_days as i64
            && p.state != PlanState::Active
        {
            self.plans[k].state = PlanState::Done;
            return;
        }
        let beyond = (p.dir > 0 && b.close < p.poi.0) || (p.dir < 0 && b.close > p.poi.1);
        if beyond
            && ((tf == Tf::W && p.kind == Kind::Trend) || (tf == Tf::D && p.kind != Kind::Trend))
        {
            self.plans[k].state = PlanState::Done;
            return;
        }
        if self.plans[k].state == PlanState::WaitConfirm && tf == p.conf_tf {
            let frame = self.frame(tf);
            let i = frame.len() - 1;
            if frame.fvg.iter().any(|z| {
                (z.dir == p.dir && z.idx == i) || (z.dir == -p.dir && z.inverted_at == Some(i))
            }) {
                self.plans[k].state = PlanState::WaitEntry;
                self.plans[k].h4_idx = self.h4.len() - usize::from(tf == Tf::H4);
                self.events.push((b.end, Event::Confirm, p.dir));
            }
        }
        if self.plans[k].state != PlanState::WaitEntry || tf != Tf::H4 {
            return;
        }
        let i = self.h4.len() - 1;
        let from = self.plans[k].h4_idx;
        let Some(zone) = self
            .h4
            .fvg
            .iter()
            .rev()
            .find(|z| z.dir == p.dir && z.idx == i && z.idx >= from)
            .cloned()
        else {
            return;
        };
        if !self.entry_hour(b.end) {
            return;
        }
        let d = f64::from(p.dir);
        let level = if p.dir > 0 {
            b.close.min(zone.lo)
        } else {
            b.close.max(zone.hi)
        };
        let reference = self
            .h4
            .nearest_fractal_beyond(p.dir, level, self.cfg.stop_lookback_h4_bars)
            .unwrap_or(if p.dir > 0 { zone.lo } else { zone.hi });
        let stop = reference - d * self.cfg.stop_buffer_atr * self.h4.atr().unwrap_or(0.0);
        let Some(sizing) = size_entry(p.dir, b.close, stop, p.target, &self.cfg) else {
            return;
        };
        let market = !sizing.limit;
        self.trades.push(Trade {
            kind: p.kind,
            dir: p.dir,
            entry: if market {
                b.close + d * self.cfg.cost
            } else {
                sizing.entry
            },
            stop,
            tp: sizing.tp,
            rr: sizing.rr,
            signal_t: b.end,
            fill_t: market.then_some(b.end),
            state: if market {
                TradeState::Active
            } else {
                TradeState::Pending
            },
            r: None,
            exit_t: None,
            plan: Some(k),
            range_box: None,
        });
        self.plans[k].state = PlanState::Active;
    }
    /// Exit by the index: `known` are the index events with time not after `t`, in order.
    pub fn reference_minute(&mut self, t: i64, close: f64, known: &[(i64, Event, i8)]) {
        let Some(rx) = &self.cfg.reference_exit else {
            return;
        };
        if t < self.cfg.start {
            return;
        }
        let recent = &known[known.len().saturating_sub(rx.recent_events)..];
        let sign = rx.sign;
        for tr in &mut self.trades {
            if tr.state != TradeState::Active {
                continue;
            }
            let filled = tr.fill_t.unwrap_or(tr.signal_t);
            let want = tr.dir * sign;
            if !recent.iter().any(|&(at, ev, dir)| {
                matches!(ev, Event::Target | Event::Sweep) && dir == want && filled < at && at <= t
            }) {
                continue;
            }
            let d = f64::from(tr.dir);
            tr.state = TradeState::ReferenceExit;
            tr.r = Some((close - tr.entry) * d / ((tr.entry - tr.stop) * d));
            tr.exit_t = Some(t);
            if let Some(k) = tr.plan {
                self.plans[k].state = PlanState::Done;
            }
            if tr.kind == Kind::Range {
                self.rbox = None;
            }
        }
    }
    fn entry_hour(&self, end: i64) -> bool {
        self.cfg
            .entry_close_hours
            .contains(&(end.rem_euclid(86_400) / 3600))
    }
    fn estimate_box(&mut self) -> Option<RangeBox> {
        let rc = &self.cfg.range;
        let a = self.d.atr().filter(|a| *a > 0.0)?;
        let (h, l, n) = (&self.h4.high, &self.h4.low, self.h4.len());
        let mut best: Option<(usize, f64, f64)> = None;
        for k in rc.window_min..=rc.window_max.min(n - 1) {
            let (from, to) = (n - k - 1, n - 1);
            let top = h[from..to].iter().fold(f64::NEG_INFINITY, |m, v| m.max(*v));
            let bot = l[from..to].iter().fold(f64::INFINITY, |m, v| m.min(*v));
            if top - bot > rc.height_max_atr_d * a {
                break;
            }
            if top - bot >= rc.height_min_atr_d * a {
                best = Some((k, top, bot));
            }
        }
        let (k, top, bot) = best?;
        if n - k - 1 < rc.impulse_bars {
            return None;
        }
        let (from, to) = (n - k - 1 - rc.impulse_bars, n - k - 1);
        let travel = h[from..to].iter().fold(f64::NEG_INFINITY, |m, v| m.max(*v))
            - l[from..to].iter().fold(f64::INFINITY, |m, v| m.min(*v));
        if travel < rc.impulse_min_atr_d * a || travel < rc.impulse_min_ratio * (top - bot) {
            return None;
        }
        let impulse = (n - k - rc.impulse_bars) as i64;
        if self
            .last_impulse
            .is_some_and(|last| impulse <= last + rc.impulse_bars as i64)
        {
            return None;
        }
        self.last_impulse = Some(impulse);
        self.boxes += 1;
        Some(RangeBox {
            id: self.boxes,
            top,
            bot,
            stops: [0, 0],
        })
    }
    fn range_step(&mut self, b: &Bar) {
        match &self.rbox {
            None => self.rbox = self.estimate_box(),
            Some(r) => {
                let height = r.top - r.bot;
                let f = self.cfg.range.break_fraction;
                if b.close > r.top + f * height || b.close < r.bot - f * height {
                    self.rbox = None;
                }
            }
        }
        let Some(r) = self.rbox.clone() else { return };
        if self.trades.iter().any(|t| {
            t.kind == Kind::Range && matches!(t.state, TradeState::Pending | TradeState::Active)
        }) {
            return;
        }
        let n = self.h4.len();
        let (prev_close, prev_high, prev_low) = (
            self.h4.close[n - 2],
            self.h4.high[n - 2],
            self.h4.low[n - 2],
        );
        let a4 = self.h4.atr().unwrap_or(0.0);
        let rc = &self.cfg.range;
        for (dir, edge, extreme) in [(-1i8, r.top, b.high), (1i8, r.bot, b.low)] {
            let d = f64::from(dir);
            let deviation = if dir < 0 {
                extreme > edge
            } else {
                extreme < edge
            };
            let inside = r.bot <= b.close && b.close <= r.top;
            let prev_out = if dir < 0 {
                prev_close > r.top
            } else {
                prev_close < r.bot
            };
            let wick = (extreme - edge).abs() >= rc.wick_min;
            if !inside || r.stops[border(dir)] >= rc.max_stops_per_border {
                continue;
            }
            if !((deviation && wick) || prev_out) || !self.entry_hour(b.end) {
                continue;
            }
            let dev_ext = if dir < 0 {
                extreme.max(prev_high)
            } else {
                extreme.min(prev_low)
            };
            let stop = if dir < 0 {
                dev_ext + self.cfg.stop_buffer_atr * a4
            } else {
                dev_ext - self.cfg.stop_buffer_atr * a4
            };
            let target = if dir < 0 { r.bot } else { r.top };
            let mut entry = b.close;
            let mut risk = (entry - stop) * d;
            if risk <= 0.0 {
                continue;
            }
            let rr_target = (target - entry) * d / risk;
            let rr_fixed = self.cfg.rr_fixed;
            let limit = rr_target < rr_fixed;
            if limit {
                entry = (target + rr_fixed * stop) / (1.0 + rr_fixed);
                risk = (entry - stop) * d;
                if risk <= 0.0 || !(r.bot <= entry && entry <= r.top) {
                    continue;
                }
            }
            let tp = if (target - entry).abs() > rr_fixed * risk {
                entry + rr_fixed * risk * d
            } else {
                target
            };
            self.trades.push(Trade {
                kind: Kind::Range,
                dir,
                entry: if limit {
                    entry
                } else {
                    entry + d * self.cfg.cost
                },
                stop,
                tp,
                rr: if limit { rr_fixed } else { rr_target },
                signal_t: b.end,
                fill_t: (!limit).then_some(b.end),
                state: if limit {
                    TradeState::Pending
                } else {
                    TradeState::Active
                },
                r: None,
                exit_t: None,
                plan: None,
                range_box: Some(r.id),
            });
            self.events.push((b.end, Event::Sweep, dir));
            break;
        }
    }
    /// One minute of quotes: `high` and `low` of the minute starting at `t`.
    pub fn minute(&mut self, t: i64, high: f64, low: f64) {
        if t < self.cfg.start {
            return;
        }
        for p in &mut self.plans {
            if p.state != PlanState::Done
                && !p.target_hit
                && ((p.dir > 0 && high >= p.target) || (p.dir < 0 && low <= p.target))
            {
                p.target_hit = true;
                self.events.push((t, Event::Target, p.dir));
            }
        }
        for tr in &mut self.trades {
            let d = f64::from(tr.dir);
            let long = tr.dir > 0;
            if tr.state == TradeState::Pending {
                let target_first = if long { high >= tr.tp } else { low <= tr.tp };
                let plan_done = tr
                    .plan
                    .is_some_and(|k| self.plans[k].state == PlanState::Done);
                if plan_done || target_first {
                    tr.state = TradeState::Cancelled;
                    if let Some(k) = tr.plan {
                        self.plans[k].state = PlanState::Done;
                    }
                    continue;
                }
                if if long {
                    low <= tr.entry
                } else {
                    high >= tr.entry
                } {
                    tr.state = TradeState::Active;
                    tr.fill_t = Some(t);
                    tr.entry += d * self.cfg.cost;
                }
            }
            if tr.state != TradeState::Active {
                continue;
            }
            let risk = (tr.entry - tr.stop) * d;
            if if long {
                low <= tr.stop
            } else {
                high >= tr.stop
            } {
                tr.state = TradeState::Stop;
                tr.r = Some(-1.0);
            } else if if long { high >= tr.tp } else { low <= tr.tp } {
                tr.state = TradeState::Target;
                tr.r = Some((tr.tp - tr.entry) * d / risk);
            } else {
                continue;
            }
            tr.exit_t = Some(t);
            if let Some(k) = tr.plan {
                self.plans[k].state = PlanState::Done;
            }
            if tr.kind == Kind::Range {
                if tr.state == TradeState::Stop {
                    if let Some(b) = self.rbox.as_mut().filter(|b| Some(b.id) == tr.range_box) {
                        b.stops[border(tr.dir)] += 1;
                    }
                }
                self.rbox = None;
            }
        }
    }
}
