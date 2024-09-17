use colored::Colorize;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::{exit, Command, Stdio};
use thiserror::Error;
use rayon::prelude::*; // For parallel iterators

const VERSION: &str = "1.0.1";

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
struct Service<'a> {
    name: Cow<'a, str>,
    path: Cow<'a, str>,
    #[serde(rename = "type")]
    service_type: ServiceType,
    #[serde(default = "default_interpreter")]
    interpreter: Cow<'a, str>,
    #[serde(default = "default_work_at")]
    work_at: Cow<'a, str>,
}

fn default_interpreter() -> Cow<'static, str> {
    Cow::Borrowed("python")
}

fn default_work_at() -> Cow<'static, str> {
    Cow::Borrowed("")
}

#[derive(Debug, Deserialize)]
enum ServiceType {
    Executable,
    Util,
}

impl Display for ServiceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            ServiceType::Executable => "Executable",
            ServiceType::Util => "Utility",
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

    command.spawn()?; // Run in background
    println!("Executable {} started in the background.", path.cyan());
    Ok(())
}

fn run_util(path: &str, interpreter: &str, work_at: &str) -> Result<(), SvcError> {
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
        Path::new(service.path.as_ref())
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_str()
            .unwrap_or(".")
    } else {
        &*service.work_at
    };

    match service.service_type {
        ServiceType::Executable => run_executable(&service.path, work_at),
        ServiceType::Util => run_util(&service.path, &service.interpreter, work_at),
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
        .arg(r#"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"#)
        .arg("/v")
        .arg(name.as_ref())
        .arg("/t")
        .arg("REG_SZ")
        .arg("/d")
        .arg(path.as_ref())
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
        .arg(r#"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"#)
        .arg("/v")
        .arg(name.as_ref())
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
    let pids: Vec<u64> = {
        let output = Command::new("powershell")
            .args(&[
                "-Command",
                &format!(
                    r#"Get-WmiObject Win32_Process | Where-Object {{ $_.ExecutablePath -like '*{}*' }} | Select-Object -ExpandProperty ProcessId"#,
                    service.path
                ),
            ])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| line.trim().parse::<u64>().ok())
            .collect()
    };

    let is_start_up = {
        let exit_code = Command::new("reg")
            .arg("query")
            .arg(r#"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run"#)
            .arg("/v")
            .arg(service.name.as_ref())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?
            .code();

        exit_code == Some(0)
    };

    Ok(ServiceStatus { pids, is_start_up })
}

fn print_status(service: &Service) -> Result<(), SvcError> {
    let status = get_status(service)?;
    println!("Name: {}", service.name.cyan());
    println!("Type: {}", service.service_type.to_string().cyan());
    println!("Path: {}", service.path.cyan());

    match service.service_type {
        ServiceType::Executable => {
            let pid_str = if status.pids.is_empty() {
                "not running".yellow().to_string()
            } else {
                status.pids.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ").green().to_string()
            };
            println!("PID: {}", pid_str);
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
            println!("Interpreter: {}", service.interpreter.cyan());
        }
    }

    Ok(())
}

fn kill_service(service: &Service) -> Result<(), SvcError> {
    let pids = get_status(service)?.pids;

    if pids.is_empty() {
        return Err(SvcError::ServiceIsNotRunning);
    }

    // Parallelize killing of PIDs
    pids.par_iter().for_each(|&pid| {
        let _ = Command::new("taskkill")
            .arg("/F")
            .arg("/PID")
            .arg(pid.to_string())
            .output();

        println!(
            "Service {} with PID {} killed.",
            service.name.cyan(),
            pid.to_string().green()
        );
    });

    Ok(())
}

fn print_help() {
    println!(
        "SVC {VERSION} by EFL, MIT License\nhttps://github.com/EFLKumo/svc\n\nUsage: svc <command> <service_name>\n\
        <command>: \t run \n\t\t enable \n\t\t disable \n\t\t status \n\t\t kill"
    );
}

fn main() -> Result<(), SvcError> {
    let config_path = format!(
        "{}\\services.yaml",
        std::env::current_exe()?.parent().unwrap().to_str().unwrap()
    );
    let config = load_config(&config_path)?;

    let args: Vec<String> = std::env::args().collect();

    if args.len() == 5 && args[1] == "run" && args[3] == "at" {
        let service_name = &args[2];
        let work_at = &args[4];

        let service_map: HashMap<&str, &Service> = config.iter().map(|s| (&*s.name, s)).collect();

        if let Some(service) = service_map.get(service_name.as_str()) {
            match service.service_type {
                ServiceType::Executable => run_executable(&service.path, work_at),
                ServiceType::Util => run_util(&service.path, &service.interpreter, work_at),
            }?;
            return Ok(());
        } else {
            println!(
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

    let service_map: HashMap<&str, &Service> = config.iter().map(|s| (&*s.name, s)).collect();

    if let Some(service) = service_map.get(service_name.as_str()) {
        match command.as_str() {
            "run" => run_service(service),
            "enable" => enable_service(service),
            "disable" => disable_service(service),
            "status" => print_status(service),
            "kill" => kill_service(service),
            _ => {
                println!("Invalid command {}", command.yellow());
                exit(1);
            }
        }
    } else {
        println!(
            "Service {} not found in the configuration.",
            service_name.cyan(),
        );
        exit(1);
    }
}
