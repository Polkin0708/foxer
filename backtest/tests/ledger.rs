use forex_backtest::Ledger;
use forex_core::{
    feed::Tick,
    processing::Change,
    trading::{Side, Signal, SignalState},
};
fn tick(ms: i64, bid: i64, ask: i64) -> Tick {
    Tick {
        symbol: "EURUSD".into(),
        source: "test".into(),
        millis: ms,
        bid,
        ask,
    }
}
fn change(side: Side, state: SignalState, previous: Option<SignalState>) -> Change {
    Change {
        signal: Signal {
            id: "x".into(),
            side,
            entry: 100,
            stop: if side == Side::Long { 90 } else { 110 },
            target: if side == Side::Long { 120 } else { 80 },
            take_profit: if side == Side::Long { 120 } else { 80 },
            state,
            last_quote_time: 1,
        },
        previous,
        basis: serde_json::json!({"rule":"fixture"}),
    }
}
#[test]
fn stop_gap_uses_exit_side_and_milliseconds() {
    let mut l = Ledger::default();
    l.observe(
        "EURUSD",
        &tick(1001, 99, 100),
        &[change(Side::Long, SignalState::Active, None)],
    );
    l.observe(
        "EURUSD",
        &tick(1002, 85, 86),
        &[change(
            Side::Long,
            SignalState::Stop,
            Some(SignalState::Active),
        )],
    );
    assert_eq!(l.trades.len(), 1);
    assert_eq!(l.trades[0]["result_r"], -1.5);
    assert_eq!(l.trades[0]["exit_at"], 1002);
    assert_eq!(l.summary()["closed"], 1);
}
#[test]
fn short_take_profit_fills_level_without_improvement() {
    let mut l = Ledger::default();
    l.observe(
        "EURUSD",
        &tick(1001, 100, 101),
        &[change(Side::Short, SignalState::Active, None)],
    );
    l.observe(
        "EURUSD",
        &tick(1002, 75, 76),
        &[change(
            Side::Short,
            SignalState::TakeProfit,
            Some(SignalState::Active),
        )],
    );
    assert_eq!(l.trades[0]["result_r"], 2.0);
    assert_eq!(l.trades[0]["exit"], 80);
}
#[test]
fn empty_and_open_are_not_closed_results() {
    let mut l = Ledger::default();
    assert_eq!(l.summary()["closed"], 0);
    assert!(l.summary()["profit_factor"].is_null());
    l.observe(
        "EURUSD",
        &tick(1001, 99, 100),
        &[change(Side::Long, SignalState::Active, None)],
    );
    l.observe("EURUSD", &tick(2001, 105, 106), &[]);
    assert_eq!(l.summary()["closed"], 0);
    assert_eq!(l.trades[0]["unrealized_r"], 0.5);
}

#[test]
fn pending_cancel_never_becomes_a_trade_and_gap_is_recorded() {
    let mut l = Ledger::default();
    l.observe(
        "EURUSD",
        &tick(1001, 104, 105),
        &[change(Side::Long, SignalState::Pending, None)],
    );
    l.gap("EURUSD", 1002, 2000);
    l.observe(
        "EURUSD",
        &tick(2001, 85, 86),
        &[change(
            Side::Long,
            SignalState::Cancelled,
            Some(SignalState::Pending),
        )],
    );
    assert!(l.trades[0]["entry_at"].is_null());
    assert_eq!(l.summary()["cancelled"], 1);
    assert_eq!(l.summary()["gap_affected"], 1);
    assert_eq!(l.summary()["closed"], 0);
}
#[test]
fn metrics_and_examples_are_deterministic_and_do_not_duplicate() {
    let trades:Vec<_>=(0..15).map(|i|serde_json::json!({"id":i.to_string(),"symbol":"EURUSD","state":"Stop","result_r":if i%2==0{-1.0}else{2.0},"entry_at":i*1000,"exit_at":i*1000+500,"gaps":[]})).collect();
    let s = forex_backtest::stats(&trades);
    assert_eq!(s["closed"], 15);
    assert_eq!(s["net_r"], 6.0);
    assert_eq!(s["max_closed_drawdown_r"], 1.0);
    assert_eq!(s["mean_duration_seconds"], 0.5);
    let a = forex_backtest::examples(&trades);
    assert_eq!(a, forex_backtest::examples(&trades));
    assert_eq!(a.len(), 9);
    let mut unique = a;
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 9);
}
