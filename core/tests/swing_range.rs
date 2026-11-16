//! Range trading in the swing rule set (requirements: Торговля в боковике).
use forex_core::swing::*;

const H4: i64 = 14_400;
fn cfg() -> SwingConfig {
    SwingConfig {
        entry_close_hours: vec![0, 4, 8, 12, 16, 20],
        range: RangeConfig {
            enabled: true,
            min_h4_bars: 5,
            window_min: 3,
            window_max: 10,
            impulse_bars: 3,
            wick_min: 0.1,
            ..RangeConfig::default()
        },
        ..SwingConfig::default()
    }
}
fn bar(i: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
    Bar {
        end: i * H4,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
fn flat(i: i64) -> Bar {
    bar(i, 103.4, 104.0, 103.0, 103.6)
}
/// ATR(D) = 1.0; H4: three impulse bars, then flat bars inside 103.0..104.0 up to `last`.
fn swing(c: SwingConfig, impulse: bool, last: i64) -> Swing {
    let mut s = Swing::new(c);
    for t in 1..=3 {
        s.close(
            Tf::D,
            Bar {
                end: t,
                open: 100.0,
                high: 100.5,
                low: 99.5,
                close: 100.0,
            },
            t,
        );
    }
    let first = if impulse {
        vec![
            bar(1, 100.0, 100.9, 99.9, 100.8),
            bar(2, 100.8, 101.9, 100.7, 101.8),
            bar(3, 101.8, 103.2, 102.4, 103.1),
        ]
    } else {
        (1..=3).map(flat).collect()
    };
    for b in first.into_iter().chain((4..=last).map(flat)) {
        s.close(Tf::H4, b, b.end);
    }
    s
}
fn h4(s: &mut Swing, b: Bar) {
    s.close(Tf::H4, b, b.end);
}

#[test]
fn range_after_an_impulse_is_found_with_its_borders() {
    assert_eq!(swing(cfg(), true, 7).range_box(), Some((104.0, 103.0)));
}

#[test]
fn stall_without_an_impulse_is_not_a_range() {
    assert_eq!(swing(cfg(), false, 7).range_box(), None);
}

#[test]
fn disabled_module_finds_no_range() {
    let mut c = cfg();
    c.range.enabled = false;
    assert_eq!(swing(c, true, 7).range_box(), None);
}

#[test]
fn wick_above_the_top_and_close_inside_sells_toward_the_bottom() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.15, 103.5, 103.9));
    let t = &s.trades()[0];
    assert_eq!(
        (t.kind, t.dir, t.state),
        (Kind::Range, -1, TradeState::Active)
    );
    assert!(t.stop > 104.15, "stop beyond the deviation extreme");
    let risk = t.stop - 103.9;
    assert!((t.tp - (103.9 - 2.0 * risk)).abs() < 1e-12, "fixed 2R");
    assert!(t.tp > 103.0, "2R is reached before the opposite border");
}

#[test]
fn small_wick_or_close_outside_gives_no_signal() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.05, 103.5, 103.9));
    h4(&mut s, bar(9, 103.9, 104.4, 103.8, 104.2));
    assert!(s.trades().is_empty());
}

#[test]
fn close_back_inside_after_a_close_outside_is_a_signal() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.3, 103.8, 104.2));
    h4(&mut s, bar(9, 104.2, 104.25, 103.7, 103.8));
    assert_eq!(s.trades().len(), 1);
    assert!(
        s.trades()[0].stop > 104.3,
        "stop beyond both deviation bars"
    );
}

#[test]
fn far_close_beyond_a_border_ends_the_range() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.7, 103.6, 104.6));
    assert_eq!(s.range_box(), None);
}

#[test]
fn only_one_range_trade_at_a_time() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.15, 103.5, 103.9));
    h4(&mut s, bar(9, 103.9, 103.95, 102.85, 103.1));
    assert_eq!(s.trades().len(), 1);
}

#[test]
fn closed_range_trade_ends_the_range_and_the_same_impulse_is_not_reused() {
    let mut s = swing(cfg(), true, 7);
    h4(&mut s, bar(8, 103.6, 104.15, 103.5, 103.9));
    let tp = s.trades()[0].tp;
    s.minute(8 * H4 + 60, 103.9, tp - 0.01);
    assert_eq!(s.trades()[0].state, TradeState::Target);
    for i in 9..=14 {
        h4(&mut s, flat(i));
    }
    assert_eq!(s.range_box(), None);
    h4(&mut s, bar(15, 103.6, 104.15, 103.5, 103.9));
    assert_eq!(s.trades().len(), 1);
}

#[test]
fn deviation_outside_entry_hours_is_skipped() {
    let mut c = cfg();
    c.entry_close_hours = vec![12];
    let mut s = swing(c, true, 7);
    h4(&mut s, bar(8, 103.6, 104.15, 103.5, 103.9)); // closes 08:00
    assert!(s.trades().is_empty());
}
