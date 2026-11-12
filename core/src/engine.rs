use crate::{model::Strategy, trading::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Timeframe {
    W,
    D,
    H4,
}
#[derive(Clone, Serialize, Deserialize)]
struct WeightedZone {
    zone: Zone,
    atr: f64,
}
#[derive(Default, Clone, Serialize, Deserialize)]
struct Frame {
    bars: Vec<Candle>,
    zones: Vec<WeightedZone>,
    fractals: Vec<Fractal>,
    trend: Option<Side>,
}
struct Delta {
    previous: Option<Side>,
    new: Vec<Zone>,
    inverted: Vec<Zone>,
    duplicate: bool,
}
fn alive(z: &Zone) -> bool {
    matches!(z.state, ZoneState::Alive | ZoneState::Tested)
}
fn zone_rank(z: &Zone, price: i64) -> (i64, std::cmp::Reverse<i128>) {
    let distance = if price < z.low {
        i128::from(z.low) - i128::from(price)
    } else if price > z.high {
        i128::from(price) - i128::from(z.high)
    } else {
        0
    };
    (z.formed_at, std::cmp::Reverse(distance))
}
fn opposite(s: Side) -> Side {
    match s {
        Side::Long => Side::Short,
        Side::Short => Side::Long,
    }
}
impl Frame {
    fn update(&mut self, b: Candle, limit: usize, c: &Strategy) -> Result<Delta, String> {
        let previous = self.trend;
        let mut delta = Delta {
            previous,
            new: vec![],
            inverted: vec![],
            duplicate: false,
        };
        if b.low <= 0
            || b.high < b.low
            || b.open < b.low
            || b.open > b.high
            || b.close < b.low
            || b.close > b.high
        {
            return Err("invalid closed bar".into());
        }
        if let Some(last) = self.bars.last() {
            if b.end <= last.end {
                if b.end == last.end
                    && serde_json::to_value(&b).ok() == serde_json::to_value(last).ok()
                {
                    delta.duplicate = true;
                    return Ok(delta);
                }
                return Err("conflicting or out-of-order closed bar".into());
            }
        }
        self.bars.push(b.clone());
        let average = atr(&self.bars, c.parameters.atr_period.value as usize);
        for f in &mut self.fractals {
            if (f.upper && b.high > f.price) || (!f.upper && b.low < f.price) {
                f.swept = true;
            }
        }
        self.fractals.extend(fractals(&self.bars));
        if let Some(a) = average {
            for z in &mut self.zones {
                let before = z.zone.state;
                z.zone.observe(
                    &b,
                    if c.parameters.atr_reference_zone.value == "formation_bar" {
                        z.atr
                    } else {
                        a
                    },
                    c.parameters.inv_margin_atr.value,
                );
                if before != ZoneState::Inverted && z.zone.state == ZoneState::Inverted {
                    delta.inverted.push(z.zone.clone());
                }
            }
            // A gap originates at the open; a three-bar FVG becomes known at the close.
            for z in [
                gap(&self.bars, a, c.parameters.min_fvg_atr.value),
                fvg(&self.bars, a, c.parameters.min_fvg_atr.value),
            ]
            .into_iter()
            .flatten()
            {
                delta.new.push(z.clone());
                self.zones.push(WeightedZone { zone: z, atr: a });
            }
            let weight = c.parameters.trend_fvg_atr.value;
            let weighted = |z: &&WeightedZone| {
                (z.zone.high - z.zone.low) as f64
                    >= weight
                        * if c.parameters.atr_reference_zone.value == "formation_bar" {
                            z.atr
                        } else {
                            a
                        }
            };
            if let Some(dir) = previous {
                let inversion = self.zones.iter().filter(weighted).any(|z| {
                    z.zone.side == dir
                        && delta.inverted.iter().any(|i| {
                            i.formed_at == z.zone.formed_at
                                && i.low == z.zone.low
                                && i.high == z.zone.high
                        })
                });
                let remaining = self
                    .zones
                    .iter()
                    .filter(weighted)
                    .any(|z| z.zone.side == dir && alive(&z.zone));
                let opposing = self
                    .zones
                    .iter()
                    .filter(weighted)
                    .rev()
                    .find(|z| z.zone.side != dir && alive(&z.zone));
                if inversion {
                    self.trend = opposing.map(|z| z.zone.side);
                } else if !remaining
                    && delta
                        .new
                        .iter()
                        .any(|z| z.side != dir && (z.high - z.low) as f64 >= weight * a)
                {
                    self.trend = Some(opposite(dir));
                }
            } else {
                let mut directions = delta
                    .new
                    .iter()
                    .filter(|z| (z.high - z.low) as f64 >= weight * a)
                    .map(|z| z.side);
                if let Some(first) = directions.next() {
                    if directions.all(|s| s == first) {
                        self.trend = Some(first);
                    }
                }
            }
        }
        if self.bars.len() > limit {
            self.bars.drain(..self.bars.len() - limit);
        }
        if let Some(first) = self.bars.first() {
            self.fractals.retain(|f| f.confirmed_at >= first.end);
            self.zones.retain(|z| z.zone.formed_at >= first.end);
        }
        Ok(delta)
    }
    fn latest_zone(&self, side: Side, price: i64) -> Option<Zone> {
        self.zones
            .iter()
            .filter(|z| z.zone.side == side && z.zone.state != ZoneState::Reacted)
            .max_by_key(|z| zone_rank(&z.zone, price))
            .filter(|z| alive(&z.zone))
            .map(|z| z.zone.clone())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum PlanState {
    WaitConfirm,
    WaitEntry,
    Ready,
    Setup,
    Active,
    Done,
}
#[derive(Clone, Serialize, Deserialize)]
struct Plan {
    id: String,
    rule: String,
    kind: String,
    side: Side,
    poi_tf: Timeframe,
    poi: Zone,
    started: i64,
    state: PlanState,
    entry_zone: Option<Zone>,
    #[serde(default)]
    entry_is_ifvg: bool,
    #[serde(default)]
    entry_atr: Option<f64>,
    #[serde(default)]
    entry_close: Option<i64>,
    signal: Option<String>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Engine {
    frames: BTreeMap<Timeframe, Frame>,
    plans: Vec<Plan>,
    signals: Vec<Signal>,
    weekly_zone_touch: Option<i64>,
    weekly_fractal_sweep: Option<i64>,
}
impl Engine {
    pub fn signal_basis(&self, id: &str) -> serde_json::Value {
        let plan = self.plans.iter().find(|p| p.signal.as_deref() == Some(id));
        let frames: std::collections::BTreeMap<_, _> = self
            .frames
            .iter()
            .map(|(tf, f)| {
                let start = f.bars.len().saturating_sub(120);
                (format!("{tf:?}"), &f.bars[start..])
            })
            .collect();
        serde_json::json!({"plan":plan,"frames":frames})
    }

    pub fn namespace_signals(&mut self, symbol: &str) {
        for signal in &mut self.signals {
            if !signal.id.starts_with(&format!("{symbol}:")) {
                let old = signal.id.clone();
                signal.id = format!("{symbol}:{old}");
                for p in &mut self.plans {
                    if p.signal.as_deref() == Some(&old) {
                        p.signal = Some(signal.id.clone());
                    }
                }
            }
        }
    }

    pub fn reconfigure(&mut self, c: &Strategy, _tick: i64) -> Result<Vec<Signal>, String> {
        let mut rebuilt = Engine::default();
        for (tf, frame) in &self.frames {
            for bar in &frame.bars {
                rebuilt.seed(*tf, bar.clone(), c)?;
            }
        }
        self.frames = rebuilt.frames;

        for p in &mut self.plans {
            if matches!(
                p.state,
                PlanState::WaitConfirm | PlanState::WaitEntry | PlanState::Ready
            ) {
                p.entry_zone = None;
                p.state = if p.rule == "W_point_A" {
                    PlanState::WaitConfirm
                } else {
                    PlanState::WaitEntry
                };
            }
        }
        Ok(Vec::new())
    }
    // Compatibility entrypoint: a stale delivery must never revise v4 levels.
    pub fn activate_revision(&mut self, _revision: &Signal) -> Result<bool, String> {
        Ok(false)
    }
    pub fn replace_context(&mut self, context: Engine) {
        self.frames = context.frames;
        self.weekly_zone_touch = None;
        self.weekly_fractal_sweep = None;
        self.plans.retain(|p| p.signal.is_some());
    }
    pub fn infer_legacy_entries(&mut self) {
        for p in &mut self.plans {
            if let Some(z) = &p.entry_zone {
                p.entry_is_ifvg = z.side != p.side;
                if p.entry_atr.is_none() {
                    p.entry_atr = self.frames.get(&Timeframe::H4).and_then(|f| {
                        f.zones
                            .iter()
                            .find(|w| {
                                w.zone.formed_at == z.formed_at
                                    && w.zone.low == z.low
                                    && w.zone.high == z.high
                                    && w.zone.kind == z.kind
                            })
                            .map(|w| w.atr)
                    });
                }
            }
        }
    }
    fn expire(&mut self, now: i64, c: &Strategy) -> Result<(), String> {
        let age = i64::try_from(c.parameters.plan_days.value)
            .ok()
            .and_then(|n| n.checked_mul(86400))
            .ok_or("plan duration overflow")?;
        for p in &mut self.plans {
            let status = match p.state {
                PlanState::WaitConfirm => "wait_confirm",
                PlanState::WaitEntry | PlanState::Ready => "wait_entry",
                PlanState::Setup => "setup",
                _ => "",
            };
            if c.parameters
                .plan_days_statuses
                .value
                .iter()
                .any(|s| s == status)
                && now >= p.started
                && now.saturating_sub(p.started) >= age
            {
                p.state = PlanState::Done;
            }
        }
        self.cancel_plan_signals();
        Ok(())
    }
    pub fn track(&mut self, q: Quote) {
        for s in &mut self.signals {
            s.observe(q);
        }
    }
    fn update(&mut self, tf: Timeframe, b: Candle, c: &Strategy) -> Result<Delta, String> {
        let limit = match tf {
            Timeframe::W => c.context.bars_analyzed.w,
            Timeframe::D => c.context.bars_analyzed.d,
            Timeframe::H4 => c.context.bars_analyzed.h4,
        };
        self.frames
            .entry(tf)
            .or_default()
            .update(b, limit as usize, c)
    }
    pub fn seed(&mut self, tf: Timeframe, b: Candle, c: &Strategy) -> Result<(), String> {
        self.update(tf, b, c)?;
        Ok(())
    }
    pub fn trend(&self, tf: Timeframe) -> Option<Side> {
        self.frames.get(&tf).and_then(|f| f.trend)
    }
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
    pub fn search_status(&self, c: &Strategy) -> &'static str {
        if !self.ready(Timeframe::H4, c) {
            "недостаточно свечей H4"
        } else if self.plans.iter().any(|p| p.state == PlanState::Ready) {
            "подтверждение есть, условия входа не выполнены"
        } else if self.plans.iter().any(|p| p.state == PlanState::WaitEntry) {
            "ожидание входной зоны H4"
        } else if self.plans.iter().any(|p| p.state == PlanState::WaitConfirm) {
            "ожидание подтверждения H4"
        } else if self.plans.iter().any(|p| p.state == PlanState::Setup) {
            "ожидание лимитного входа"
        } else {
            "нет ожидающих планов"
        }
    }
    pub fn waiting_count(&self) -> usize {
        self.plans
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    PlanState::WaitConfirm
                        | PlanState::WaitEntry
                        | PlanState::Ready
                        | PlanState::Setup
                )
            })
            .count()
    }
    pub fn signals(&self) -> Vec<Signal> {
        self.signals.clone()
    }
    pub fn add_plan(&mut self, rule: &str, side: Side, tf: Timeframe, poi: Zone, now: i64) -> bool {
        let kind = match rule {
            "W_point_A" => "trend",
            "D_correction_start" | "D_correction_after_weekly_fractal" => "correction",
            "D_correction_end" | "D_continuation_from_weekly_zone" => "continuation",
            _ => return false,
        };
        let expected_tf = if rule == "W_point_A" {
            Timeframe::W
        } else {
            Timeframe::D
        };
        if tf != expected_tf {
            return false;
        }
        let id = format!(
            "{rule}:{tf:?}:{}:{side:?}:{}:{}:{:?}",
            poi.formed_at, poi.low, poi.high, poi.kind
        );
        if self.plans.iter().any(|p| p.id == id) {
            return false;
        }
        self.plans.push(Plan {
            id,
            rule: rule.into(),
            kind: kind.into(),
            side,
            poi_tf: tf,
            poi,
            started: now,
            state: if rule == "W_point_A" {
                PlanState::WaitConfirm
            } else {
                PlanState::WaitEntry
            },
            entry_zone: None,
            entry_is_ifvg: false,
            entry_atr: None,
            entry_close: None,
            signal: None,
        });
        true
    }
    fn ready(&self, tf: Timeframe, c: &Strategy) -> bool {
        let n = match tf {
            Timeframe::W => c.context.minimum_bars.w,
            Timeframe::D => c.context.minimum_bars.d,
            Timeframe::H4 => c.context.minimum_bars.h4,
        };
        self.frames
            .get(&tf)
            .is_some_and(|f| f.bars.len() as u64 >= n)
    }
    fn cancel_plan_signals(&mut self) {
        for p in &self.plans {
            if p.state == PlanState::Done {
                if let Some(id) = &p.signal {
                    if let Some(s) = self
                        .signals
                        .iter_mut()
                        .find(|s| &s.id == id && s.state == SignalState::Pending)
                    {
                        s.state = SignalState::Cancelled;
                    }
                }
            }
        }
    }
    pub fn closed(&mut self, tf: Timeframe, b: Candle, c: &Strategy) -> Result<(), String> {
        let delta = self.update(tf, b.clone(), c)?;
        if delta.duplicate {
            return Ok(());
        }
        match tf {
            Timeframe::W => {
                let new = self.trend(tf);
                if new != delta.previous {
                    for p in &mut self.plans {
                        if !matches!(p.state, PlanState::Active | PlanState::Done) {
                            if Some(p.side) != new {
                                p.state = PlanState::Done
                            } else if p.kind == "correction" {
                                p.kind = "trend".into();
                            }
                        }
                    }
                }
            }
            Timeframe::D => {
                for p in &mut self.plans {
                    if !matches!(p.state, PlanState::Active | PlanState::Done)
                        && ((p.side == Side::Long && b.close < p.poi.low)
                            || (p.side == Side::Short && b.close > p.poi.high))
                    {
                        p.state = PlanState::Done;
                    }
                }
                let Some(w) = self.trend(Timeframe::W) else {
                    self.cancel_plan_signals();
                    return Ok(());
                };
                if !self.ready(Timeframe::W, c) || !self.ready(Timeframe::D, c) {
                    return Ok(());
                }
                if let Some(weekly) = self.frames.get_mut(&Timeframe::W) {
                    let n = c.parameters.weekly_fractal_sweep_lookback_bars.value as usize;
                    let oldest = weekly
                        .bars
                        .get(weekly.bars.len().saturating_sub(n))
                        .map(|b| b.end)
                        .unwrap_or(b.end);
                    for f in &mut weekly.fractals {
                        if f.confirmed_at >= oldest
                            && !f.swept
                            && ((w == Side::Long && f.upper && b.high > f.price)
                                || (w == Side::Short && !f.upper && b.low < f.price))
                        {
                            self.weekly_fractal_sweep = Some(b.end);
                            f.swept = true;
                        }
                    }
                }
                if let Some(z) = self
                    .frames
                    .get(&Timeframe::W)
                    .and_then(|f| f.latest_zone(w, b.close))
                {
                    if b.low <= z.high && b.high >= z.low {
                        self.weekly_zone_touch = Some(b.end);
                        self.add_plan("W_point_A", w, Timeframe::W, z, b.end);
                    }
                }
                let d = self.trend(Timeframe::D);
                if delta.previous == Some(w) && d != Some(w) {
                    if let Some(z) = delta.inverted.iter().rev().find(|z| z.side == w) {
                        self.add_plan(
                            "D_correction_start",
                            opposite(w),
                            Timeframe::D,
                            z.clone(),
                            b.end,
                        );
                    }
                }
                if delta.previous != Some(w) && d == Some(w) {
                    let z = delta
                        .new
                        .iter()
                        .rev()
                        .find(|z| z.side == w)
                        .cloned()
                        .or_else(|| delta.inverted.iter().rev().find(|z| z.side != w).cloned());
                    if let Some(z) = z {
                        self.add_plan("D_correction_end", w, Timeframe::D, z, b.end);
                    }
                }
                let age = i64::try_from(c.parameters.plan_days.value)
                    .ok()
                    .and_then(|d| d.checked_mul(86400))
                    .ok_or("plan duration overflow")?;
                let recent = |t: Option<i64>| t.is_some_and(|t| b.end >= t && b.end - t <= age);
                if delta.previous != Some(w) && recent(self.weekly_zone_touch) {
                    if let Some(z) = delta.new.iter().rev().find(|z| z.side == w) {
                        self.add_plan(
                            "D_continuation_from_weekly_zone",
                            w,
                            Timeframe::D,
                            z.clone(),
                            b.end,
                        );
                    }
                }
                if recent(self.weekly_fractal_sweep) {
                    if let Some(z) = delta.new.iter().rev().find(|z| z.side != w) {
                        self.add_plan(
                            "D_correction_after_weekly_fractal",
                            opposite(w),
                            Timeframe::D,
                            z.clone(),
                            b.end,
                        );
                    }
                }
            }
            Timeframe::H4 => {
                let current_atr = self
                    .frames
                    .get(&tf)
                    .and_then(|f| atr(&f.bars, c.parameters.atr_period.value as usize));
                for p in &mut self.plans {
                    if p.state != PlanState::Setup || !p.entry_is_ifvg {
                        continue;
                    }
                    let Some(z) = &p.entry_zone else { continue };
                    let reference = if c.parameters.atr_reference_zone.value == "formation_bar" {
                        p.entry_atr
                    } else {
                        current_atr
                    };
                    let Some(a) = reference else { continue };
                    let distance = if p.side == Side::Long {
                        i128::from(z.low) - i128::from(b.close)
                    } else {
                        i128::from(b.close) - i128::from(z.high)
                    };
                    if distance >= 0 && distance as f64 >= a * c.parameters.inv_margin_atr.value {
                        p.state = PlanState::Done;
                    }
                }
                self.cancel_plan_signals();
                if !self.ready(Timeframe::H4, c) {
                    return Ok(());
                }
                let frame = self.frames.get(&tf).expect("updated frame");
                let n = c.parameters.confirmation_inversion_h4_bars.value as usize;
                let oldest = frame
                    .bars
                    .get(frame.bars.len().saturating_sub(n))
                    .map(|b| b.end)
                    .unwrap_or(b.end);
                for p in &mut self.plans {
                    if p.state == PlanState::WaitConfirm
                        && (delta.new.iter().any(|z| z.side == p.side)
                            || delta
                                .inverted
                                .iter()
                                .any(|z| z.side != p.side && z.formed_at >= oldest))
                    {
                        p.state = PlanState::WaitEntry;
                    }
                    if p.state != PlanState::WaitEntry {
                        continue;
                    }
                    let start = if p.rule == "D_correction_start" {
                        crate::calendar::entry_window(
                            p.started,
                            &c.parameters.correction_entry_zones_from.value,
                        )?
                    } else {
                        p.started
                    };
                    let directional = frame
                        .zones
                        .iter()
                        .filter(|z| {
                            z.zone.side == p.side && alive(&z.zone) && z.zone.formed_at >= start
                        })
                        .max_by_key(|z| zone_rank(&z.zone, b.close))
                        .map(|z| z.zone.clone());
                    let oldest_entry = crate::calendar::entry_window(
                        p.started,
                        &c.parameters.ifvg_max_zone_age.value,
                    )?;
                    let inverse = delta
                        .inverted
                        .iter()
                        .filter(|z| {
                            c.parameters.ifvg_entry.value
                                && z.side != p.side
                                && z.formed_at >= oldest_entry
                        })
                        .max_by_key(|z| zone_rank(z, b.close))
                        .cloned();
                    let chosen = if directional.as_ref().is_some_and(|z| z.formed_at == b.end) {
                        directional
                    } else {
                        inverse.or(directional)
                    };
                    if let Some(z) = chosen {
                        p.entry_is_ifvg = z.side != p.side;
                        p.entry_atr = frame
                            .zones
                            .iter()
                            .find(|w| {
                                w.zone.formed_at == z.formed_at
                                    && w.zone.low == z.low
                                    && w.zone.high == z.high
                                    && w.zone.kind == z.kind
                            })
                            .map(|w| w.atr);
                        p.entry_close = Some(b.close);
                        p.entry_zone = Some(z);
                        p.state = PlanState::Ready;
                    }
                }
            }
        }
        self.cancel_plan_signals();
        Ok(())
    }
    fn target_on(&self, side: Side, tf: Timeframe, entry: i64, c: &Strategy) -> Option<i64> {
        let frame = self.frames.get(&tf)?;
        let n = if tf == Timeframe::W {
            c.parameters.target_fractal_lookback_w_bars.value
        } else {
            c.parameters.target_fractal_lookback_d_bars.value
        } as usize;
        let oldest = frame.bars.get(frame.bars.len().saturating_sub(n))?.end;
        let mut values: Vec<i64> = frame
            .fractals
            .iter()
            .filter(|f| !f.swept && f.confirmed_at >= oldest && f.upper == (side == Side::Long))
            .map(|f| f.price)
            .collect();
        values.extend(frame.zones.iter().filter(|z| alive(&z.zone)).map(|z| {
            if side == Side::Long {
                z.zone.low
            } else {
                z.zone.high
            }
        }));
        values
            .into_iter()
            .filter(|v| {
                if side == Side::Long {
                    *v > entry
                } else {
                    *v < entry
                }
            })
            .min_by_key(|v| (i128::from(*v) - i128::from(entry)).abs())
    }
    pub fn target(
        &self,
        side: Side,
        tf: Timeframe,
        entry: i64,
        stop: i64,
        c: &Strategy,
    ) -> Option<i64> {
        let primary = self.target_on(side, tf, entry, c)?;
        if tf == Timeframe::W && c.policies.targets.weekly_poi_daily_alternative.enabled {
            if let Some(daily) = self.target_on(side, Timeframe::D, entry, c) {
                let r = (i128::from(entry) - i128::from(stop)).abs();
                let d = (i128::from(daily) - i128::from(entry)).abs();
                let w = (i128::from(primary) - i128::from(entry)).abs();
                if r > 0 && d < w && d as f64 >= r as f64 * c.parameters.rr_min.value {
                    return Some(daily);
                }
            }
        }
        Some(primary)
    }
    pub fn quote(&mut self, q: Quote, tick: i64, c: &Strategy) -> Result<(), String> {
        if !q.valid() {
            return Err("invalid quote".into());
        }
        self.expire(q.time, c)?;
        for s in &mut self.signals {
            s.observe(q);
        }
        for i in 0..self.plans.len() {
            if let Some(id) = &self.plans[i].signal {
                if let Some(s) = self.signals.iter().find(|s| &s.id == id) {
                    self.plans[i].state = match s.state {
                        SignalState::Pending => PlanState::Setup,
                        SignalState::Active => PlanState::Active,
                        _ => PlanState::Done,
                    };
                }
            }
            let p = self.plans[i].clone();
            if p.state != PlanState::Ready || q.time < p.started {
                continue;
            }
            let Some(frame) = self.frames.get(&Timeframe::H4) else {
                continue;
            };
            let Some(a) = atr(&frame.bars, c.parameters.atr_period.value as usize) else {
                continue;
            };
            let Some(z) = &p.entry_zone else { continue };
            let stop = match select_stop(
                p.side,
                z.low,
                z.high,
                &frame.fractals,
                &frame.bars,
                a,
                tick,
                c,
            ) {
                Ok(s) => s,
                Err(_) => {
                    self.plans[i].state = PlanState::Done;
                    continue;
                }
            };
            let entry = q.price;
            let Some(target) = self.target(p.side, p.poi_tf, entry, stop, c) else {
                continue;
            };
            let maximum = if p.kind == "correction" {
                c.parameters.rr_max_correction.value
            } else {
                c.parameters.rr_max_trend.value
            };
            match Signal::with_cap(&p.id, p.side, q, stop, target, tick, c, maximum) {
                Ok(signal) => {
                    if p.entry_is_ifvg && signal.state == SignalState::Pending {
                        let close = p.entry_close.ok_or("missing IFVG close")?;
                        let (lo, hi) = if c.parameters.ifvg_limit_range.value == "inside_zone" {
                            (z.low, z.high)
                        } else if p.side == Side::Long {
                            (z.low, close)
                        } else {
                            (close, z.high)
                        };
                        if signal.entry < lo || signal.entry > hi {
                            self.plans[i].state = PlanState::Done;
                            continue;
                        }
                    }
                    self.plans[i].state = if signal.state == SignalState::Active {
                        PlanState::Active
                    } else {
                        PlanState::Setup
                    };
                    self.plans[i].signal = Some(signal.id.clone());
                    self.signals.push(signal);
                }
                Err(_) => self.plans[i].state = PlanState::Done,
            }
        }
        self.cancel_plan_signals();
        Ok(())
    }
}
