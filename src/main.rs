use anyhow::Result;
mod args;
mod logger;
mod watchdog;
use clap::Parser;
use logger::create_logger;

use crate::watchdog::create_shutdown_chanel;

#[cfg(windows)]
mod serivce;

fn main() -> Result<()> {
    let args = args::Args::parse();
    let logger = create_logger(args.verbose)?;

    #[cfg(windows)]
    if args.service.is_some() {
        return serivce::main(args);
    }

    println!("swatchdog v{} started!", env!("CARGO_PKG_VERSION"));

    let (shutdown_tx, shutdown_rx) = create_shutdown_chanel();
    let mut shutdown = Some(shutdown_tx);

    let res = ctrlc::set_handler(move || {
        println!("recieved Ctrl-C");
        shutdown.take(); // drop shutdown_tx
    });

    if res.is_ok() {
        println!("Press Ctrl-C to stop");
    }

    let watchdog = args.create_watchdog(shutdown_rx)?;
    watchdog.run()?;

    log::info!("bye!");
    drop(logger);
    Ok(())
}
