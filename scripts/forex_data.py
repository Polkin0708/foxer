#!/usr/bin/env python3
"""Download and audit Dukascopy EURUSD M1 archives (Python standard library)."""
import argparse
import concurrent.futures
import csv
from datetime import date, datetime, timedelta, timezone
import hashlib
import json
import lzma
import math
from pathlib import Path
import struct
import subprocess
import time

UTC = timezone.utc
FIELDS = ['open', 'high', 'low', 'close', 'volume']


def decode(payload):
    data = lzma.decompress(payload) if payload else b''
    if len(data) % 24:
        raise ValueError('Invalid candle record length')
    rows = {}
    previous = -1
    for sec, op, cl, lo, hi, vol in struct.iter_unpack('>5If', data):
        if not (0 <= sec < 86400 and sec % 60 == 0 and sec > previous):
            raise ValueError('Duplicate, unordered or invalid minute')
        if not (0 < lo <= min(op, cl) <= max(op, cl) <= hi):
            raise ValueError('Invalid OHLC')
        if not math.isfinite(vol) or vol < 0:
            raise ValueError('Invalid volume')
        rows[sec] = (op, hi, lo, cl, vol)
        previous = sec
    return rows


def fetch(job, root, base, offline=False):
    day, side = job
    rel = f'EURUSD/{day.year}/{day.month-1:02d}/{day.day:02d}/{side}_candles_min_1.bi5'
    url = f'{base.rstrip("/")}/{rel}'
    path = root / 'raw' / str(day.year) / f'{day}-{side}.bi5'
    meta = dict(date=str(day), side=side, url=url, path=str(path))
    if path.exists():
        payload = path.read_bytes()
        rows = decode(payload)
        return dict(meta, status='ok', rows=len(rows), bytes=len(payload),
                    reused_cached_file=True,
                    sha256=hashlib.sha256(payload).hexdigest())
    if offline:
        return dict(meta, status='not_downloaded')
    for attempt in range(4):
        try:
            # System curl uses the OS certificate store, including on macOS.
            result = subprocess.run(['curl', '-sS', '-L', '--max-time', '30',
                                     '-w', '\n%{http_code}', url], capture_output=True)
            if result.returncode:
                raise OSError(result.stderr.decode(errors='replace'))
            payload, status = result.stdout.rsplit(b'\n', 1)
            if status == b'404':
                return dict(meta, status='not_found', error='HTTP 404')
            if status == b'429':
                return dict(meta, status='rate_limited', error='HTTP 429')
            if status != b'200':
                raise OSError(f'HTTP {status.decode()}')
            rows = decode(payload)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(payload)
            return dict(meta, status='ok', rows=len(rows), bytes=len(payload),
                        sha256=hashlib.sha256(payload).hexdigest())
        except Exception as exc:
            error = str(exc)
        time.sleep(1 + attempt)
    return dict(meta, status='error', error=error)


def iso(ts):
    return datetime.fromtimestamp(ts, UTC).isoformat().replace('+00:00', 'Z')


