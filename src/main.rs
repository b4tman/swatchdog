use std::{sync::mpsc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use flexi_logger::{
    AdaptiveFormat, Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, LoggerHandle, Naming,
};
use parse_duration::parse as parse_duration;
use reqwest::Method;

mod watchdog;
use watchdog::{Nothing, Watchdog};

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
struct Args {
    #[arg(short, long)]
    url: reqwest::Url,
    #[arg(long, default_value = "GET")]
    method: Method,
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    interval: Duration,
    #[arg(long, default_value = "false")]
    verbose: bool,
}

fn create_logger(verbose: bool) -> Result<LoggerHandle> {
    let stdout_level = if verbose {
        Duplicate::Info
    } else {
        Duplicate::Warn
    };
    Logger::try_with_str("info")
        .context("default logging level invalid")?
        .log_to_file(
            FileSpec::default().directory(
                std::env::current_exe()
                    .context("can't get current exe path")?
                    .parent()
                    .context("can't get parent folder")?,
            ),
        )
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(4),
        )
        .format(flexi_logger::detailed_format)
        .adaptive_format_for_stdout(AdaptiveFormat::Detailed)
        .print_message()
        .duplicate_to_stdout(stdout_level)
        .write_mode(flexi_logger::WriteMode::Async)
        .start_with_specfile(
            std::env::current_exe()
                .context("can't get current exe path")?
                .with_file_name("logspec.toml"),
        )
        .context("can't start logger")
}

fn main() -> Result<()> {
    let args = Args::parse();
    let logger = create_logger(args.verbose)?;
    log_panics::init();

    println!("swatchdog v{} started!", env!("CARGO_PKG_VERSION"));

    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<Nothing>(1);
    let mut shutdown = Some(shutdown_tx);

    let res = ctrlc::set_handler(move || {
        println!("recieved Ctrl-C");
        shutdown.take(); // drop shutdown_tx
    });

    if res.is_ok() {
        println!("Press Ctrl-C to stop");
    }

    let watchdog = Watchdog::new(args.url, args.method, args.interval, shutdown_rx)?;
    watchdog.run()?;

    log::info!("bye!");
    drop(logger);
    Ok(())
}
