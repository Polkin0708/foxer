//! Dollar index and the exit by the index (requirements: Выход по индексу доллара).
use forex_core::swing::*;

const H4: i64 = 14_400;
fn ohlc(o: f64, h: f64, l: f64, c: f64) -> Ohlc {
    Ohlc {
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
fn one() -> Ohlc {
    ohlc(1.0, 1.0, 1.0, 1.0)
}

#[test]
fn dollar_index_uses_ice_weights_and_the_extremes_that_move_it() {
    let base = dollar_index(&[one(); 6]);
    assert!((base.close - 50.14348112).abs() < 1e-9);
    let mut pairs = [one(); 6];
    pairs[0] = ohlc(1.1, 1.2, 1.0, 1.1); // EURUSD: weight -0.576
    pairs[1] = ohlc(1.0, 1.5, 0.5, 1.0); // USDJPY: weight +0.136
    let x = dollar_index(&pairs);
    let c = 50.14348112;
    assert!((x.close - c * 1.1f64.powf(-0.576)).abs() < 1e-9);
    assert!((x.high - c * 1.0f64.powf(-0.576) * 1.5f64.powf(0.136)).abs() < 1e-9);
    assert!((x.low - c * 1.2f64.powf(-0.576) * 0.5f64.powf(0.136)).abs() < 1e-9);
}

fn cfg(sign: i8) -> SwingConfig {
    SwingConfig {
        reference_exit: Some(ReferenceExit {
            sign,
            recent_events: 50,
        }),
        ..SwingConfig::default()
    }
}
fn bar(end: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
    Bar {
        end,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
const END: i64 = 10 * 86_400 + 12 * 3600;
/// A long trend trade opened by market on the close at END (entry 102.5).
fn long_trade(c: SwingConfig) -> Swing {
    let mut s = Swing::new(c);
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    for b in [
        bar(END - 2 * H4, 100.0, 101.0, 99.0, 100.5),
        bar(END - H4, 100.5, 103.0, 100.5, 102.8),
        bar(END, 102.8, 103.0, 101.5, 102.5),
    ] {
        s.close(Tf::H4, b, b.end);
    }
    assert_eq!(s.trades()[0].state, TradeState::Active);
    s
}

#[test]
fn plan_events_are_recorded_once_with_their_times() {
    let s = long_trade(SwingConfig::default());
    assert!(s.events().contains(&(END, Event::Confirm, 1)));
    let mut s = s;
    s.minute(END + 60, 120.5, 102.0);
    s.minute(END + 120, 121.0, 102.0);
    let targets: Vec<_> = s.events().iter().filter(|e| e.1 == Event::Target).collect();
    assert_eq!(targets, [&(END + 60, Event::Target, 1)]);
}

#[test]
fn index_reaching_its_target_against_the_dollar_closes_a_long_euro_trade() {
    let mut s = long_trade(cfg(-1));
    let known = [(END + 60, Event::Target, -1)];
    s.reference_minute(END + 60, 102.9, &known);
    let t = &s.trades()[0];
    assert_eq!(t.state, TradeState::ReferenceExit);
    assert_eq!(t.exit_t, Some(END + 60));
    let expected = (102.9 - t.entry) / (t.entry - t.stop);
    assert!((t.r.unwrap() - expected).abs() < 1e-12);
}

#[test]
fn events_before_the_fill_or_in_the_other_direction_do_not_close() {
    for known in [
        [(END, Event::Target, -1)],
        [(END + 60, Event::Target, 1)],
        [(END + 60, Event::Confirm, -1)],
        [(END + 120, Event::Sweep, -1)],
    ] {
        let mut s = long_trade(cfg(-1));
        s.reference_minute(END + 60, 102.9, &known);
        assert_eq!(s.trades()[0].state, TradeState::Active, "{known:?}");
    }
    let mut s = long_trade(cfg(-1));
    s.reference_minute(END + 60, 102.9, &[(END + 60, Event::Sweep, -1)]);
    assert_eq!(s.trades()[0].state, TradeState::ReferenceExit);
}

#[test]
fn only_the_most_recent_events_are_considered() {
    let mut c = cfg(-1);
    c.reference_exit.as_mut().unwrap().recent_events = 2;
    let mut s = long_trade(c);
    let known = [
        (END + 60, Event::Target, -1),
        (END + 60, Event::Confirm, 1),
        (END + 60, Event::Confirm, 1),
    ];
    s.reference_minute(END + 60, 102.9, &known);
    assert_eq!(s.trades()[0].state, TradeState::Active);
}

#[test]
fn without_reference_exit_events_are_ignored() {
    let mut s = long_trade(SwingConfig::default());
    s.reference_minute(END + 60, 102.9, &[(END + 60, Event::Target, -1)]);
    assert_eq!(s.trades()[0].state, TradeState::Active);
}
