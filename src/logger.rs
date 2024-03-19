use anyhow::{Context, Result};

use flexi_logger::{
    AdaptiveFormat, Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, LoggerHandle, Naming,
};

pub(crate) fn create_logger(verbose: bool) -> Result<LoggerHandle> {
    let stdout_level = if verbose {
        Duplicate::Info
    } else {
        Duplicate::Warn
    };

    let logger = Logger::try_with_str("info")
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
        .context("can't start logger");
    log_panics::init();
    logger
}
