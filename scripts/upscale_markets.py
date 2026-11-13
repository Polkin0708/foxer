"""Market lookup for read-only Upscale diagnostics; no price conversion."""

# External API identities, not the strategy's enabled instrument list.
_IDENTITIES = {
    'EURUSD': ('EUR/USD', 'EUR', 'USD'),
    'GBPUSD': ('GBP/USD', 'GBP', 'USD'),
    'USDJPY': ('USDJPY/USD', 'USDJPY', 'USD'),
}


def resolve_market(markets, symbol):
    """Return one original market, refusing missing or ambiguous identities."""
    if symbol not in _IDENTITIES:
        raise ValueError('unsupported Upscale symbol')
    ticker, base, quote = _IDENTITIES[symbol]
    matches = [
        market for market in markets
        if market.get('config', {}).get('ticker') == ticker
    ]
    if len(matches) != 1:
        raise ValueError('missing or ambiguous Upscale market')
    market = matches[0]
    config = market['config']
    if config.get('baseAsset') != base or config.get('quoteAsset') != quote:
        raise ValueError('unexpected Upscale market identity')
    if not isinstance(market.get('id'), str) or not market['id'].strip():
        raise ValueError('missing Upscale market ID')
    return market
