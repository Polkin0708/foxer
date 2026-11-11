use forex_bot::engine::Timeframe;
use forex_bot::feed::{parse_frames, Aggregator, Tick};
fn tick(ms: i64, bid: i64) -> Tick {
    Tick {
        symbol: "EURUSD".into(),
        millis: ms,
        bid,
        ask: bid + 1,
        source: "test".into(),
    }
}
#[test]
fn parses_signalr_tick_and_preserves_milliseconds_and_price() {
    let v=parse_frames("{}\u{1e}{\"type\":6}\u{1e}{\"type\":1,\"target\":\"ReceiveTick\",\"arguments\":[{\"symbol\":\"EURUSD\",\"bid\":1.123456,\"ask\":1.123457,\"timestamp\":\"2026-11-06T21:59:59.123Z\",\"source\":\"test\"}]}\u{1e}").unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].bid, 1123456);
    assert_eq!(v[0].millis % 1000, 123);
}
#[test]
fn rejects_crossed_quotes_excess_precision_and_close() {
    for frame in [
        r#"{"type":7}"#,
        r#"{"type":1,"target":"ReceiveTick","arguments":[{"symbol":"EURUSD","bid":1.2,"ask":1.1,"timestamp":"2026-11-06T22:00:00Z","source":"test"}]}"#,
        r#"{"type":1,"target":"ReceiveTick","arguments":[{"symbol":"EURUSD","bid":1.1234567,"ask":1.2,"timestamp":"2026-11-06T22:00:00Z","source":"test"}]}"#,
    ] {
        assert!(parse_frames(&format!("{frame}\u{1e}")).is_err());
    }
}
#[test]
fn aggregator_closes_bid_bar_at_fixed_boundary_and_survives_restart() {
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
