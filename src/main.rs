use colored::Colorize;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::{exit, Command};
use thiserror::Error;

const VERSION: &str = "1.0.0";

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
    #[serde(default = "default_interpreter")]
    interpreter: String,
    #[serde(default = "default_work_at")]
    work_at: String,
}

fn default_interpreter() -> String {
    "python".to_string()
}

fn default_work_at() -> String {
    "".to_string()
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

fn run_executable(path: &str, work_at: &str) -> Result<(), SvcError> {
    let mut command = Command::new(path);
    if !work_at.is_empty() {
        command.current_dir(work_at);
    }

    // Run at background
    command.spawn()?;

    println!(
        "Executable {} started in the background.",
        path.cyan(),
    );

    Ok(())
}

fn run_util(path: &str, interpreter: &String, work_at: &str) -> Result<(), SvcError> {
    let mut command = Command::new(interpreter);
    command.arg(path);
    if !work_at.is_empty() {
        command.current_dir(work_at);
    }

    let status = command.status()?;

    if status.success() {
        Ok(())
    } else {
        Err(SvcError::IoError(Error::new(
            ErrorKind::Other,
            format!(
                "Utility {} failed to run with error: {}",
                path.cyan(),
                status.to_string().red()
            ),
        )))
    }
}

fn run_service(service: &Service) -> Result<(), SvcError> {
    if !get_status(service)?.pids.is_empty() {
        return Err(SvcError::ServiceIsRunning);
    }

    let work_at = if service.work_at.is_empty() {
        Path::new(&service.path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_str()
            .unwrap()
            .to_string()
    } else {
        service.work_at.clone()
    };

    match service.service_type {
        ServiceType::Executable => run_executable(&service.path, &work_at),
        ServiceType::Util => run_util(&service.path, &service.interpreter, &work_at),
    }
}

fn enable_service(service: &Service) -> Result<(), SvcError> {
    if get_status(service)?.is_start_up {
        return Err(SvcError::ServiceIsEnabled);
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

    println!("Service {} enabled.", name.cyan());
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

    println!("Service {} disabled.", name.cyan());
    Ok(())
}

struct ServiceStatus {
    pids: Vec<u64>,
    is_start_up: bool,
}

fn get_status(service: &Service) -> Result<ServiceStatus, SvcError> {
    let pids: Vec<u64>;
    let is_start_up: bool;

    {
        let path = &service.path;

        let output = Command::new("powershell")
            .args(&[
                "-Command",
                &format!("Get-WmiObject Win32_Process | Where-Object {{ $_.ExecutablePath -like '*{}*' }} | Select-Object -ExpandProperty ProcessId", path)
            ])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        pids = stdout
            .lines()
            .filter_map(|line| line.trim().parse::<u64>().ok())
            .collect();
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

    Ok(ServiceStatus { pids, is_start_up })
}

fn print_status(service: &Service) -> Result<(), SvcError> {
    let status = get_status(service)?;
    println!("Name: {}", service.name.cyan());
    println!("Type: {}", service.service_type.to_string().cyan());
    println!("Path: {}", service.path.cyan());

    match service.service_type {
        ServiceType::Executable => {
            let pid_str = status
                .pids
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<String>>()
                .join(", ");
            println!(
                "PID: {}",
                if status.pids.is_empty() {
                    "not running".yellow()
                } else {
                    pid_str.as_str().green()
                }
            );

            println!(
                "Start-up: {}",
                if status.is_start_up {
                    "enabled".green()
                } else {
                    "disabled".yellow()
                }
            );
        }

        ServiceType::Util => {
            println!(
                "{}: {}",
                "Interpreter".blue(),
                service.interpreter.yellow()
            )
        }
    }

    Ok(())
}

fn kill_service(service: &Service) -> Result<(), SvcError> {
    let pids = get_status(service)?.pids;

    if pids.is_empty() {
        return Err(SvcError::ServiceIsNotRunning);
    }

    for pid in pids {
        Command::new("taskkill")
            .arg("/F")
            .arg("/PID")
            .arg(pid.to_string())
            .output()?;

        println!(
            "Service {} with PID {} killed.",
            service.name.cyan(),
            pid.to_string().green(),
        );
    }

    Ok(())
}

fn print_help() {
    println!("SVC {VERSION} by EFL, MIT License\nhttps://github.com/EFLKumo/svc\n\nUsage: svc <command> <service_name> \n <command>: \t run \n\t\t enable \n\t\t disable \n\t\t status \n\t\t kill");
}

fn main() -> Result<(), SvcError> {
    let config = load_config("services.yaml")?;

    let args: Vec<String> = std::env::args().collect();

    // svc run xxx at "C:\xxx\dir"
    if args.len() == 5 && args[1] == "run" && args[3] == "at" {
        let service_name = &args[2];
        let work_at = &args[4];

        let service_map: HashMap<String, Service> =
            config.into_iter().map(|s| (s.name.clone(), s)).collect();

        if let Some(service) = service_map.get(service_name) {
            match service.service_type {
                ServiceType::Executable => run_executable(&service.path, work_at),
                ServiceType::Util => run_util(&service.path, &service.interpreter, work_at),
            }?;
            return Ok(());
        } else {
            eprintln!(
                "Service {} not found in the configuration.",
                service_name.cyan(),
            );
            exit(1);
        }
    }

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
                eprintln!("Invalid command {}", command.yellow());
                exit(1);
            }
        }
    } else {
        eprintln!(
            "Service {} not found in the configuration.",
            service_name.cyan(),
        );
        exit(1);
    }
}