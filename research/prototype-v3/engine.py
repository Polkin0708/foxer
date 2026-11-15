"""Mechanical reading of 'SWING system by HADIUKOV' (PDF, 31 slides).

Author's rules (explicit in the PDF):
  tools: FVG (3-candle gap), fractal (3-candle extremum)
  W, D = analysis, plan, targets; H4 = entry
  step 1: phase = trend / correction / range
  step 2: point A and target on the SAME timeframe
  step 3: confirm = FVG only: (a) nearest opposite FVG ignored (pierced, close
          clearly beyond) or (b) new FVG toward the plan
          strict trend: point A on W, confirm on H4; correction: point A on W, confirm on D
  step 4: entry = H4 FVG only; stop behind nearest H4 fractal;
          RR trend 1.5-3, correction 1.5-2, range 1.5-2; timing 08:00-23:59 (Kyiv)

Mechanical assumptions (NOT in the PDF, chosen before running, listed in the report):
  A1 trend on W = last fractal high > previous AND last fractal low > previous (up); mirror (down)
  A2 range on D = neither up nor down structure on D; boundaries = last two fractal highs/lows
  A3 FVG is alive until a candle of its timeframe closes beyond its far edge
  A4 point A (trend) = most recent live W FVG in trend direction below/above price
  A5 point A (correction) = unswept W fractal reached by price (trend target reached)
  A6 entry = limit at the near edge of the confirming H4 FVG (retest, as on the slide);
     variant 'market' enters at the confirming H4 close
  A7 stop = nearest confirmed H4 fractal beyond entry (within 30 H4 bars) - 0.05 ATR(H4)
  A8 take profit = min(structural target, RR cap); skip if RR to TP < 1.5
  A9 a pending limit dies with the plan, when the target is hit first, or after 10 D bars
  A10 one trade per plan; plan invalid when W closes beyond point A
  A11 session: confirm and fill only 08:00-23:59 server time (UTC+2/+3 = Kyiv)
  A12 execution on M1: if stop and TP both inside one minute -> stop; spread 1 pip on entry
"""
import sys, json, collections
import numpy as np, pandas as pd

PIP = {'EURUSD': 0.0001, 'GBPUSD': 0.0001, 'USDJPY': 0.01, 'DXY': 0.01, 'USDCAD': 0.0001, 'USDCHF': 0.0001, 'USDSEK': 0.001}
# sign of the pair's move relative to a DXY move (DXY up -> EURUSD down)
DXY_SIGN = {'EURUSD': -1, 'GBPUSD': -1, 'USDJPY': 1, 'USDCAD': 1, 'USDCHF': 1, 'USDSEK': 1}
DXY_CONFIRM_DAYS = 10
import os
TWO_ATTEMPTS = os.environ.get('TWO_ATTEMPTS','1') == '1'   # max 2 stops per idea (kind,dir,poi)
BOX_BLOCKS = os.environ.get('BOX_BLOCKS','0') == '1'
TWO_ATTEMPTS = os.environ.get('TWO_ATTEMPTS','0') == '1'
NIGHT_KILLS = os.environ.get('NIGHT_KILLS','0') == '1'   # a night entry signal that is skipped closes the plan (no late re-entry)
RANGE_ON = os.environ.get('RANGE_ON','1') == '1'
TARGET_TP = os.environ.get('TARGET_TP','0') == '1'   # author 2026: hold to the structural target instead of fixed 2R
TARGET_CAP = float(os.environ.get('TARGET_CAP','5'))
COND_D = os.environ.get('COND_D','0') == '1'         # skip the D confirm when no live D zone lies against the plan before the target         # live range box blocks zone entries inside it   # a DXY confirm counts for the pair's plan for this many days
RR_MIN = 1.5
RR_CAP = {'trend': 3.0, 'correction': 2.0, 'range': 2.0}
PLAN_DAYS = 10
RANGE_BARS = 30
IMPULSE_BARS = 12       # H4 bars (2 days) before the stall window
IMPULSE_ATR = 1.5       # impulse travel >= 1.5 ATR(D)
RANGE_MIN_ATR, RANGE_MAX_ATR = 0.4, 1.5
WICK_MIN_PIPS = 10


