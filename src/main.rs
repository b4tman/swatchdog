use std::{sync::mpsc, time::Duration};

use anyhow::Result;
use clap::Parser;
use parse_duration::parse as parse_duration;
use reqwest::Method;

mod watchdog;
use watchdog::{Nothing, Watchdog};

mod logger;
use logger::create_logger;

#[cfg(windows)]
mod serivce;

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
struct Args {
    /// target url
    #[arg(short, long)]
    url: reqwest::Url,

    /// http method
    #[arg(long, default_value = "GET")]
    method: Method,

    /// heartbeats interval
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    interval: Duration,

    /// verbose messages
    #[arg(long, default_value = "false")]
    verbose: bool,

    /// service command ( install | uninstall | start | stop | run )
    /// "run" is used for windows service entrypoint
    #[cfg(windows)]
    #[clap(long)]
    service: Option<serivce::ServiceCommand>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let logger = create_logger(args.verbose)?;

    #[cfg(windows)]
    if args.service.is_some() {
        return serivce::main(
            args.url,
            args.method,
            args.interval,
            args.service.clone().take().unwrap(),
        );
    }

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
