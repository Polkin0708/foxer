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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeProfit {
    Fixed,
    Target,
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
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeState {
    Pending,
    Active,
    Stop,
    Target,
    Cancelled,
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
    plan: usize,
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
        }
    }
    pub fn trades(&self) -> &[Trade] {
        &self.trades
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
        if !self
            .cfg
            .entry_close_hours
            .contains(&(b.end.rem_euclid(86_400) / 3600))
        {
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
            plan: k,
        });
        self.plans[k].state = PlanState::Active;
    }
    /// One minute of quotes: `high` and `low` of the minute starting at `t`.
    pub fn minute(&mut self, t: i64, high: f64, low: f64) {
        if t < self.cfg.start {
            return;
        }
        for tr in &mut self.trades {
            let d = f64::from(tr.dir);
            let long = tr.dir > 0;
            if tr.state == TradeState::Pending {
                let target_first = if long { high >= tr.tp } else { low <= tr.tp };
                if self.plans[tr.plan].state == PlanState::Done || target_first {
                    tr.state = TradeState::Cancelled;
                    self.plans[tr.plan].state = PlanState::Done;
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
            self.plans[tr.plan].state = PlanState::Done;
        }
    }
}
