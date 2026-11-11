use forex_core::{
    engine::{Engine, Timeframe},
    feed::{Aggregator, Tick},
    trading::Quote,
};
#[test]
fn shared_processing_matches_existing_sequence_and_snapshot() {
    let cfg = forex_core::load(std::path::Path::new("../strategy/fvg-bot-rules.json")).unwrap();
    let mut old = Engine::default();
    let mut new = Engine::default();
    let mut a = Aggregator::default();
    let mut b = Aggregator::default();
    let start = forex_core::prices::utc("2026-03-06T22:00:00Z").unwrap();
    for i in 0..200 {
        let t = Tick {
            symbol: "EURUSD".into(),
            source: "fixture".into(),
            millis: (start + i * 3600) * 1000 + 123,
            bid: 1_100_000 + i % 7 * 100,
            ask: 1_100_010 + i % 7 * 100,
        };
        for (tf, bar) in a.push(t.clone()).unwrap() {
            old.closed(tf, bar, &cfg).unwrap();
        }
        old.quote(
            Quote {
                time: t.millis / 1000,
                bid: t.bid,
                ask: t.ask,
            },
            1,
            &cfg,
        )
        .unwrap();
        old.namespace_signals("EURUSD");
        forex_core::processing::process(&mut new, &mut b, &t, &cfg, true).unwrap();
        assert_eq!(
            serde_json::to_value(&old).unwrap(),
            serde_json::to_value(&new).unwrap()
        );
        if i == 99 {
            b = serde_json::from_value(serde_json::to_value(&b).unwrap()).unwrap();
        }
    }
    assert_eq!(old.trend(Timeframe::W), new.trend(Timeframe::W));
}
