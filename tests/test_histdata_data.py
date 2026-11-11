import contextlib
import csv
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('histdata', Path(__file__).parents[1]/'scripts/histdata_data.py')
hd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hd)


class HistDataTests(unittest.TestCase):
    def test_paired_quotes_ohlc_and_fixed_timezone_in_summer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root/'sample.zip'
            # Same millisecond with a different quote is valid; exact repeat is skipped.
            data = ('20240701 120001001,1.100000,1.100200,0\n'
                    '20240701 120001001,1.100000,1.100200,0\n'
                    '20240701 120001001,1.100100,1.100400,0\n'
                    '20240701 120059999,1.099900,1.100000,0\n'
                    '20240701 120200001,1.100000,1.100200,0\n')
            with zipfile.ZipFile(path,'w') as z:
                z.writestr('sample.csv',data)
            manifest = dict(files=[dict(file='sample.zip',month='2024-07',
                sha256=hashlib.sha256(path.read_bytes()).hexdigest())],missing_months=[])
            with contextlib.redirect_stdout(io.StringIO()):
                hd.prepare(root,manifest)
            with (root/'prepared/EURUSD-M1-bid-ask-2024.csv').open() as f:
                rows=list(csv.DictReader(f))
            self.assertEqual(len(rows),2)
            self.assertEqual(rows[0]['timestamp_utc'],'2024-07-01T17:00:00Z')
            self.assertEqual(rows[0]['bar_end_utc'],'2024-07-01T17:01:00Z')
            self.assertEqual(rows[0]['quote_count'],'3')
            self.assertEqual(rows[0]['bid_high'],'1.100100')
            self.assertEqual(rows[0]['ask_close'],'1.100000')
            self.assertEqual(rows[0]['spread_min_pips'],'1.0000')
            self.assertEqual(rows[0]['spread_max_pips'],'3.0000')
            self.assertEqual(rows[0]['spread_mean_pips'],'2.0000')
            self.assertEqual(rows[1]['gap_before_minutes'],'1')
            report=json.loads((root/'quality-report.json').read_text())
            self.assertEqual(report['exact_consecutive_duplicates'],1)

    def test_exact_price_parsing(self):
        self.assertEqual(hd.price('1.000001'),1000001)
        for bad in ('NaN','-1','0','1.0000001'):
            with self.assertRaises(ValueError):
                hd.price(bad)

    def test_repeated_clock_hour_is_quarantined_not_sorted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            path=root/'sample.zip'
            stamps=['20251026 185900000','20251026 190000000',
                    '20251026 195958437','20251026 190000248','20251026 200000000']
            with zipfile.ZipFile(path,'w') as z:
                z.writestr('sample.csv',''.join(s+',1.100000,1.100100,0\n' for s in stamps))
            manifest=dict(files=[dict(file='sample.zip',month='2025-10',
                sha256=hashlib.sha256(path.read_bytes()).hexdigest())],missing_months=[])
            with contextlib.redirect_stdout(io.StringIO()):
                hd.prepare(root,manifest)
            with (root/'prepared/EURUSD-M1-bid-ask-2025.csv').open() as f:
                rows=list(csv.DictReader(f))
            self.assertEqual(len(rows),2)
            self.assertEqual(rows[1]['gap_before_minutes'],'60')
            report=json.loads((root/'quality-report.json').read_text())
            self.assertEqual(report['quarantined_quotes'],3)
            self.assertEqual(len(report['timestamp_reversals']),1)

    def test_jpy_symbol_and_pip_units(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); path=root/'sample.zip'
            with zipfile.ZipFile(path,'w') as z:
                z.writestr('sample.csv','20240701 120001001,153.123,153.133,0\n')
            manifest=dict(symbol='USDJPY',files=[dict(file='sample.zip',month='2024-07',sha256=hashlib.sha256(path.read_bytes()).hexdigest())],missing_months=[])
            with contextlib.redirect_stdout(io.StringIO()): hd.prepare(root,manifest)
            self.assertTrue((root/'prepared/USDJPY-M1-bid-ask-2024.csv').exists())
            with (root/'prepared/USDJPY-M1-bid-ask-2024.csv').open() as f: row=next(csv.DictReader(f))
            self.assertEqual(row['spread_open_pips'],'1.0000')
            self.assertEqual(json.loads((root/'quality-report.json').read_text())['symbol'],'USDJPY')