def load_m1(path):
    if 'synthetic' in path:
        m = pd.read_csv(path, index_col=0, parse_dates=True)
        return m[['o', 'h', 'l', 'c']]
    m = pd.read_csv(path, sep='\t', usecols=[0, 1, 2, 3, 4, 5])
    m.columns = ['d', 't', 'o', 'h', 'l', 'c']
    ts = pd.to_datetime(m.d + ' ' + m.t, format='%Y.%m.%d %H:%M:%S')  # server time = NY+7
    m.index = ts
    return m[['o', 'h', 'l', 'c']]


def bars(m1, rule):
    g = m1.resample(rule, label='left', closed='left')
    b = pd.DataFrame({'o': g.o.first(), 'h': g.h.max(), 'l': g.l.min(), 'c': g.c.last()}).dropna()
    b['end'] = b.index + pd.tseries.frequencies.to_offset(rule)
    return b


class TF:
    """Closed bars of one timeframe with fractals, FVGs and ATR."""

    def __init__(self, name, atr_n=14):
        self.name, self.n = name, atr_n
        self.o, self.h, self.l, self.c, self.t = [], [], [], [], []
        self.fr_hi, self.fr_lo = [], []   # (idx, price, swept)
        self.fvg = []                     # dict(dir, lo, hi, idx, alive)

    def atr(self):
        if len(self.h) < 2:
            return None
        hh = np.array(self.h[-self.n:]); ll = np.array(self.l[-self.n:])
        return float(np.mean(hh - ll))

    def push(self, o, h, l, c, t):
        self.o.append(o); self.h.append(h); self.l.append(l); self.c.append(c); self.t.append(t)
        i = len(self.c) - 1
        # sweeps and zone life
        for f in self.fr_hi:
            if not f[2] and h > f[1]:
                f[2] = True
        for f in self.fr_lo:
            if not f[2] and l < f[1]:
                f[2] = True
        for z in self.fvg:
            if z['alive']:
                if z['dir'] > 0 and c < z['lo']:
                    z['alive'] = False; z['inverted_at'] = i
                if z['dir'] < 0 and c > z['hi']:
                    z['alive'] = False; z['inverted_at'] = i
        # fractal confirmed on bar i for bar i-1
        if i >= 2:
            if self.h[i-1] > self.h[i-2] and self.h[i-1] > self.h[i]:
                self.fr_hi.append([i-1, self.h[i-1], False])
            if self.l[i-1] < self.l[i-2] and self.l[i-1] < self.l[i]:
                self.fr_lo.append([i-1, self.l[i-1], False])
            if self.l[i] > self.h[i-2]:
                self.fvg.append({'dir': 1, 'lo': self.h[i-2], 'hi': self.l[i], 'idx': i, 'alive': True})
            if self.h[i] < self.l[i-2]:
                self.fvg.append({'dir': -1, 'lo': self.h[i], 'hi': self.l[i-2], 'idx': i, 'alive': True})

    def structure(self):
        """A1: +1 up, -1 down, 0 none."""
        if len(self.fr_hi) < 2 or len(self.fr_lo) < 2:
            return 0
        h2, h1 = self.fr_hi[-2][1], self.fr_hi[-1][1]
        l2, l1 = self.fr_lo[-2][1], self.fr_lo[-1][1]
        if h1 > h2 and l1 > l2:
            return 1
        if h1 < h2 and l1 < l2:
            return -1
        return 0

    def nearest_live_fvg(self, d, price):
        zs = [z for z in self.fvg if z['alive'] and z['dir'] == d and (z['hi'] <= price if d > 0 else z['lo'] >= price)]
        return zs[-1] if zs else None

    def nearest_unswept_fractal(self, d, price):
        if d > 0:
            fs = [f for f in self.fr_hi if not f[2] and f[1] > price]
            return min(fs, key=lambda f: f[1]) if fs else None
        fs = [f for f in self.fr_lo if not f[2] and f[1] < price]
        return max(fs, key=lambda f: f[1]) if fs else None

    def nearest_fractal_beyond(self, d, level, lookback=30):
        """A7: stop reference. d=+1 long -> fractal low below level."""
        n = len(self.c)
        if d > 0:
            fs = [f for f in self.fr_lo if f[1] < level and f[0] >= n - lookback]
            return max(fs, key=lambda f: f[1])[1] if fs else None
        fs = [f for f in self.fr_hi if f[1] > level and f[0] >= n - lookback]
        return min(fs, key=lambda f: f[1])[1] if fs else None


