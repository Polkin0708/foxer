use forex_core::{
    engine::Timeframe,
    feed::{Aggregator, Tick},
    prices::{utc, Minute},
};
#[test]
fn partial_current_bar_survives_history_boundary_without_future_prices() {
    let start = utc("2026-01-02T06:00:00Z").unwrap();
    let mut agg = Aggregator::default();
    let m = Minute {
        timestamp: start,
        bid: [100, 115, 95, 110],
        ask: [101, 116, 96, 111],
        quotes: 4,
        gap_before_minutes: 0,
    };
    assert!(agg.minute("USDJPY", "fixture", &m).unwrap().is_empty());
    assert!(agg.closed_before(start + 60).is_empty());
    let bars = agg
        .push(Tick {
            symbol: "USDJPY".into(),
            source: "fixture".into(),
            millis: (start + 14400) * 1000,
            bid: 999,
            ask: 1000,
        })
        .unwrap();
    let h4 = bars.iter().find(|(tf, _)| *tf == Timeframe::H4).unwrap();
    assert_eq!(
        (h4.1.open, h4.1.high, h4.1.low, h4.1.close),
        (100, 115, 95, 110)
    );
}
#[test]
fn known_gap_prevents_publishing_incomplete_bar() {
    let start = utc("2026-01-02T06:00:00Z").unwrap();
    let mut a = Aggregator::default();
    for i in [0, 1] {
        let t = Tick {
            symbol: "EURUSD".into(),
            source: "fixture".into(),
            millis: (start + i * 14400) * 1000,
            bid: 100,
            ask: 101,
        };
        if i == 1 {
            a.disconnected();
        }
        assert!(a.push(t).unwrap().is_empty());
    }
}
