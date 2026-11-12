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
        price: [100, 115, 95, 110],

        quotes: 4,
        gap_before_minutes: 0,
    };
    assert!(agg
        .minute("USDJPY", "upscale:index", &m)
        .unwrap()
        .is_empty());
    assert!(agg.closed_before(start + 60).is_empty());
    for i in 1..240 {
        let next = Minute {
            timestamp: start + i * 60,
            price: [110; 4],
            quotes: 1,
            gap_before_minutes: 0,
        };
        assert!(agg
            .minute("USDJPY", "upscale:index", &next)
            .unwrap()
            .is_empty());
    }
    let bars = agg
        .push(Tick {
            symbol: "USDJPY".into(),
            source: "upscale:index".into(),
            millis: (start + 14400) * 1000,
            price: 999,
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
            source: "upscale:index".into(),
            millis: (start + i * 14400) * 1000,
            price: 100,
        };
        if i == 1 {
            a.disconnected();
        }
        assert!(a.push(t).unwrap().is_empty());
    }
}

#[test]
fn incomplete_index_minutes_cannot_seed_a_complete_bar() {
    let start = utc("2026-01-02T06:00:00Z").unwrap();
    let minute = |timestamp, gap_before_minutes| Minute {
        timestamp,
        price: [100, 115, 95, 110],
        quotes: 4,
        gap_before_minutes,
    };
    let mut a = Aggregator::default();
    a.minute("EURUSD", "upscale:index", &minute(start, 0))
        .unwrap();
    assert!(
        a.closed_before(start + 14400).is_empty(),
        "one minute cannot complete H4"
    );
    assert!(a
        .minute("EURUSD", "upscale:index", &minute(start + 14400, 239))
        .unwrap()
        .is_empty());
}
