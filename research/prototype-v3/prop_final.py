"""Upscale $50K Basic 2-Step on the FINAL trade stream. One new account bought every month.
P1 +5%, P2 +8%, daily loss 5% (day-start balance), max DD 10% static from stage base, 80% split,
30% best-day rule for passing a phase and for a payout. Payout as soon as funded profit >= 3%
(and >= 14 days since funding / last payout). Risk: `risk` until the stage target, `post` after it
until the 30% rule lets the stage/payout through."""
import json, glob, collections, sys
import pandas as pd
SIZE=50_000.0; FEE=349.0; SPLIT=0.8; PAY_AT=0.03; PAY_DAYS=14
def load(sess='sess'):
    tr=[]
    for s in ['EURUSD','GBPUSD','USDJPY']:
        tr+=[t for t in json.load(open(glob.glob(f'{s}_author_{sess}*_final.json')[0]))['trades'] if t.get('r') is not None]
    for t in tr:
        t['fill']=pd.Timestamp(t.get('fill_t') or t['signal_t']); t['exit']=pd.Timestamp(t['exit_t'])
    return sorted(tr,key=lambda t:t['exit'])
def account(tr,start,risk,post,frisk=None):
    stage=1; bal=SIZE; day=None; day_start=SIZE; daily={}; last=None; funded=None; pays=[]
    tgt={1:0.05,2:0.08,3:PAY_AT}
    def r30():
        tot=sum(daily.values()); return tot>0 and max(daily.values())<0.30*tot
    for t in tr:
        if t['fill']<start: continue
        d=t['exit'].normalize()
        if d!=day: day=d; day_start=bal
        hit=bal>=SIZE*(1+tgt[stage])
        pnl=t['r']*((frisk if (stage==3 and frisk) else (post if hit else risk)))*SIZE
        bal+=pnl; daily[d]=daily.get(d,0)+pnl
        if bal<SIZE*0.90 or bal<day_start*0.95:
            return dict(start=start,funded=funded,pays=pays,died=t['exit'])
        if bal>=SIZE*(1+tgt[stage]) and r30():
            if stage<3:
                stage+=1; bal=SIZE; daily={}
                if stage==3: funded=t['exit']; last=t['exit']
            elif (t['exit']-last).days>=PAY_DAYS:
                pays.append((t['exit'],(bal-SIZE)*SPLIT)); bal=SIZE; daily={}; last=t['exit']
    return dict(start=start,funded=funded,pays=pays,died=None)