def in_session(ts):
    return 8 <= ts.hour <= 23  # A11


def run(symbol, path, entry_mode='retest', session=True, start='2020-09-01', dxy_events=None, dxy_rules=(), record_events=False):
    """dxy_events: list of (time, kind, dxy_dir) with kind in {'confirm','target','sweep'} from a DXY run
    (record_events=True). dxy_rules: subset of {'close','confirm'}."""
    events = []  # recorded when record_events
    dxy_events = sorted(dxy_events or [])
    dxy_i = 0; dxy_recent = []  # events seen so far
    sign = DXY_SIGN.get(symbol, 0)
    pip = PIP[symbol]
    m1 = load_m1(path)
    H4, D = bars(m1, '4h'), bars(m1, '1D')
    # server week starts Monday 00:00 (= Sunday 17:00 New York)
    W = bars(m1, 'W-MON')
    assert all(W.index.weekday == 0) and all((W.end - W.index) == pd.Timedelta(days=7))
    tfs = {'H4': TF('H4'), 'D': TF('D'), 'W': TF('W')}
    closes = sorted([(r.end, 'W', r) for r in W.itertuples()] + [(r.end, 'D', r) for r in D.itertuples()] + [(r.end, 'H4', r) for r in H4.itertuples()],
                    key=lambda x: (x[0], {'W': 0, 'D': 1, 'H4': 2}[x[1]]))
    plans, trades, pending = [], [], []
    box = None; last_impulse = None
    active = []
    ci = 0
    start_ts = pd.Timestamp(start)
    m1v = m1.values; m1i = m1.index
    d_count = 0
    log = collections.Counter()

    idea_stops = collections.Counter()

    def new_plan(kind, d, poi, target, tf_conf, t):
        if any(p['kind'] == kind and p['dir'] == d and p['poi'] == poi and p['state'] != 'done' for p in plans):
            return
        if TWO_ATTEMPTS and idea_stops[(kind, d, poi)] >= 2:
            log['skip_two_attempts'] += 1; return
        if COND_D and tf_conf == 'D':
            px = tfs['D'].c[-1] if tfs['D'].c else None
            against = [z for z in tfs['D'].fvg if z['alive'] and z['dir'] == -d and px is not None and
                       ((d > 0 and px <= z['lo'] < target) or (d < 0 and target < z['hi'] <= px))]
            if not against:
                tf_conf = 'H4'; log['cond_d_h4'] += 1
        plans.append({'kind': kind, 'dir': d, 'poi': poi, 'target': target, 'conf_tf': tf_conf,
                      'state': 'wait_confirm', 'created': t, 'created_day': d_count, 'h4_idx': len(tfs['H4'].c)})
        log[f'plan_{kind}'] += 1

    for k in range(len(m1v)):
        t = m1i[k]
        # 1) process bar closes up to this minute
        while ci < len(closes) and closes[ci][0] <= t:
            end, name, r = closes[ci]; ci += 1
            tf = tfs[name]
            tf.push(r.o, r.h, r.l, r.c, end)
            if t < start_ts:
                continue
            if name == 'D':
                d_count += 1
            price = r.c
            # ---- plans on bar closes
            if name == 'W':
                s = tfs['W'].structure()
                for d in (1, -1):
                    if s != d:
                        continue
                    z = tfs['W'].nearest_live_fvg(d, price)
                    tgt = tfs['W'].nearest_unswept_fractal(d, price)
                    touched = z and ((d > 0 and r.l <= z['hi']) or (d < 0 and r.h >= z['lo']))
                    if z and tgt and touched:
                        new_plan('trend', d, (z['lo'], z['hi']), tgt[1], 'H4', end)
                    # A5 correction: trend target fractal reached this week
                    for f in (tfs['W'].fr_hi if d > 0 else tfs['W'].fr_lo):
                        if f[2] and ((d > 0 and r.h > f[1] >= tfs['W'].h[-2]) or (d < 0 and r.l < f[1] <= tfs['W'].l[-2])):
                            zt = tfs['W'].nearest_live_fvg(d, price)
                            if zt:
                                tg = zt['hi'] if d > 0 else zt['lo']
                                new_plan('correction', -d, (f[1], f[1]), tg, 'D', end)
            if name == 'D':
                sD, sW = tfs['D'].structure(), tfs['W'].structure()
                if sD == 0 and sW == 0 and len(tfs['D'].fr_hi) >= 2 and len(tfs['D'].fr_lo) >= 2:
                    top = max(tfs['D'].fr_hi[-1][1], tfs['D'].fr_hi[-2][1])
                    bot = min(tfs['D'].fr_lo[-1][1], tfs['D'].fr_lo[-2][1])
                    a = tfs['D'].atr()
                    if a and top - bot >= a:
                        if r.h >= top:
                            new_plan('range', -1, (top, top), bot, 'D', end)
                        if r.l <= bot:
                            new_plan('range', 1, (bot, bot), top, 'D', end)
            # ---- H4 range module (author mode only). Box is frozen once found; it is
            # re-estimated (expansion) after each completed range trade and dropped when
            # price closes beyond a border by more than half the box height.
            if RANGE_ON and name == 'H4' and entry_mode == 'author' and len(tfs['H4'].c) > RANGE_BARS + 2:
                h4 = tfs['H4']; aD = tfs['D'].atr(); a4 = h4.atr() or 0
                def estimate():
                    # longest recent stretch (12..60 closed H4 bars, excluding the current one)
                    # that fits inside the allowed height: the impulse before the stall is excluded
                    if not aD:
                        return None
                    best = None
                    for k in range(12, min(60, len(h4.c) - 1) + 1):
                        hs, ls = h4.h[-k-1:-1], h4.l[-k-1:-1]
                        top, bot = max(hs), min(ls)
                        if top - bot > RANGE_MAX_ATR * aD:
                            break
                        if top - bot >= RANGE_MIN_ATR * aD:
                            best = {'top': top, 'bot': bot, 'stops': {1: 0, -1: 0}, 'born': len(h4.c), 'k': k}
                    if best is None:
                        return None
                    # filter 1: an impulse must precede the stall -- the IMPULSE_BARS H4 bars before the
                    # window must travel >= IMPULSE_ATR * ATR(D) and end inside the box
                    k = best['k']; pre_h = h4.h[-k-1-IMPULSE_BARS:-k-1]; pre_l = h4.l[-k-1-IMPULSE_BARS:-k-1]
                    if len(pre_h) < IMPULSE_BARS:
                        return None
                    travel = max(pre_h) - min(pre_l)
                    if travel < IMPULSE_ATR * aD or travel < 1.2 * (best['top'] - best['bot']):
                        log['skip_no_impulse'] += 1; return None
                    # filter 2: one box per impulse -- the impulse must be newer than the last used one
                    imp_id = len(h4.c) - k - IMPULSE_BARS
                    if last_impulse is not None and imp_id <= last_impulse + IMPULSE_BARS:
                        log['skip_same_impulse'] += 1; return None
                    best['impulse'] = imp_id
                    return best
                if box is None:
                    box = estimate()
                    if box is not None:
                        last_impulse = box['impulse']; log['box_set'] += 1
                else:
                    height = box['top'] - box['bot']
                    if r.c > box['top'] + 0.5 * height or r.c < box['bot'] - 0.5 * height:
                        box = None; log['box_broken'] += 1
                if box is not None and not any(tr['state'] in ('pending', 'active') and tr['kind'] == 'range' for tr in trades):
                    top, bot = box['top'], box['bot']
                    for d, border, extreme in ((-1, top, r.h), (1, bot, r.l)):
                        dev = extreme > border if d < 0 else extreme < border
                        inside = bot <= r.c <= top
                        prev_out = (h4.c[-2] > top) if d < 0 else (h4.c[-2] < bot)
                        wick = abs(extreme - border) >= WICK_MIN_PIPS * pip
                        if not inside or box['stops'][d] >= 2:
                            continue
                        if not ((dev and wick) or prev_out):
                            continue
                        if session and not in_session(end - pd.Timedelta(minutes=1)):
                            log['skip_session'] += 1; continue
                        dev_ext = max(extreme, h4.h[-2]) if d < 0 else min(extreme, h4.l[-2])
                        stop = dev_ext + 0.05 * a4 if d < 0 else dev_ext - 0.05 * a4
                        entry = r.c; risk = (entry - stop) * d
                        tgt = bot if d < 0 else top
                        if risk <= 0:
                            continue
                        rr = (tgt - entry) * d / risk
                        limit = False
                        if rr < 2.0:
                            entry = (tgt + 2.0 * stop) / 3.0; risk = (entry - stop) * d; limit = True
                            if risk <= 0 or not (bot <= entry <= top):
                                log['skip_rr'] += 1; continue
                        tp = entry + 2.0 * risk * d if abs(tgt - entry) > 2.0 * risk else tgt  # fixed 2R cap
                        tr = {'symbol': symbol, 'kind': 'range', 'dir': d, 'entry': entry, 'stop': stop, 'tp': tp,
                              'rr': 2.0 if limit else rr, 'signal_t': end, 'state': 'pending' if limit else 'active',
                              'plan': {'state': 'active', 'kind': 'range', 'box': box, 'dir': d}}
                        if not limit:
                            tr['entry'] = r.c + (pip if d > 0 else -pip); tr['fill_t'] = end; active.append(tr)
                        else:
                            pending.append(tr)
                        trades.append(tr); log['range_entry'] += 1
                        if record_events:
                            events.append((end, 'sweep', d))
                        break
            # ---- plan lifecycle
            for p in plans:
                if p['state'] == 'done' or p.get('kind') == 'range':
                    continue
                if d_count - p['created_day'] > PLAN_DAYS and p['state'] != 'active':
                    p['state'] = 'done'; log['expired'] += 1; continue
                lo, hi = p['poi']
                if name == 'W' and p['kind'] == 'trend' and ((p['dir'] > 0 and r.c < lo) or (p['dir'] < 0 and r.c > hi)):
                    p['state'] = 'done'; log['invalid'] += 1; continue
                if name == 'D' and p['kind'] != 'trend' and ((p['dir'] > 0 and r.c < lo) or (p['dir'] < 0 and r.c > hi)):
                    p['state'] = 'done'; log['invalid'] += 1; continue
                # confirm (step 3) on conf_tf
                if p['state'] == 'wait_confirm' and name == p['conf_tf']:
                    tfc = tfs[name]; i = len(tfc.c) - 1; d = p['dir']
                    new_fvg = any(z['dir'] == d and z['idx'] == i for z in tfc.fvg)
                    inverted = any(z['dir'] == -d and z.get('inverted_at') == i for z in tfc.fvg)
                    if p['state'] == 'wait_confirm' and 'confirm' in dxy_rules and sign and p['conf_tf'] == 'D':
                        # DXY confirm in the matching direction within DXY_CONFIRM_DAYS stands in for the D confirm
                        want = p['dir'] * sign
                        if any(k == 'confirm' and dd == want and (end - tt) <= pd.Timedelta(days=DXY_CONFIRM_DAYS) for tt, k, dd in dxy_recent):
                            new_fvg = True; log['dxy_confirmed'] += 1
                    if new_fvg or inverted:
                        p['state'] = 'wait_entry'; p['h4_idx'] = len(tfs['H4'].c) - (1 if name == 'H4' else 0)
                        log['confirmed'] += 1
                        if record_events:
                            events.append((end, 'confirm', p['dir']))
                # entry (step 4): H4 FVG toward plan, formed after confirm
                if p['state'] == 'wait_entry' and name == 'H4':
                    h4 = tfs['H4']; i = len(h4.c) - 1; d = p['dir']
                    z = next((z for z in reversed(h4.fvg) if z['dir'] == d and z['idx'] == i and z['idx'] >= p['h4_idx']), None)
                    if not z:
                        continue
                    if session and not in_session(end - pd.Timedelta(minutes=1)):
                        log['skip_session'] += 1
                        if NIGHT_KILLS:
                            p['state'] = 'done'
                        continue
                    if BOX_BLOCKS and box is not None and box['bot'] <= r.c <= box['top']:
                        log['skip_in_box'] += 1; continue
                    entry = (z['hi'] if d > 0 else z['lo']) if entry_mode == 'retest' else r.c
                    a4 = h4.atr() or 0
                    if entry_mode == 'author':
                        entry = r.c
                    ref = h4.nearest_fractal_beyond(d, min(entry, z['lo']) if d > 0 else max(entry, z['hi']))
                    if ref is None:
                        ref = z['lo'] if d > 0 else z['hi']
                    stop = ref - 0.05 * a4 if d > 0 else ref + 0.05 * a4
                    risk = (entry - stop) * d
                    if risk <= 0:
                        log['bad_stop'] += 1; continue
                    tgt = p['target']
                    if entry_mode == 'author':
                        # fixed 2R; if structural target is nearer than 2R, move the entry (limit) so that RR to target == 2
                        rr_t = (tgt - entry) * d / risk
                        if rr_t >= 2.0:
                            if TARGET_TP:
                                tp = min(tgt, entry + TARGET_CAP * risk * d) if d > 0 else max(tgt, entry - TARGET_CAP * risk * d)
                                rr = (tp - entry) * d / risk
                            else:
                                tp = entry + 2.0 * risk * d; rr = 2.0
                            limit = False
                        else:
                            entry = (tgt + 2.0 * stop) / 3.0
                            risk = (entry - stop) * d
                            if risk <= 0 or ((d > 0 and entry > r.c) or (d < 0 and entry < r.c)):
                                log['skip_rr'] += 1; continue
                            tp = tgt; rr = 2.0; limit = True
                    else:
                        tp = min(tgt, entry + RR_CAP[p['kind']] * risk) if d > 0 else max(tgt, entry - RR_CAP[p['kind']] * risk)
                        rr = (tp - entry) * d / risk
                        limit = entry_mode == 'retest'
                    if rr < RR_MIN:
                        log['skip_rr'] += 1; continue
                    tr = {'symbol': symbol, 'kind': p['kind'], 'dir': d, 'entry': entry, 'stop': stop, 'tp': tp, 'rr': rr,
                          'signal_t': end, 'state': 'pending' if limit else 'active', 'plan': p}
                    if not limit:
                        tr['entry'] = r.c + (pip if d > 0 else -pip); tr['fill_t'] = end
                        active.append(tr)
                    else:
                        pending.append(tr)
                    p['state'] = 'active'; trades.append(tr)
        if t < start_ts:
            continue
        o, h, l, c = m1v[k]
        while dxy_i < len(dxy_events) and dxy_events[dxy_i][0] <= t:
            dxy_recent.append(dxy_events[dxy_i]); dxy_i += 1
        if record_events:
            for p in plans:
                if p['state'] != 'done' and not p.get('target_hit') and p.get('kind') != 'range':
                    if (p['dir'] > 0 and h >= p['target']) or (p['dir'] < 0 and l <= p['target']):
                        p['target_hit'] = t; events.append((t, 'target', p['dir']))
        if 'close' in dxy_rules and sign and active and dxy_recent:
            # DXY reached its target (or swept its range border) in the direction that favours the trade
            for tr in active[:]:
                d = tr['dir']; want = d * sign
                ft = tr.get('fill_t', tr['signal_t'])
                if any(k in ('target', 'sweep') and dd == want and ft < tt <= t for tt, k, dd in dxy_recent[-50:]):
                    risk = (tr['entry'] - tr['stop']) * d
                    tr['state'] = 'dxy_close'; tr['r'] = (c - tr['entry']) * d / risk; tr['exit_t'] = t
                    active.remove(tr); tr['plan']['state'] = 'done'; log['dxy_close'] += 1
                    if tr['kind'] == 'range':
                        box = None
        # 2) pending fills / cancels (A9, A12)
        for tr in pending[:]:
            d = tr['dir']
            if tr['plan']['state'] == 'done' or (d > 0 and h >= tr['tp']) or (d < 0 and l <= tr['tp']):
                tr['state'] = 'cancelled'; pending.remove(tr); tr['plan']['state'] = 'done'; continue
            hit = (d > 0 and l <= tr['entry']) or (d < 0 and h >= tr['entry'])
            if hit and (not session or in_session(t)):
                tr['state'] = 'active'; tr['fill_t'] = t
                tr['entry'] = tr['entry'] + (pip if d > 0 else -pip)
                pending.remove(tr); active.append(tr)
                # stop hit in the same minute -> stop
                if (d > 0 and l <= tr['stop']) or (d < 0 and h >= tr['stop']):
                    tr['state'] = 'sl'; tr['exit_t'] = t; tr['r'] = -1.0; active.remove(tr); tr['plan']['state'] = 'done'
        # 3) active trades
        for tr in active[:]:
            d = tr['dir']; risk = (tr['entry'] - tr['stop']) * d
            if (d > 0 and l <= tr['stop']) or (d < 0 and h >= tr['stop']):
                tr['state'] = 'sl'; tr['r'] = -1.0
                if tr['kind'] == 'range':
                    tr['plan']['box']['stops'][d] += 1
                else:
                    idea_stops[(tr['kind'], d, tr['plan']['poi'])] += 1
            elif (d > 0 and h >= tr['tp']) or (d < 0 and l <= tr['tp']):
                tr['state'] = 'tp'; tr['r'] = (tr['tp'] - tr['entry']) * d / risk
            else:
                continue
            tr['exit_t'] = t; active.remove(tr); tr['plan']['state'] = 'done'
            if tr['kind'] == 'range':
                box = None  # re-estimated on the next H4 close (expansion rule)
    out = [{k: (str(v) if isinstance(v, pd.Timestamp) else v) for k, v in tr.items() if k != 'plan'} for tr in trades]
    if record_events:
        return out, dict(log), events
    return out, dict(log)


