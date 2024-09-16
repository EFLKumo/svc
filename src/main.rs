use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::{Error, ErrorKind};
use std::process::{exit, Command};
use std::str::FromStr;
use thiserror::Error;

const VERSION: &str = "0.2.0";

#[derive(Error, Debug)]
pub enum SvcError {
    #[error("Service is already running.")]
    ServiceIsRunning,
    #[error("Service is not running.")]
    ServiceIsNotRunning,
    #[error("Service has been disabled")]
    ServiceIsDisabled,
    #[error("Service has been enabled")]
    ServiceIsEnabled,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Yaml error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Cannot read PID")]
    CannotReadPID,
    #[error("Failed to parse PID")]
    FailedToParsePID,
    #[error("Failed to convert string from Utf8")]
    FailedToConvertUtf8(#[from] std::string::FromUtf8Error),
}

// YAML config file structure, use serde for (de)serializing
#[derive(Debug, Deserialize)]
struct Service {
    name: String,
    path: String,
    #[serde(rename = "type")]
    service_type: ServiceType,
}

#[derive(Debug, Deserialize)]
enum ServiceType {
    Executable,
    Util,
}

impl Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            ServiceType::Executable => "Executable".to_string(),
            ServiceType::Util => "Utility".to_string(),
        };
        write!(f, "{}", str)
    }
}

fn load_config(path: &str) -> Result<Vec<Service>, SvcError> {
    let content = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&content)?)
}

fn run_executable(path: &str) -> Result<(), SvcError> {
    // Run at background
    Command::new(path).spawn()?;

    println!("Executable at `{path}` started in the background.");
    Ok(())
}

fn run_util(path: &str) -> Result<(), SvcError> {
    let status = Command::new("python") // TODO: support other interpreters
        .arg(path)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(SvcError::IoError(Error::new(
            ErrorKind::Other,
            format!("Utility `{path}` failed to run with error: {status}",),
        )))
    }
}

fn run_service(service: &Service) -> Result<(), SvcError> {
    if !get_status(service)?.pids.is_empty() {
        return Err(SvcError::ServiceIsRunning)
    }

    match service.service_type {
        ServiceType::Executable => run_executable(&service.path),
        ServiceType::Util => run_util(&service.path),
    }
}

fn enable_service(service: &Service) -> Result<(), SvcError> {
    if get_status(service)?.is_start_up {
        return Err(SvcError::ServiceIsEnabled)
    }

    let path = &service.path;
    let name = &service.name;

    Command::new("reg")
        .arg("add")
        .arg("\"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run")
        .arg("/v")
        .arg(name)
        .arg("/t")
        .arg("REG_SZ")
        .arg("/d")
        .arg(format!("\"{path}\""))
        .arg("/f")
        .status()?;

    Ok(())
}

fn disable_service(service: &Service) -> Result<(), SvcError> {
    if !get_status(service)?.is_start_up {
        return Err(SvcError::ServiceIsDisabled);
    }

    let name = &service.name;

    Command::new("reg")
        .arg("delete")
        .arg("\"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run")
        .arg("/v")
        .arg(name)
        .arg("/f")
        .status()?;

    Ok(())
}

struct ServiceStatus {
    pids: Vec<u64>,
    is_start_up: bool,
}

fn get_status(service: &Service) -> Result<ServiceStatus, SvcError> {
    let mut pids: Vec<u64> = Vec::new();
    let is_start_up: bool;

    {
        let path = &service.path;

        let output = Command::new("powershell")
            .args(&["-Command", &format!("Get-WmiObject Win32_Process | Where-Object {{ $_.ExecutablePath -like '*{}*' }} | Select-Object -ExpandProperty ProcessId", path)])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let lines: Vec<&str> = stdout.lines().collect();

        for (_, line) in lines.iter().enumerate() {
            if let Ok(pid) = line.trim().parse::<u64>() {
                pids.push(pid);
            }
        }
    }

    {
        let exit_code = Command::new("reg")
            .arg("query")
            .arg("\"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run")
            .arg("/v")
            .arg(&service.name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?
            .code();

        is_start_up = exit_code == Some(0);
    }

    Ok(ServiceStatus{ pids, is_start_up })
}

fn print_status(service: &Service) -> Result<(), SvcError> {
    let status = get_status(service)?;
    println!("Name: {}", service.name);
    println!("Type: {}", service.service_type.to_string());
    println!("Path: {}", service.path);

    let pid_str = status.pids.iter().map(|n| n.to_string()).collect::<Vec<String>>().join(", ");
    println!("PID: {}", {
        if status.pids.is_empty() {
            "not running"
        } else {
            pid_str.as_str()
        }
    });

    println!("Start up: {}", if status.is_start_up { "enabled" } else { "disabled" });
    Ok(())
}

fn kill_service(service: &Service) -> Result<(), SvcError> {
    if get_status(service)?.pids.is_empty() {
        return Err(SvcError::ServiceIsNotRunning);
    }

    let output = Command::new("wmic")
        .arg("process")
        .arg("where")
        .arg(format!("ExecutablePath='{}'", service.path))
        .arg("get")
        .arg("ProcessId")
        .output()?;

    let output_str = String::from_utf8(output.stdout)?;
    let pid_str = output_str
        .lines()
        .find(|line| line.trim().starts_with("ProcessId"))
        .and_then(|line| line.split_whitespace().last())
        .ok_or(SvcError::CannotReadPID)?;
    let pid: u64 = u64::from_str(pid_str).map_err(|_| SvcError::FailedToParsePID)?;

    Command::new("taskkill")
        .arg("/F")
        .arg("/PID")
        .arg(pid.to_string())
        .output()?;

    Ok(())
}

fn print_help() {
    println!("SVC {VERSION} by EFL, MIT License\nhttps://github.com/EFLKumo/svc\n\nUsage: svc <command> <service_name> \n <command>: \t enable \n\t\t disable \n\t\t status \n\t\t kill");
}

fn main() -> Result<(), SvcError> {
    let config = load_config("services.yaml")?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        print_help();
        exit(1);
    }

    let command = &args[1];
    let service_name = &args[2];

    let service_map: HashMap<String, Service> =
        config.into_iter().map(|s| (s.name.clone(), s)).collect();

    if let Some(service) = service_map.get(service_name) {
        match command.as_str() {
            "run" => run_service(service),
            "enable" => enable_service(service),
            "disable" => disable_service(service),
            "status" => print_status(service),
            "kill" => kill_service(service),
            _ => {
                eprintln!("Invalid command `{}`", command);
                exit(1);
            }
        }
    } else {
        eprintln!("Service `{service_name}` not found in the configuration.");
        exit(1);
    }
}
