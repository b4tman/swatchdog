use std::{fs, path::PathBuf, str::FromStr};

use anyhow::{anyhow, Context, Result};

use crate::args::Args;

use flexi_logger::{
    AdaptiveFormat, Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, LoggerHandle, Naming,
};

#[derive(Debug, Clone, Default)]
pub enum LogConfig {
    #[default]
    Default,
    None,
    Directory(String),
    File(String),
    StdOut,
    StdErr,
}

impl FromStr for LogConfig {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "" => Ok(LogConfig::Default),
            "none" => Ok(LogConfig::None),
            "stdout" => Ok(LogConfig::StdOut),
            "stderr" => Ok(LogConfig::StdErr),
            path => match fs::metadata(path) {
                Ok(meta) => {
                    if meta.is_file() {
                        Ok(LogConfig::File(path.to_string()))
                    } else {
                        Ok(LogConfig::Directory(path.to_string()))
                    }
                }
                _ => Ok(LogConfig::File(path.to_string())),
            },
        }
    }
}

impl From<&LogConfig> for String {
    fn from(value: &LogConfig) -> Self {
        match value {
            LogConfig::Default => "".into(),
            LogConfig::None => "none".into(),
            LogConfig::StdOut => "stdout".into(),
            LogConfig::StdErr => "stderr".into(),
            LogConfig::Directory(x) | LogConfig::File(x) => format!(r#""{}""#, x),
        }
    }
}

fn get_default_log_dir() -> Result<String> {
    let root = PathBuf::from("/");
    let binding = std::env::current_exe().unwrap_or_default();
    let cur_path = binding.parent().unwrap_or(&root);
    if let Ok(md) = fs::metadata(cur_path) {
        let permissions = md.permissions();
        if !permissions.readonly() {
            return Ok(cur_path.to_string_lossy().to_string());
        }
    }

    let cur_path = std::env::current_dir().unwrap_or(PathBuf::from("."));
    if let Ok(md) = fs::metadata(&cur_path) {
        let permissions = md.permissions();
        if !permissions.readonly() {
            return Ok(cur_path.to_string_lossy().to_string());
        }
    }

    Err(anyhow!("can't get default [writable] log dir"))
}

impl LogConfig {
    fn configure(&self, logger: Logger, verbose: bool) -> Result<Logger> {
        let stdout_dup_level = if verbose {
            Duplicate::Info
        } else {
            Duplicate::Warn
        };

        Ok(match self {
            LogConfig::Default => {
                if let Ok(dir) = get_default_log_dir() {
                    LogConfig::Directory(dir)
                        .configure(logger, verbose)?
                        .adaptive_format_for_stdout(AdaptiveFormat::Detailed)
                        .print_message()
                        .duplicate_to_stdout(stdout_dup_level)
                } else {
                    LogConfig::StdOut.configure(logger, verbose)?
                }
            }
            LogConfig::None => logger.do_not_log(),
            LogConfig::Directory(path) => logger
                .log_to_file(FileSpec::default().directory(path))
                .rotate(
                    Criterion::Age(Age::Day),
                    Naming::Timestamps,
                    Cleanup::KeepLogFiles(4),
                )
                .print_message(),
            LogConfig::File(path) => logger
                .log_to_file(FileSpec::try_from(path)?)
                .print_message(),
            LogConfig::StdOut => logger
                .adaptive_format_for_stdout(AdaptiveFormat::Detailed)
                .log_to_stdout(),
            LogConfig::StdErr => logger
                .adaptive_format_for_stderr(AdaptiveFormat::Detailed)
                .log_to_stderr(),
        })
    }
}

pub(crate) fn create_logger(args: &Args) -> Result<LoggerHandle> {
    let cfg = args.log.clone().unwrap_or_default();
    let logger = cfg
        .configure(
            Logger::try_with_str("info")
                .context("default logging level invalid")?
                .format(flexi_logger::detailed_format),
            args.verbose,
        )?
        .write_mode(flexi_logger::WriteMode::Async)
        .start()
        .context("can't start logger");
    log_panics::init();
    logger
}
