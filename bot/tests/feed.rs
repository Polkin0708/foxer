use forex_bot::engine::Timeframe;
use forex_bot::feed::{Aggregator, Tick};
fn tick(ms: i64, price: i64) -> Tick {
    Tick {
        symbol: "EURUSD".into(),
        millis: ms,
        price,

        source: "upscale:index".into(),
    }
}
#[test]
fn typed_tick_preserves_milliseconds_and_exact_prices_after_restart() {
    let original = tick(1_794_000_000_123, 1_123_456);
    let mut a = Aggregator::default();
    a.push(original.clone()).unwrap();
    let mut restored: Aggregator =
        serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
    assert_eq!(restored.last_quote_millis(), Some(original.millis));
    assert!(restored.push(original.clone()).unwrap().is_empty());
    let last = &serde_json::to_value(restored).unwrap()["last"];
    assert_eq!(last["price"], original.price);
}
#[test]
fn aggregator_rejects_invalid_ticks_without_changing_state() {
    let mut a = Aggregator::default();
    a.push(tick(1000, 100)).unwrap();
    let before = serde_json::to_value(&a).unwrap();
    let mut invalid = vec![tick(-1, 100), tick(2000, 0)];
    let mut missing = tick(2000, 100);
    missing.source.clear();
    invalid.push(missing);
    let mut changed = tick(2000, 100);
    changed.source = "other-source".into();
    invalid.push(changed);
    for t in invalid {
        assert!(a.push(t).is_err());
        assert_eq!(serde_json::to_value(&a).unwrap(), before);
    }
}
#[test]
fn aggregator_closes_index_bar_at_fixed_boundary_and_survives_restart() {
    let mut a = Aggregator::default();
    let start = 79200_i64 * 1000;
    assert!(a.push(tick(start, 100)).unwrap().is_empty());
    a.push(tick(start + 1000, 110)).unwrap();
    let mut a: Aggregator = serde_json::from_value(serde_json::to_value(a).unwrap()).unwrap();
    a.push(tick(start + 2000, 90)).unwrap();
    let bars = a.push(tick(start + 14400000, 120)).unwrap();
    assert_eq!(bars.len(), 1);
    assert_eq!(bars[0].0, Timeframe::H4);
    let b = &bars[0].1;
    assert_eq!((b.open, b.high, b.low, b.close), (100, 110, 90, 90));
    assert!(a.push(tick(start + 14400000, 120)).unwrap().is_empty());
    assert!(a.push(tick(start + 1000, 90)).is_err());
}
#[test]
fn cold_partial_and_interrupted_bars_are_not_reported_as_complete() {
    let mut a = Aggregator::default();
    let start = 79200_i64 * 1000;
    a.push(tick(start + 1000, 100)).unwrap();
    assert!(a.push(tick(start + 14400000, 110)).unwrap().is_empty());
    a.disconnected();
    assert!(a.push(tick(start + 28800000, 120)).unwrap().is_empty());
}
