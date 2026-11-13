import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from upscale_markets import resolve_market


def market(ticker, base, quote='USD'):
    return {'id': 'synthetic-market-id',
            'config': {'ticker': ticker, 'baseAsset': base, 'quoteAsset': quote},
            'state': {'indexPrice': '159889999999', 'blockTimestamp': 1000}}


class MarketMappingTests(unittest.TestCase):
    def test_three_symbols_preserve_original_market_and_price(self):
        markets = [market('EUR/USD', 'EUR'), market('GBP/USD', 'GBP'),
                   market('USDJPY/USD', 'USDJPY')]
        original = copy.deepcopy(markets)
        for symbol, expected in zip(['EURUSD', 'GBPUSD', 'USDJPY'], markets):
            with self.subTest(symbol=symbol):
                self.assertIs(resolve_market(markets, symbol), expected)
        self.assertEqual(markets, original)

    def test_missing_duplicate_and_unknown_are_rejected(self):
        eur = market('EUR/USD', 'EUR')
        for markets, symbol in [([], 'EURUSD'), ([eur, copy.deepcopy(eur)], 'EURUSD'),
                                ([market('AUD/USD', 'AUD')], 'AUDUSD')]:
            with self.subTest(symbol=symbol, count=len(markets)):
                with self.assertRaises(ValueError):
                    resolve_market(markets, symbol)

    def test_incorrect_identity_is_rejected(self):
        for field, value in [('baseAsset', 'GBP'), ('quoteAsset', 'JPY')]:
            bad = market('EUR/USD', 'EUR')
            bad['config'][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                resolve_market([bad], 'EURUSD')
        for value in [None, '', 123]:
            bad = market('EUR/USD', 'EUR')
            bad['id'] = value
            with self.subTest(id=value), self.assertRaises(ValueError):
                resolve_market([bad], 'EURUSD')

    def test_slash_removal_and_inverse_pair_are_not_fallbacks(self):
        for ticker, base in [('USD/JPY', 'USD'), ('JPY/USD', 'JPY'), ('USDJPY', 'USDJPY')]:
            with self.subTest(ticker=ticker), self.assertRaises(ValueError):
                resolve_market([market(ticker, base)], 'USDJPY')


if __name__ == '__main__':
    unittest.main()
