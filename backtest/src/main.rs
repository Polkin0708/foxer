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
    if args == ["--help"] {
        println!("forex-backtest --strategy FILE --data-config FILE --from UTC --to UTC [--symbols A,B] --output NEW_DIR [--no-pdf]\nforex-backtest --report-only RESULT_DIR");
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
