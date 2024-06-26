use std::{net::IpAddr, time::Duration};

#[allow(unused)]
use anyhow::{anyhow, Result};
#[allow(unused)]
use std::str::FromStr;

use clap::Parser;
use humantime::format_duration;
use parse_duration::parse as parse_duration;
use reqwest::Method;

use crate::logger::LogConfig;

#[cfg(windows)]
#[derive(Debug, Clone)]
/// service commands
pub enum ServiceCommand {
    /// install service
    Install,
    /// uninstall service
    Uninstall,
    /// start service
    Start,
    /// stop service
    Stop,
    /// run service (by Windows)
    Run,
}

#[cfg(windows)]
impl FromStr for ServiceCommand {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "install" => Ok(ServiceCommand::Install),
            "uninstall" => Ok(ServiceCommand::Uninstall),
            "start" => Ok(ServiceCommand::Start),
            "stop" => Ok(ServiceCommand::Stop),
            "run" => Ok(ServiceCommand::Run),
            _ => Err(anyhow!("unknown service command")),
        }
    }
}

#[cfg(windows)]
impl From<&ServiceCommand> for String {
    fn from(value: &ServiceCommand) -> Self {
        match value {
            ServiceCommand::Install => "install",
            ServiceCommand::Uninstall => "uninstall",
            ServiceCommand::Start => "start",
            ServiceCommand::Stop => "stop",
            ServiceCommand::Run => "run",
        }
        .into()
    }
}

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
pub struct Args {
    /// target url
    #[arg(short, long)]
    pub url: reqwest::Url,

    /// http method
    #[arg(long, default_value = "GET")]
    pub method: Method,

    /// heartbeats interval
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    pub interval: Duration,

    /// ignore certificate errors
    #[arg(short = 'k', long, default_value = "false")]
    pub insecure: bool,

    /// optional local ip ("0.0.0.0" for ipv4, "::" for ipv6)
    #[arg(short = 's', long = "from")]
    pub local_address: Option<IpAddr>,

    /// verbose messages
    #[arg(long, default_value = "false")]
    pub verbose: bool,

    /// optional log variant (none | stdout | stderr | file | dir )
    /// default is dir, one of (current_exe, current_dir) + stdout,
    /// if writable dir found, or just stdout
    #[clap(long)]
    pub log: Option<LogConfig>,

    /// service command ( install | uninstall | start | stop | run )
    /// "run" is used for windows service entrypoint
    #[cfg(windows)]
    #[clap(long)]
    pub service: Option<ServiceCommand>,
}

impl Args {
    #[allow(unused)]
    pub fn render(&self) -> Vec<String> {
        let mut result = vec![];
        result.push("--url".into());
        result.push(self.url.to_string());

        if self.method != "GET" {
            result.push("--method".into());
            result.push(self.method.to_string());
        }

        if self.interval != parse_duration("60s").unwrap() {
            result.push("--interval".into());
            result.push(format_duration(self.interval).to_string());
        }

        if self.insecure {
            result.push("--insecure".into());
        }

        if self.local_address.is_some() {
            result.push("--from".into());
            result.push(self.local_address.as_ref().unwrap().to_string());
        }

        if self.verbose {
            result.push("--verbose".into());
        }

        if self.log.is_some() {
            result.push("--log".into());
            result.push(self.log.as_ref().unwrap().into());
        }

        #[cfg(windows)]
        if self.service.is_some() {
            let service = self.service.as_ref().unwrap();
            result.push("--service".into());
            result.push(service.into());
        }

        result
    }
}
