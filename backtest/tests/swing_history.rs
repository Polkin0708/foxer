//! Swing replay over prepared minutes through the bot's own bar aggregator.
use forex_backtest::swing_history::*;
use forex_core::{engine::Timeframe, swing::Tf, trading::Candle};

#[test]
fn aggregator_bars_reach_the_rules_in_time_order_on_the_source_grid() {
    let c = |end: i64| Candle {
        end,
        open: 1_100_000,
        high: 1_200_000,
        low: 1_000_000,
        close: 1_150_000,
    };
    // W, D and H4 ending at the same time, plus an earlier H4 after a gap.
    let bars = vec![
        (Timeframe::W, c(1_704_852_000)),
        (Timeframe::D, c(1_704_852_000)),
        (Timeframe::H4, c(1_704_837_600)),
    ];
    let out = to_rules(&bars);
    let order: Vec<Tf> = out.iter().map(|(tf, _)| *tf).collect();
    assert_eq!(order, [Tf::H4, Tf::W, Tf::D]);
    let b = out[1].1;
    assert_eq!(b.end, 1_704_852_000 + 2 * 3600);
    assert_eq!((b.open, b.high, b.low, b.close), (1.1, 1.2, 1.0, 1.15));
}
