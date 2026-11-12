use std::{env, path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if (args.len() == 2 && (args[0] == "check-service" || args[0] == "probe" || args[0] == "run"))
        || (args.len() == 3 && args[0] == "smoke-test")
    {
        let result = if args[0] == "check-service" {
            forex_bot::service::check(Path::new(&args[1]))
        } else if args[0] == "probe" {
            forex_bot::service::probe(Path::new(&args[1]))
        } else if args[0] == "run" {
            forex_bot::service::run(Path::new(&args[1]))
        } else {
            forex_bot::service::smoke(Path::new(&args[1]), Path::new(&args[2]))
        };
        return match result {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 2 && args[0] == "probe-upscale" {
        return match forex_bot::upscale::probe(Path::new(&args[1])) {
            Ok(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 3 && args[0] == "prepare-state" {
        return match forex_bot::service::prepare_state(Path::new(&args[1]), Path::new(&args[2])) {
            Ok(v) => {
                println!("{v}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 2 && args[0] == "upgrade-db" {
        if !Path::new(&args[1]).is_file() {
            eprintln!("existing database required");
            return ExitCode::from(1);
        }
        return match forex_bot::data::open(Path::new(&args[1])) {
            Ok(_) => {
                println!("Existing database schema updated.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 4 && args[0] == "import-history" {
        return match forex_bot::data::import_history_symbol(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].to_str().unwrap_or(""),
        ) {
            Ok(v) => {
                println!("{v}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 3 && (args[0] == "import-history" || args[0] == "inspect-db") {
        let result = if args[0] == "import-history" {
            forex_bot::data::import_history(Path::new(&args[1]), Path::new(&args[2]))
        } else {
            forex_bot::load(Path::new("strategy/fvg-bot-rules.json")).and_then(|strategy| {
                let time = args[2].to_str().ok_or("invalid UTC argument")?;
                forex_bot::data::inspect(Path::new(&args[1]), time, &strategy)
            })
        };
        return match result {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        };
    }
    if args.is_empty()
        || (args[0] != "check-config" && args[0] != "describe-config")
        || args.len() > 2
    {
        eprintln!("usage: forex-bot <check-config|describe-config> [path]");
        return ExitCode::from(2);
    }
    let path = args
        .get(1)
        .map(Path::new)
        .unwrap_or_else(|| Path::new("strategy/fvg-bot-rules.json"));
    match forex_bot::load(path) {
        Ok(strategy) => {
            if args[0] == "describe-config" {
                match forex_bot::contract::describe(&strategy).and_then(|c| {
                    serde_json::to_string_pretty(&c)
                        .map_err(|_| "structure: contract serialization".to_owned())
                }) {
                    Ok(report) => println!("{report}"),
                    Err(error) => {
                        eprintln!("{error}");
                        return ExitCode::from(1);
                    }
                }
            } else {
                println!("Configuration contract OK.");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
