//! Bar building for the swing replay: source-grid H4/D/W bars from MT5 minutes.
use forex_backtest::swing_replay::*;
use forex_core::swing::Tf;

fn minute(t: i64, o: f64, h: f64, l: f64, c: f64) -> Minute {
    Minute {
        time: t,
        open: o,
        high: h,
        low: l,
        close: c,
    }
}
// 2024-01-08 is a Monday; times are naive seconds of the source grid.
const MONDAY: i64 = 1_704_672_000;

#[test]
fn parses_mt5_rows_as_naive_grid_time() {
    let m = parse_row("2024.01.08\t00:01:00\t1.1\t1.3\t1.0\t1.2\t5\t0\t1").unwrap();
    assert_eq!(m.time, MONDAY + 60);
    assert_eq!((m.open, m.high, m.low, m.close), (1.1, 1.3, 1.0, 1.2));
    assert!(parse_row("<DATE>\t<TIME>\t<OPEN>").is_err());
}

#[test]
fn bars_close_when_a_minute_of_the_next_bin_arrives_weekly_before_daily_before_h4() {
    let mut b = Bars::default();
    assert!(b.push(&minute(MONDAY - 120, 1.0, 1.5, 0.9, 1.2)).is_empty());
    assert!(b.push(&minute(MONDAY - 60, 1.2, 1.6, 1.1, 1.4)).is_empty());
    let closed = b.push(&minute(MONDAY, 1.4, 1.45, 1.3, 1.35));
    let order: Vec<Tf> = closed.iter().map(|(tf, _)| *tf).collect();
    assert_eq!(order, [Tf::W, Tf::D, Tf::H4], "Monday 00:00 starts a week");
    for (_, bar) in &closed {
        assert_eq!(bar.end, MONDAY);
        assert_eq!(
            (bar.open, bar.high, bar.low, bar.close),
            (1.0, 1.6, 0.9, 1.4)
        );
    }
    let later = b.push(&minute(MONDAY + 4 * 3600, 1.3, 1.3, 1.3, 1.3));
    assert_eq!(later.len(), 1);
    assert_eq!((later[0].0, later[0].1.end), (Tf::H4, MONDAY + 4 * 3600));
}

#[test]
fn empty_bins_produce_no_bars_and_ends_keep_their_own_grid_time() {
    let mut b = Bars::default();
    b.push(&minute(MONDAY - 3 * 86_400 + 60, 1.0, 1.0, 1.0, 1.0)); // Friday 00:01
    let closed = b.push(&minute(MONDAY + 60, 2.0, 2.0, 2.0, 2.0));
    let ends: Vec<(Tf, i64)> = closed.iter().map(|(tf, bar)| (*tf, bar.end)).collect();
    assert_eq!(
        ends,
        [
            (Tf::H4, MONDAY - 3 * 86_400 + 4 * 3600),
            (Tf::D, MONDAY - 2 * 86_400),
            (Tf::W, MONDAY),
        ],
        "sorted by end time, then W, D, H4"
    );
}

#[test]
fn dollar_index_minutes_exist_only_where_all_six_pairs_have_the_minute() {
    let pairs: Vec<Vec<Minute>> = (0..6)
        .map(|k| {
            (0..3)
                .filter(|&i| !(k == 4 && i == 1))
                .map(|i| minute(MONDAY + 60 * i, 1.0, 1.0, 1.0, 1.0))
                .collect()
        })
        .collect();
    let index = build_dollar_index(&pairs);
    let times: Vec<i64> = index.iter().map(|m| m.time).collect();
    assert_eq!(times, [MONDAY, MONDAY + 120]);
    assert!((index[0].close - 50.14348112).abs() < 1e-9);
}
