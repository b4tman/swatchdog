use anyhow::Result;
mod args;
mod logger;
mod watchdog;
use clap::Parser;
use logger::create_logger;

use crate::watchdog::Watchdog;

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

    let mut watchdog = Watchdog::try_from(args)?;
    let mut shutdown = watchdog.take_shutdown_tx();

    let res = ctrlc::set_handler(move || {
        println!("recieved Ctrl-C");
        shutdown.take(); // drop shutdown_tx
    });

    if res.is_ok() {
        println!("Press Ctrl-C to stop");
    }

    watchdog.run()?;

    log::info!("bye!");
    drop(logger);
    Ok(())
}
