import importlib.util
import json
import lzma
from pathlib import Path
import struct
import tempfile
import unittest
import hashlib

spec = importlib.util.spec_from_file_location('forex_data', Path(__file__).parents[1] / 'scripts/forex_data.py')
fx = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fx)


class ForexDataTests(unittest.TestCase):
    def test_binary_order_and_duplicate_rejection(self):
        row = struct.pack('>5If', 60, 110000, 110010, 109990, 110020, 4.5)
        self.assertEqual(fx.decode(lzma.compress(row))[60],
                         (110000, 110020, 109990, 110010, 4.5))
        with self.assertRaises(ValueError):
            fx.decode(lzma.compress(row + row))

    def test_invalid_ohlc_and_truncated_payload(self):
        with self.assertRaises(ValueError):
            fx.decode(lzma.compress(struct.pack('>5If', 0, 10, 12, 11, 13, 1)))
        with self.assertRaises(ValueError):
            fx.decode(lzma.compress(b'bad'))

    def test_join_utc_and_no_fill(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            files = []
            for side, delta in [('BID', 0), ('ASK', 10)]:
                # One inactive minute and one missing ask minute must not be filled.
                seconds = [0, 60, 120, 180] if side == 'BID' else [0, 60, 180]
                data = b''.join(struct.pack('>5If', sec, 110000+delta, 110010+delta,
                    109990+delta, 110020+delta, 0 if sec == 60 else 1) for sec in seconds)
                payload = lzma.compress(data)
                path = root / f'{side}.bi5'
                path.write_bytes(payload)
                files.append(dict(date='2024-01-02', side=side, status='ok',
                    path=str(path), sha256=hashlib.sha256(payload).hexdigest()))
            fx.prepare(root, dict(start='2024-01-02', end='2024-01-03', files=files))
            import csv
            with (root / 'prepared/EURUSD-M1-bid-ask-2024.csv').open() as f:
                rows = list(csv.DictReader(f))
            self.assertEqual(len(rows), 2)
            self.assertEqual(rows[0]['timestamp_utc'], '2024-01-02T00:00:00Z')
            self.assertEqual(rows[0]['bar_end_utc'], '2024-01-02T00:01:00Z')
            self.assertEqual(rows[0]['bid_close'], '1.10010')
            self.assertEqual(rows[0]['ask_close'], '1.10020')
            self.assertEqual(rows[1]['gap_before_minutes'], '2')
            report = json.loads((root / 'quality-report.json').read_text())
            self.assertEqual(report['unmatched_minutes'], 1)
            self.assertEqual(report['zero_volume_pairs'], 1)
