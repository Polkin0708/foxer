use forex_bot::trading::*;
fn bar(t: i64, o: i64, h: i64, l: i64, c: i64) -> Candle {
    Candle {
        end: t,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
fn config() -> forex_bot::model::Strategy {
    forex_bot::load(std::path::Path::new("../strategy/fvg-bot-rules.json")).unwrap()
}
#[test]
fn atr_and_fvg_use_only_complete_three_bar_pattern() {
    let bars = vec![
        bar(1, 100, 110, 90, 105),
        bar(2, 105, 125, 100, 120),
        bar(3, 125, 135, 120, 130),
    ];
    assert_eq!(atr(&bars, 3), Some(20.0));
    assert!(fvg(&bars[..2], 20.0, 0.05).is_none());
    let z = fvg(&bars, 20.0, 0.05).unwrap();
    assert_eq!((z.low, z.high, z.side), (110, 120, Side::Long));
    assert!(fvg(&bars, 20.0, 0.6).is_none());
}
#[test]
fn gap_is_close_to_open_even_when_wicks_overlap() {
    let bars = [bar(1, 100, 120, 90, 105), bar(2, 110, 125, 95, 115)];
    let z = gap(&bars, 20.0, 0.05).unwrap();
    assert_eq!((z.low, z.high, z.kind), (105, 110, ZoneKind::Gap));
    assert!(gap(&bars[..1], 20.0, 0.05).is_none());
}
#[test]
fn reaction_on_test_bar_prevents_later_inversion() {
    let mut z = Zone::new(Side::Long, 100, 110, 1, ZoneKind::Fvg);
    z.observe(&bar(2, 115, 120, 105, 116), 20.0, 0.1);
    assert_eq!(z.state, ZoneState::Reacted);
    z.observe(&bar(3, 100, 105, 90, 95), 20.0, 0.1);
    assert_eq!(z.state, ZoneState::Reacted);
}
#[test]
fn inversion_requires_margin_and_no_prior_reaction() {
    let mut z = Zone::new(Side::Long, 100, 110, 1, ZoneKind::Fvg);
    z.observe(&bar(2, 105, 108, 99, 99), 20.0, 0.1);
    assert_eq!(z.state, ZoneState::Tested);
    z.observe(&bar(3, 99, 100, 95, 97), 20.0, 0.1);
    assert_eq!(z.state, ZoneState::Inverted);
}
#[test]
fn fractal_is_confirmed_by_right_bar_and_strict() {
    let bars = [
        bar(1, 100, 110, 90, 100),
        bar(2, 100, 120, 80, 100),
        bar(3, 100, 110, 90, 100),
    ];
    assert!(fractals(&bars[..2]).is_empty());
    let f = fractals(&bars);
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].confirmed_at, 3);
    let equal = [bars[0].clone(), bars[1].clone(), bars[1].clone()];
    assert!(fractals(&equal).is_empty());
}
#[test]
fn short_stop_uses_latest_fractal_not_nearest_price() {
    let cfg = config();
    let f = [
        Fractal {
            price: 1_102_000,
            confirmed_at: 1,
            upper: true,
            swept: false,
        },
        Fractal {
            price: 1_105_000,
            confirmed_at: 2,
            upper: true,
            swept: false,
        },
    ];
    let stop = select_stop(Side::Short, 1_100_000, 1_101_000, &f, &[], 1000.0, 10, &cfg).unwrap();
    assert_eq!(stop, 1_105_050);
}
#[test]
fn entries_use_index_price_limit_rounding_and_structural_target_cap() {
    let cfg = config();
    let market = Signal::new(
        "a",
        Side::Long,
        Quote {
            time: 1,
            price: 101,
        },
        90,
        150,
        1,
        &cfg,
    )
    .unwrap();
    assert_eq!(market.entry, 101);
    assert_eq!(market.state, SignalState::Active);
    let cap = Signal::new(
        "b",
        Side::Short,
        Quote {
            time: 1,
            price: 101,
        },
        110,
        -100,
        1,
        &cfg,
    );
    assert!(cap.is_err());
    let cap = Signal::new(
        "c",
        Side::Long,
        Quote {
            time: 1,
            price: 101,
        },
        100,
        200,
        1,
        &cfg,
    )
    .unwrap();
    assert_eq!((cap.target, cap.take_profit), (200, 111));
    let limit = Signal::new(
        "d",
        Side::Long,
        Quote {
            time: 1,
            price: 110,
        },
        100,
        120,
        1,
        &cfg,
    )
    .unwrap();
    assert_eq!((limit.entry, limit.target, limit.stop), (106, 120, 100));
    assert_eq!(limit.state, SignalState::Pending);
}
#[test]
fn pending_cancel_is_terminal_and_uses_index_price() {
    let cfg = config();
    let mut s = Signal::new(
        "a",
        Side::Long,
        Quote {
            time: 1,
            price: 110,
        },
        100,
        120,
        1,
        &cfg,
    )
    .unwrap();
    s.observe(Quote {
        time: 2,
        price: 120,
    });
    assert_eq!(s.state, SignalState::Cancelled);
    s.observe(Quote {
        time: 3,
        price: 105,
    });
    assert_eq!(s.state, SignalState::Cancelled);
    let mut short = Signal::new(
        "b",
        Side::Short,
        Quote {
            time: 1,
            price: 109,
        },
        120,
        100,
        1,
        &cfg,
    )
    .unwrap();
    short.observe(Quote {
        time: 2,
        price: 120,
    });
    assert_eq!(short.state, SignalState::Cancelled);
}
#[test]
fn signals_restore_and_repeat_quotes_without_duplicate_results() {
    let cfg = config();
    let mut s = Signal::new(
        "a",
        Side::Short,
        Quote {
            time: 1,
            price: 110,
        },
        120,
        80,
        1,
        &cfg,
    )
    .unwrap();
    s.observe(Quote { time: 2, price: 81 });
    assert_eq!(s.state, SignalState::Active);
    let mut restored: Signal = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    restored.observe(Quote { time: 3, price: 79 });
    assert_eq!(restored.state, SignalState::TakeProfit);
    restored.observe(Quote {
        time: 4,
        price: 121,
    });
    assert_eq!(restored.state, SignalState::TakeProfit);
}
