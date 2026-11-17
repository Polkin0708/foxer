//! Behaviour of the "swing" rule set (requirements: Правила «свинг»).
use forex_core::swing::*;

const H4: i64 = 14_400;
fn bar(end: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
    Bar {
        end,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
fn cfg() -> SwingConfig {
    SwingConfig {
        cost: 0.01,
        ..SwingConfig::default()
    }
}
/// Three H4 bars whose third leaves a bullish gap 101.0..101.5 and closes at 102.5.
fn bullish_gap(last_end: i64) -> [Bar; 3] {
    [
        bar(last_end - 2 * H4, 100.0, 101.0, 99.0, 100.5),
        bar(last_end - H4, 100.5, 103.0, 100.5, 102.8),
        bar(last_end, 102.8, 103.0, 101.5, 102.5),
    ]
}
fn feed(s: &mut Swing, bars: &[Bar]) {
    for b in bars {
        s.close(Tf::H4, *b, b.end);
    }
}

#[test]
fn fractals_gaps_inversion_and_sweep_follow_closed_bars() {
    let mut f = Frame::new(14);
    for b in [
        bar(1, 10.0, 11.0, 9.0, 10.5),
        bar(2, 10.5, 13.0, 10.2, 12.5),
        bar(3, 12.5, 12.8, 11.5, 12.0),
    ] {
        f.push(&b);
    }
    assert_eq!(f.fr_hi.len(), 1, "middle bar is an upper fractal");
    assert_eq!(f.fr_hi[0].price, 13.0);
    assert_eq!(f.fvg.len(), 1, "third low above first high leaves a gap");
    assert_eq!((f.fvg[0].dir, f.fvg[0].lo, f.fvg[0].hi), (1, 11.0, 11.5));
    assert!(f.fvg[0].alive);
    f.push(&bar(4, 12.0, 12.2, 10.9, 11.2));
    assert!(f.fvg[0].alive, "close inside the gap keeps it alive");
    f.push(&bar(5, 11.2, 13.5, 10.5, 10.8));
    assert!(!f.fvg[0].alive, "close below the far edge inverts the gap");
    assert_eq!(f.fvg[0].inverted_at, Some(4));
    assert!(f.fr_hi[0].swept, "a later high above the fractal sweeps it");
}

#[test]
fn structure_needs_two_rising_or_two_falling_fractal_pairs() {
    let mut f = Frame::new(14);
    let mut t = 0;
    // zig-zag with rising highs and rising lows
    for (h, l) in [
        (10.0, 9.0),
        (12.0, 10.0),
        (11.0, 9.5),
        (10.5, 9.2),
        (13.0, 10.5),
        (12.0, 10.0),
        (11.8, 9.8),
        (14.0, 11.0),
    ] {
        t += 1;
        f.push(&bar(t, l, h, l, h));
    }
    assert_eq!(f.structure(), 1);
    assert_eq!(Frame::new(14).structure(), 0);
}

#[test]
fn entry_is_market_at_two_r_or_a_limit_two_r_from_the_target() {
    let c = cfg();
    let m = size_entry(1, 100.0, 99.0, 103.0, &c).unwrap();
    assert!(!m.limit);
    assert_eq!((m.entry, m.tp), (100.0, 102.0));
    let near = size_entry(1, 100.0, 99.0, 101.5, &c).unwrap();
    assert!(near.limit);
    assert!((near.entry - (101.5 + 2.0 * 99.0) / 3.0).abs() < 1e-12);
    assert_eq!(near.tp, 101.5);
    let short = size_entry(-1, 100.0, 101.0, 97.0, &c).unwrap();
    assert_eq!((short.limit, short.tp), (false, 98.0));
    assert!(
        size_entry(1, 100.0, 100.0, 103.0, &c).is_none(),
        "no risk, no trade"
    );
    let hold = SwingConfig {
        take_profit: TakeProfit::Target,
        ..cfg()
    };
    assert_eq!(size_entry(1, 100.0, 99.0, 103.0, &hold).unwrap().tp, 103.0);
    assert_eq!(
        size_entry(1, 100.0, 99.0, 110.0, &hold).unwrap().tp,
        105.0,
        "target mode is capped at rr_target_cap"
    );
}

#[test]
fn confirmed_plan_enters_on_h4_gap_only_at_allowed_close_hours() {
    let allowed = 10 * 86_400 + 12 * 3600; // 12:00
    let mut s = Swing::new(cfg());
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    feed(&mut s, &bullish_gap(allowed));
    let t = &s.trades()[0];
    assert_eq!(t.state, TradeState::Active);
    assert_eq!(t.entry, 102.5 + 0.01, "market entry pays the cost");
    assert!(
        t.stop < 101.0,
        "stop sits beyond the gap with an ATR buffer"
    );

    let blocked = 10 * 86_400 + 4 * 3600; // 04:00
    let mut s = Swing::new(cfg());
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    feed(&mut s, &bullish_gap(blocked));
    assert!(
        s.trades().is_empty(),
        "no entry at a close hour outside the list"
    );
}

#[test]
fn stop_wins_when_stop_and_target_fall_in_one_minute() {
    let end = 10 * 86_400 + 12 * 3600;
    let mut s = Swing::new(cfg());
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    feed(&mut s, &bullish_gap(end));
    let (stop, tp) = (s.trades()[0].stop, s.trades()[0].tp);
    s.minute(end + 60, tp + 1.0, stop - 1.0);
    assert_eq!(s.trades()[0].state, TradeState::Stop);
    assert_eq!(s.trades()[0].r, Some(-1.0));
}

#[test]
fn target_closes_trade_with_r_measured_from_the_paid_entry() {
    let end = 10 * 86_400 + 12 * 3600;
    let mut s = Swing::new(cfg());
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    feed(&mut s, &bullish_gap(end));
    let t = s.trades()[0].clone();
    s.minute(end + 60, t.tp, t.stop + 0.5);
    let done = &s.trades()[0];
    assert_eq!(done.state, TradeState::Target);
    let expected = (t.tp - t.entry) / (t.entry - t.stop);
    assert!((done.r.unwrap() - expected).abs() < 1e-12);
    assert!(done.r.unwrap() < 2.0, "cost reduces the fixed 2R");
}

#[test]
fn near_target_places_a_limit_that_fills_on_touch_or_is_cancelled_at_target() {
    let end = 10 * 86_400 + 12 * 3600;
    for (high, low, state) in [
        (102.6, 101.9, TradeState::Active),
        (104.1, 102.4, TradeState::Cancelled),
    ] {
        let mut s = Swing::new(cfg());
        s.add_plan(Kind::Trend, 1, (90.0, 95.0), 104.0, Tf::H4);
        feed(&mut s, &bullish_gap(end));
        let t = s.trades()[0].clone();
        assert_eq!(t.state, TradeState::Pending);
        assert!(t.entry < 102.5 && t.tp == 104.0);
        s.minute(end + 60, high, low);
        assert_eq!(s.trades()[0].state, state);
    }
}

#[test]
fn waiting_plan_expires_after_plan_days_daily_closes() {
    let mut s = Swing::new(SwingConfig {
        plan_days: 2,
        ..cfg()
    });
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    for d in 1..=3 {
        s.close(
            Tf::D,
            bar(d * 86_400, 100.0, 101.0, 99.0, 100.0),
            d * 86_400,
        );
    }
    feed(&mut s, &bullish_gap(10 * 86_400 + 12 * 3600));
    assert!(s.trades().is_empty());
}

#[test]
fn nothing_is_planned_or_traded_before_start() {
    let end = 10 * 86_400 + 12 * 3600;
    let mut s = Swing::new(SwingConfig {
        start: end + 1,
        ..cfg()
    });
    s.add_plan(Kind::Trend, 1, (90.0, 95.0), 120.0, Tf::H4);
    feed(&mut s, &bullish_gap(end));
    assert!(s.trades().is_empty());
}

#[test]
fn grid_time_is_new_york_wall_time_plus_seven_hours() {
    // 2024-01-10 02:00 UTC = 21:00 New York (EST) -> 04:00 on the source grid
    assert_eq!(
        grid_time(1_704_852_000),
        1_704_852_000 - 5 * 3600 + 7 * 3600
    );
    // 2024-07-10 01:00 UTC = 21:00 New York (EDT) -> 04:00 on the source grid
    assert_eq!(
        grid_time(1_720_573_200),
        1_720_573_200 - 4 * 3600 + 7 * 3600
    );
}

#[test]
fn compaction_keeps_only_recent_bars_and_the_absolute_bar_count() {
    let mut s = Swing::new(cfg());
    for k in 1..=300 {
        let p = 100.0 + (k % 7) as f64;
        s.close(Tf::H4, bar(k * H4, p, p + 1.0, p - 1.0, p + 0.5), k * H4);
    }
    assert_eq!(s.frame(Tf::H4).len(), 300);
    assert!(s.frame(Tf::H4).stored() <= 128);
    assert!(s.frame(Tf::H4).atr().is_some());
}

#[test]
fn rules_bars_convert_bot_candles_to_the_source_grid_in_order() {
    use forex_core::{engine::Timeframe, trading::Candle};
    let c = |end| Candle {
        end,
        open: 1_100_000,
        high: 1_200_000,
        low: 1_000_000,
        close: 1_150_000,
    };
    let out = rules_bars(&[
        (Timeframe::W, c(1_704_852_000), true),
        (Timeframe::H4, c(1_704_837_600), false),
    ]);
    assert_eq!(out[0].0, Tf::H4);
    assert_eq!(out[1].1.end, 1_704_852_000 + 2 * 3600);
    assert_eq!(out[1].1.close, 1.15);
}
