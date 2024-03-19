use anyhow::{anyhow, Ok, Result};
use humantime::format_duration;
use parse_duration::parse as parse_duration;
use reqwest::{Method, Url};
use std::{ffi::OsString, path::Path, str::FromStr, sync::mpsc, thread, time::Duration};
use windows_service::{
    define_windows_service,
    service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use winreg::enums::*;
use winreg::RegKey;

use crate::watchdog::{Nothing, Watchdog};

const SERVICE_NAME: &str = env!("CARGO_PKG_NAME");
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_DISPLAY: &str = env!("CARGO_PKG_NAME");
const SERVICE_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

#[derive(Debug)]
struct Config {
    url: Url,
    method: Method,
    interval: Duration,
    _key: RegKey,
}

impl Config {
    fn new(url: Url, method: Method, interval: Duration) -> Result<Self> {
        Ok(Self {
            url,
            method,
            interval,
            _key: Self::reg_key()?,
        })
    }

    fn get() -> Result<Self> {
        let _key = Self::reg_key()?;
        let url: String = _key.get_value("url")?;
        let url: Url = url.parse()?;

        let method: String = _key.get_value("method")?;
        let method: Method = method.parse()?;

        let interval: String = _key.get_value("interval")?;
        let interval: Duration = parse_duration(&interval)?;

        Ok(Self {
            url,
            method,
            interval,
            _key,
        })
    }

    fn save(self) -> Result<()> {
        self._key.set_value("url", &self.url.to_string())?;
        self._key.set_value("method", &self.method.to_string())?;
        self._key
            .set_value("interval", &format_duration(self.interval).to_string())?;
        Ok(())
    }

    fn name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn reg_key() -> Result<RegKey> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let path = Path::new("Software").join(Self::name()).join(Self::name());
        let result = hkcu.create_subkey(path);
        if let Err(e) = &result {
            log::error!("reg_key error: {:#?}", &e);
        }
        let (key, _) = result?;
        Ok(key)
    }
}

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

pub fn main(
    url: reqwest::Url,
    method: reqwest::Method,
    interval: Duration,
    command: ServiceCommand,
) -> Result<()> {
    match command {
        ServiceCommand::Install => install(url, method, interval),
        ServiceCommand::Uninstall => uninstall(),
        ServiceCommand::Run => run(url, method, interval),
        ServiceCommand::Start => start(),
        ServiceCommand::Stop => stop(),
    }
}

trait ServiceStatusEx {
    fn running() -> ServiceStatus;
    fn stopped() -> ServiceStatus;
    fn stopped_with_error(code: u32) -> ServiceStatus;
}

impl ServiceStatusEx for ServiceStatus {
    fn running() -> ServiceStatus {
        ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        }
    }

    fn stopped() -> ServiceStatus {
        ServiceStatus {
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            ..Self::running()
        }
    }

    fn stopped_with_error(code: u32) -> ServiceStatus {
        ServiceStatus {
            exit_code: ServiceExitCode::ServiceSpecific(code),
            ..Self::stopped()
        }
    }
}

pub fn install(url: reqwest::Url, method: reqwest::Method, interval: Duration) -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_binary_path = std::env::current_exe()?;

    let service_info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: OsString::from(SERVICE_DISPLAY),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary_path,
        launch_arguments: vec![
            "--url".into(),
            url.to_string().as_str().into(),
            "--method".into(),
            method.to_string().as_str().into(),
            "--interval".into(),
            format_duration(interval).to_string().as_str().into(),
            "--service".into(),
            "run".into(),
        ],
        dependencies: vec![],
        account_name: Some(OsString::from(r#"NT AUTHORITY\NetworkService"#)),
        account_password: None,
    };
    let service = service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    service.set_description(SERVICE_DESCRIPTION)?;
    log::info!("service installed");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
    let service = service_manager.open_service(SERVICE_NAME, service_access)?;

    let service_status = service.query_status()?;
    if service_status.current_state != ServiceState::Stopped {
        log::warn!("stopping service");
        service.stop()?;
        // Wait for service to stop
        thread::sleep(Duration::from_secs(5));
    }

    service.delete()?;
    log::warn!("service deleted");
    Ok(())
}

pub fn stop() -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP;
    let service = service_manager.open_service(SERVICE_NAME, service_access)?;

    let service_status = service.query_status()?;
    if service_status.current_state != ServiceState::Stopped {
        log::info!("stopping service");
        service.stop()?;
    }
    Ok(())
}

pub fn start() -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::START;
    let service = service_manager.open_service(SERVICE_NAME, service_access)?;

    let service_status = service.query_status()?;
    if service_status.current_state != ServiceState::Running {
        log::info!("start service");
        service.start(Vec::<&str>::new().as_slice())?;
    }
    Ok(())
}

pub fn run(url: reqwest::Url, method: reqwest::Method, interval: Duration) -> Result<()> {
    log::info!("service run");
    let config = Config::new(url, method, interval)?;
    config.save()?;
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;

    Ok(())
}

define_windows_service!(ffi_service_main, my_service_main);

pub fn my_service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        log::error!("error: {}", e);
    }
}

pub fn run_service() -> Result<()> {
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<Nothing>(1);
    let mut shutdown = Some(shutdown_tx);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                log::info!("service stop event received");
                shutdown.take();
                ServiceControlHandlerResult::NoError
            }

            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
    status_handle.set_service_status(ServiceStatus::running())?;

    log::info!("service started");

    let config = Config::get()?;
    let watchdog = Watchdog::new(config.url, config.method, config.interval, shutdown_rx);

    if let Err(e) = watchdog {
        log::error!("error create watchdod: {:#?}", e);
        status_handle.set_service_status(ServiceStatus::stopped_with_error(1))?;
        return Err(e);
    }

    let result = watchdog.unwrap().run();
    if let Err(e) = result {
        log::error!("error run watchdod: {:#?}", e);
        status_handle.set_service_status(ServiceStatus::stopped_with_error(2))?;
        return Err(e);
    }

    status_handle.set_service_status(ServiceStatus::stopped())?;
    log::info!("service stoped");
    Ok(())
}