def prepare(root, manifest):
    out = root / 'prepared'
    out.mkdir(exist_ok=True)
    by_day = {}
    for item in manifest['files']:
        by_day.setdefault(item['date'], {})[item['side']] = item
    report = dict(requested_start=manifest['start'], requested_end_exclusive=manifest['end'],
                  years={}, files_by_status={}, unmatched_minutes=0, zero_volume_pairs=0,
                  crossed_open_or_close=0, missing_or_empty_days=[], gap_count=0,
                  no_forward_fill=True, duplicate_policy='reject', price_scale=100000)
    for item in manifest['files']:
        status = item['status']
        report['files_by_status'][status] = report['files_by_status'].get(status, 0) + 1
    header = ['timestamp_utc', 'timestamp_ms', 'bar_end_utc']
    header += [f'{s}_{f}' for s in ('bid', 'ask') for f in FIELDS]
    header += ['open_side_difference_pips', 'close_side_difference_pips', 'gap_before_minutes']
    previous = None
    handles = {}
    writers = {}
    with (out / 'gaps.csv').open('w') as gaps_file:
        gaps = csv.writer(gaps_file)
        gaps.writerow(['previous_bar_utc', 'next_bar_utc', 'missing_minutes'])
        try:
            for day_str, sides in sorted(by_day.items()):
                day = date.fromisoformat(day_str)
                parsed = {}
                for side in ('BID', 'ASK'):
                    item = sides[side]
                    if item['status'] == 'ok':
                        payload = Path(item['path']).read_bytes()
                        if hashlib.sha256(payload).hexdigest() != item['sha256']:
                            raise ValueError(f'Hash mismatch: {item["path"]}')
                        parsed[side] = decode(payload)
                    else:
                        parsed[side] = {}
                bid, ask = parsed['BID'], parsed['ASK']
                if not bid or not ask:
                    report['missing_or_empty_days'].append(dict(date=day_str,
                        weekday=day.strftime('%A'), bid_rows=len(bid), ask_rows=len(ask)))
                report['unmatched_minutes'] += len(bid.keys() ^ ask.keys())
                year = str(day.year)
                if year not in handles:
                    handles[year] = (out / f'EURUSD-M1-bid-ask-{year}.csv').open('w')
                    writers[year] = csv.writer(handles[year])
                    writers[year].writerow(header)
                    report['years'][year] = dict(rows=0, first=None, last=None)
                midnight = int(datetime.combine(day, datetime.min.time(), UTC).timestamp())
                for sec in sorted(bid.keys() & ask.keys()):
                    b, a = bid[sec], ask[sec]
                    # Source emits flat placeholders outside trading hours. Keep minutes
                    # if either side has activity; never invent prices for absent minutes.
                    if b[4] == a[4] == 0:
                        report['zero_volume_pairs'] += 1
                        continue
                    ts = midnight + sec
                    gap = (ts - previous) // 60 - 1 if previous is not None else 0
                    if gap:
                        gaps.writerow([iso(previous), iso(ts), gap])
                        report['gap_count'] += 1
                    differences = ((a[0]-b[0])/10, (a[3]-b[3])/10)
                    if min(differences) < 0:
                        report['crossed_open_or_close'] += 1
                    row = [iso(ts), ts*1000, iso(ts+60)]
                    for values in (b, a):
                        row += [f'{v/100000:.5f}' for v in values[:4]] + [f'{values[4]:.8g}']
                    writers[year].writerow(row + [f'{v:.1f}' for v in differences] + [gap])
                    stats = report['years'][year]
                    stats['rows'] += 1
                    stats['first'] = stats['first'] or iso(ts)
                    stats['last'] = iso(ts)
                    previous = ts
        finally:
            for handle in handles.values():
                handle.close()
    report['outputs'] = {p.name: dict(bytes=p.stat().st_size,
        sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in out.glob('*.csv')}
    (root / 'quality-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k not in ('missing_or_empty_days', 'outputs')}, indent=2))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--start', default='2024-01-01')
    p.add_argument('--end', default='2026-11-06', help='Exclusive UTC date')
    p.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1] / 'data/eurusd-m1-2024-2026')
    p.add_argument('--base-url', default='https://datafeed.dukascopy.com/datafeed')
    p.add_argument('--workers', type=int, default=2)
    p.add_argument('--prepare-only', action='store_true')
    p.add_argument('--offline', action='store_true', help='Audit existing files, explicitly mark missing days')
    args = p.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    manifest_path = root / 'manifest.json'
    if args.prepare_only:
        manifest = json.loads(manifest_path.read_text())
    else:
        start, end = date.fromisoformat(args.start), date.fromisoformat(args.end)
        if not start < end <= datetime.now(UTC).date():
            p.error('Require start < end <= current UTC date (completed days only)')
        jobs = [(start + timedelta(days=n), side) for n in range((end-start).days)
                for side in ('BID', 'ASK')]
        manifest = dict(source='Dukascopy', instrument='EURUSD', timeframe='M1',
                        start=args.start, end=args.end, retrieved_at=iso(time.time()), files=[])
        limited = False
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
            # Bounded batches: a 429 stops new network requests immediately.
            for offset in range(0, len(jobs), args.workers):
                batch = jobs[offset:offset+args.workers]
                futures = [pool.submit(fetch, job, root, args.base_url, args.offline or limited) for job in batch]
                for future in futures:
                    item = future.result()
                    manifest['files'].append(item)
                    if item['status'] == 'rate_limited':
                        limited = True
                        print('HTTP 429: stopping network requests; preserving partial data.', flush=True)
                if offset % 50 < args.workers:
                    print(f'{len(manifest["files"])}/{len(jobs)} checked', flush=True)
                    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
                if not (args.offline or limited):
                    time.sleep(1)
        manifest['files'].sort(key=lambda f: (f['date'], f['side']))
        manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')
    prepare(root, manifest)
    if any(f['status'] in ('error', 'rate_limited', 'not_downloaded') for f in manifest['files']):
        raise SystemExit('Dataset is INCOMPLETE; rerun to resume. See manifest.json.')


if __name__ == '__main__':
    main()