if __name__ == '__main__':
    symbol, path, mode, sess = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] == '1'
    rules = tuple(sys.argv[5].split(',')) if len(sys.argv) > 5 and sys.argv[5] else ()
    tag = (('_dxy-' + '-'.join(rules)) if rules else '') + os.environ.get('OUT_TAG', '')
    if symbol == 'DXY':
        trades, log, events = run(symbol, path, mode, sess, record_events=True)
        json.dump([(str(t), k, d) for t, k, d in events], open('/home/claude/runs/hadiukov/DXY_events.json', 'w'))
        print('events', collections.Counter(k for _, k, _ in events))
    else:
        ev = [(pd.Timestamp(t), k, d) for t, k, d in json.load(open('/home/claude/runs/hadiukov/DXY_events.json'))] if rules else None
        trades, log = run(symbol, path, mode, sess, dxy_events=ev, dxy_rules=rules)
    json.dump({'trades': trades, 'log': log}, open(f'/home/claude/runs/hadiukov/{symbol}_{mode}_{"sess" if sess else "all"}{tag}.json', 'w'), default=str)
    c = [t for t in trades if t.get('r') is not None]
    print(symbol, mode, 'sess' if sess else 'all', 'n', len(c), 'net %+.1f' % sum(t['r'] for t in c), log)
