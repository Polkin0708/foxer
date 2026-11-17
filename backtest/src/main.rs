use forex_backtest::runner::{self, Options};
use std::path::PathBuf;
fn main() {
    if let Err(e) = execute() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn execute() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 2 && args[0] == "--report-only" {
        return runner::report(std::path::Path::new(&args[1]));
    }
    if (args.len() == 4 || args.len() == 6) && args[0] == "swing-replay" {
        let p = |k: usize| std::path::Path::new(&args[k]);
        let index = (args.len() == 6).then(|| (p(4), p(5)));
        return forex_backtest::swing_replay::run(p(1), p(2), p(3), index);
    }
    if args.len() == 5 && args[0] == "swing-history" {
        let p = |k: usize| std::path::Path::new(&args[k]);
        return forex_backtest::swing_history::run(p(1), &args[2], p(3), p(4));
    }
    if args.len() == 8 && args[0] == "dxy-build" {
        let inputs: Vec<&std::path::Path> = args[1..7].iter().map(std::path::Path::new).collect();
        return forex_backtest::swing_replay::build(&inputs, std::path::Path::new(&args[7]));
    }
    if args == ["--help"] {
        println!("forex-backtest --strategy FILE --data-config FILE --from UTC --to UTC [--symbols A,B] --output NEW_DIR [--no-pdf]\nforex-backtest --report-only RESULT_DIR\nforex-backtest swing-replay MT5_M1.csv SWING_CONFIG.json NEW_TRADES.csv [INDEX_M1.csv INDEX_CONFIG.json]\nforex-backtest swing-history HISTORY_ROOT SYMBOL SWING_CONFIG.json NEW_TRADES.csv\nforex-backtest dxy-build EURUSD.csv USDJPY.csv GBPUSD.csv USDCAD.csv USDSEK.csv USDCHF.csv NEW_INDEX.csv");
        return Ok(());
    }
    let mut values = std::collections::BTreeMap::new();
    let mut pdf = true;
    let mut i = 0;
    while i < args.len() {
        let k = &args[i];
        if k == "--no-pdf" {
            pdf = false;
            i += 1;
            continue;
        }
        if ![
            "--strategy",
            "--data-config",
            "--from",
            "--to",
            "--symbols",
            "--output",
        ]
        .contains(&k.as_str())
        {
            return Err(format!("unknown argument: {k}"));
        }
        let v = args.get(i + 1).ok_or("missing argument value")?;
        if values.insert(k.as_str(), v.as_str()).is_some() {
            return Err("duplicate argument".into());
        }
        i += 2;
    }
    let required = |k| {
        values
            .get(k)
            .copied()
            .ok_or_else(|| format!("required {k}"))
    };
    runner::run(Options {
        strategy: PathBuf::from(
            values
                .get("--strategy")
                .copied()
                .unwrap_or("strategy/fvg-bot-rules.json"),
        ),
        data_config: PathBuf::from(
            values
                .get("--data-config")
                .copied()
                .unwrap_or("backtest/data-sources.json"),
        ),
        from: runner::date(required("--from")?)?,
        to: runner::date(required("--to")?)?,
        symbols: values
            .get("--symbols")
            .map(|s| s.split(',').map(str::to_owned).collect())
            .unwrap_or_default(),
        output: PathBuf::from(required("--output")?),
        pdf,
    })
}
