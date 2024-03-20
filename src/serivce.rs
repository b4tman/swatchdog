use crate::args::{self, ServiceCommand};
use crate::watchdog::Watchdog;
use anyhow::{anyhow, Ok, Result};
use std::sync::Mutex;
use std::{ffi::OsString, thread, time::Duration};
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

const SERVICE_NAME: &str = env!("CARGO_PKG_NAME");
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_DISPLAY: &str = env!("CARGO_PKG_NAME");
const SERVICE_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

// for pass ImagePath args to ffi_service_main
static RUN_ARGS: Mutex<Option<args::Args>> = Mutex::new(None);

pub fn main(args: args::Args) -> Result<()> {
    match args.service.as_ref().unwrap() {
        ServiceCommand::Install => install(args),
        ServiceCommand::Uninstall => uninstall(),
        ServiceCommand::Run => run(args),
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

pub fn install(args: args::Args) -> Result<()> {
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    let service_binary_path = std::env::current_exe()?;

    let mut args = args;
    args.service = Some(ServiceCommand::Run);

    let service_info = ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: OsString::from(SERVICE_DISPLAY),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary_path,
        launch_arguments: args.render().iter().map(|x| x.into()).collect(),
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

pub fn run(args: args::Args) -> Result<()> {
    log::info!("service run");
    RUN_ARGS
        .lock()
        .map_err(|e| anyhow!("lock args for set error: {}", e))?
        .replace(args);
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
    log::info!("service started");

    let args = RUN_ARGS
        .lock()
        .map_err(|e| anyhow!("lock args for get error: {}", e))?
        .take()
        .ok_or(anyhow!("no args in run_service"))?;

    let watchdog = Watchdog::try_from(args);
    if let Err(e) = watchdog {
        log::error!("error create watchdod: {:#?}", e);
        let status_handle = service_control_handler::register(SERVICE_NAME, move |_| {
            ServiceControlHandlerResult::NotImplemented
        })?;
        status_handle.set_service_status(ServiceStatus::stopped_with_error(1))?;
        return Err(e);
    }
    let mut watchdog = watchdog.unwrap();
    let mut shutdown = watchdog.take_shutdown_tx();
    let watchdog = watchdog;

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

    let result = watchdog.run();
    if let Err(e) = result {
        log::error!("error run watchdod: {:#?}", e);
        status_handle.set_service_status(ServiceStatus::stopped_with_error(2))?;
        return Err(e);
    }

    status_handle.set_service_status(ServiceStatus::stopped())?;
    log::info!("service stoped");

    Ok(())
}
