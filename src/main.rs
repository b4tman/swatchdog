use std::{sync::mpsc, time::Duration};

use clap::Parser;
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

fn main() {
    let args = Args::parse();

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

    let watchdog = Watchdog::new(
        args.url,
        args.method,
        args.interval,
        args.verbose,
        shutdown_rx,
    );
    watchdog.run();
    println!("bye!");
}
