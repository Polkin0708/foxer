#!/usr/bin/env python3
"""Download HistData ASCII bid/ask quotes and aggregate independent UTC M1 bars."""
import argparse
import csv
from datetime import datetime, timedelta, timezone
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parents[1] / 'data/histdata-eurusd-2024-2026'
BASE = 'https://www.histdata.com'


def curl(args):
    result = subprocess.run(['curl', '-f', '-sS', '-L', '--max-time', '180',
                             '--retry', '2', '--retry-delay', '3', *args], capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors='replace'))
    return result.stdout


def download(root, year, month, symbol="EURUSD"):
    if not re.fullmatch(r"[A-Z]{6}", symbol): raise ValueError("Invalid symbol")
    path = root / 'raw' / f'{symbol}-T-{year}{month:02}.zip'
    page_url = f'{BASE}/download-free-forex-historical-data/?/ascii/tick-data-quotes/{symbol.lower()}/{year}/{month}'
    if not path.exists():
        html = curl([page_url]).decode()
        form = re.search(r'<form id="file_down".*?</form>', html, re.S)
        if not form:
            raise ValueError(f'No download form: {page_url}')
        fields = dict(re.findall(r'name="([^"]+)"[^>]*value="([^"]*)"', form[0]))
        expected = dict(date=str(year), datemonth=f'{year}{month:02}', platform='ASCII', timeframe='T', fxpair=symbol)
        if any(fields.get(k) != v for k, v in expected.items()) or 'tk' not in fields:
            raise ValueError(f'Unexpected form: {page_url}')
        from urllib.parse import urlencode
        temp = path.with_suffix('.part')
        curl(['-e', page_url, '--data', urlencode(fields), f'{BASE}/get.php', '-o', str(temp)])
        with zipfile.ZipFile(temp) as archive:
            if archive.testzip() is not None:
                raise ValueError(f'ZIP checksum error: {temp}')
        temp.replace(path)
    with zipfile.ZipFile(path) as archive:
        expected = f'DAT_ASCII_{symbol}_T_{year}{month:02}.csv'
        if expected not in archive.namelist():
            raise ValueError(f'Missing expected member: {expected}')
    return dict(month=f'{year}-{month:02}', file=str(path.relative_to(root)),
                page_url=page_url, bytes=path.stat().st_size,
                sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def price(text):
    whole, dot, frac = text.partition('.')
    if len(frac) > 6 or not whole.isdigit() or (frac and not frac.isdigit()):
        raise ValueError(f'Invalid quote: {text}')
    result = int(whole)*1000000 + int(frac.ljust(6, '0') or '0')
    if result <= 0:
        raise ValueError('Nonpositive price')
    return result


def iso(dt):
    return dt.isoformat().replace('+00:00', 'Z')


def prepare(root, manifest):
    symbol = manifest.get('symbol', 'EURUSD')
    if not re.fullmatch(r'[A-Z]{6}', symbol): raise ValueError('Invalid symbol')
    pip_units = 10000 if symbol.endswith('JPY') else 100
    out = root / 'prepared'
    out.mkdir(exist_ok=True)
    report = dict(symbol=symbol, pip_size='0.01' if symbol.endswith('JPY') else '0.0001', source='HistData Generic ASCII Tick', timezone_conversion='fixed EST UTC-05:00 to UTC; no DST',
                  quote_rows=0, exact_consecutive_duplicates=0, crossed_quotes=0,
                  years={}, gaps=0, missing_months=manifest['missing_months'], volume_available=False,
                  timestamp_reversals=[], quarantined_quotes=0)
    header = ['timestamp_utc', 'timestamp_ms', 'bar_end_utc']
    header += [f'{side}_{field}' for side in ('bid', 'ask') for field in ('open','high','low','close')]
    header += ['quote_count','spread_open_pips','spread_close_pips','spread_min_pips',
               'spread_max_pips','spread_mean_pips','gap_before_minutes']
    handles, writers = {}, {}
    previous_bar = previous_stamp = previous_quote = None
    active_key = None
    bar = None
    gaps_file = (out/'gaps.csv').open('w')
    gaps = csv.writer(gaps_file)
    gaps.writerow(['previous_bar_utc','next_bar_utc','missing_minutes'])

    def emit(key, values):
        nonlocal previous_bar
        # HistData timestamps use fixed EST, explicitly not America/New_York.
        dt = datetime.strptime(key, '%Y%m%d %H%M').replace(tzinfo=timezone.utc) + timedelta(hours=5)
        year = str(dt.year)
        if year not in writers:
            handles[year] = (out/f'{symbol}-M1-bid-ask-{year}.csv').open('w')
            writers[year] = csv.writer(handles[year])
            writers[year].writerow(header)
            report['years'][year] = dict(rows=0, first=iso(dt), last=None)
        gap = int((dt-previous_bar).total_seconds()/60)-1 if previous_bar else 0
        if gap < 0:
            raise ValueError('Overlapping or unordered minutes')
        if gap:
            gaps.writerow([iso(previous_bar),iso(dt),gap])
            report['gaps'] += 1
        b,a,n,so,sc,smin,smax,ssum = values
        row = [iso(dt),int(dt.timestamp()*1000),iso(dt+timedelta(minutes=1))]
        row += [f'{p/1000000:.6f}' for p in b+a]
        row += [n] + [f'{s/pip_units:.4f}' for s in (so,sc,smin,smax,ssum/n)] + [gap]
        writers[year].writerow(row)
        report['years'][year]['rows'] += 1
        report['years'][year]['last'] = iso(dt)
        previous_bar = dt

    try:
        for item in manifest['files']:
            path = root/item['file']
            if hashlib.sha256(path.read_bytes()).hexdigest() != item['sha256']:
                raise ValueError(f'Hash mismatch: {path}')
            with zipfile.ZipFile(path) as archive:
                name = next(n for n in archive.namelist() if n.endswith('.csv'))
                # Do not sort an ambiguous repeated clock hour into fabricated OHLC.
                # Quarantine the full overlapping minute range from both occurrences.
                ranges = []
                prev = None
                with archive.open(name) as scan:
                    for raw in scan:
                        stamp_bytes = raw.split(b',',1)[0]
                        if prev is not None and stamp_bytes < prev:
                            lo,hi = stamp_bytes[:13].decode(),prev[:13].decode()
                            ranges.append((lo,hi))
                            report['timestamp_reversals'].append(dict(month=item['month'],
                                previous_local=prev.decode(),next_local=stamp_bytes.decode(),
                                excluded_from_local_minute=lo,excluded_through_local_minute=hi))
                        prev = stamp_bytes
                with archive.open(name) as stream:
                    for line in io.TextIOWrapper(stream, encoding='ascii'):
                        stamp,bid,ask,volume = line.strip().split(',')
                        if not re.fullmatch(r'\d{8} \d{9}', stamp):
                            raise ValueError(f'Invalid timestamp: {stamp}')
                        if any(lo <= stamp[:13] <= hi for lo,hi in ranges):
                            report['quarantined_quotes'] += 1
                            continue
                        if previous_stamp is not None and stamp < previous_stamp:
                            raise ValueError(f'Unordered quotes: {stamp}')
                        b,a = price(bid),price(ask)
                        quote = (stamp,b,a)
                        if quote == previous_quote:
                            report['exact_consecutive_duplicates'] += 1
                            continue
                        previous_stamp, previous_quote = stamp,quote
                        report['quote_rows'] += 1
                        spread = a-b
                        report['crossed_quotes'] += int(spread < 0)
                        key = stamp[:13]
                        if key != active_key:
                            if bar is not None:
                                emit(active_key,bar)
                            active_key = key
                            bar = [[b,b,b,b],[a,a,a,a],1,spread,spread,spread,spread,spread]
                        else:
                            for values,p in ((bar[0],b),(bar[1],a)):
                                values[1],values[2],values[3] = max(values[1],p),min(values[2],p),p
                            bar[2] += 1
                            bar[4] = spread
                            bar[5],bar[6],bar[7] = min(bar[5],spread),max(bar[6],spread),bar[7]+spread
            print(f'Aggregated {item["month"]}: {report["quote_rows"]:,} quotes', flush=True)
        if bar is not None:
            emit(active_key,bar)
    finally:
        gaps_file.close()
        for handle in handles.values():
            handle.close()
    report['outputs'] = {p.name:dict(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                         for p in out.glob('*.csv')}
    (root/'quality-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('outputs','timestamp_reversals')},indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--symbol', default='EURUSD')
    parser.add_argument('--start-year', type=int, default=2024)
    parser.add_argument('--end-year', type=int, default=2026)
    parser.add_argument('--end-month', type=int, default=10)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--year', type=int, help='Download one complete calendar year into a separate --root')
    args = parser.parse_args()
    root = args.root.resolve()
    (root/'raw').mkdir(parents=True,exist_ok=True)
    path = root/'manifest.json'
    if args.prepare_only:
        manifest = json.loads(path.read_text())
    else:
        manifest = dict(symbol=args.symbol, source='HistData', downloaded_at=iso(datetime.now(timezone.utc)),
                        requested_months=f'{args.year}-01 through {args.year}-12' if args.year else f'{args.start_year}-01 through {args.end_year}-{args.end_month:02}', files=[],missing_months=[])
        for year in ((args.year,) if args.year else range(args.start_year,args.end_year+1)):
            for month in range(1,args.end_month+1 if year==args.end_year and not args.year else 13):
                try:
                    manifest['files'].append(download(root,year,month,args.symbol))
                    print(f'Downloaded {year}-{month:02}',flush=True)
                except Exception as exc:
                    manifest['missing_months'].append(dict(month=f'{year}-{month:02}',error=str(exc)))
                    print(f'FAILED {year}-{month:02}: {exc}',flush=True)
                path.write_text(json.dumps(manifest,indent=2)+'\n')
                time.sleep(1)
    prepare(root,manifest)
    if manifest['missing_months']:
        raise SystemExit('Missing months; see manifest.json')


if __name__ == '__main__':
    main()
