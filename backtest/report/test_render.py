import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from pypdf import PdfReader
spec = importlib.util.spec_from_file_location('render', Path(__file__).with_name('render.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ReportTests(unittest.TestCase):
    def test_empty_report_contains_method_and_no_fabricated_trades(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            summary = dict(closed=0, net_r=0, max_closed_drawdown_r=0, win_rate=None,
                           mean_r=None, profit_factor=None, mean_duration_seconds=None,
                           open=0, pending=0, cancelled=0, gap_affected=0, curve=[])
            result = dict(schema_version=1, from_=0, to=86400000, symbols=['EURUSD'],
                          strategy_sha256='a'*64, summary=summary, by_symbol={'EURUSD':summary},
                          monthly={}, gap_affected_summary=summary, trades=[], examples=[],
                          sources={}, coverage={}, initial_context={}, engine_version='test')
            result['from'] = result.pop('from_')
            (p/'results.json').write_text(json.dumps(result))
            module.render(p)
            first = (p/'report.pdf').read_bytes()
            module.render(p)
            self.assertEqual(first, (p/'report.pdf').read_bytes())
            pdf = PdfReader(p/'report.pdf')
            text = '\n'.join(page.extract_text() for page in pdf.pages)
            self.assertIn('Сделки не закрывались', text)
            self.assertIn('Спред', text)
            self.assertIn('EURUSD', text)
            self.assertGreaterEqual(len(pdf.pages), 2)

if __name__ == '__main__': unittest.main()
