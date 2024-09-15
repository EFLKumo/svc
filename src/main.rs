use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind};
use std::process::{exit, Command};
use std::str::FromStr;
use thiserror::Error;


const VERSION: &str = "0.1.0";

#[derive(Error, Debug)]
pub enum SvcError {
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


// YAML config file structure,
// use serde for (de)serializing
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
    match service.service_type {
        ServiceType::Executable => run_executable(&service.path),
        ServiceType::Util => run_util(&service.path),
    }
}

fn enable_service(service: &Service) -> Result<(), SvcError> {
    let path = &service.path;
    let name = &service.name;

    Command::new("reg")
        .arg("add")
        .arg("\"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\"")
        .arg("/v")
        .arg(format!("\"{name}\""))
        .arg("/t")
        .arg("REG_SZ")
        .arg("/d")
        .arg(format!("\"{path}\""))
        .arg("/f")
        .status()?;

    Ok(())
}

fn disable_service(service: &Service) -> Result<(), SvcError> {
    let name = &service.name;

    Command::new("reg")
        .arg("delete")
        .arg("\"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\"")
        .arg("/v")
        .arg(format!("\"{name}\""))
        .arg("/f")
        .status()?;

    Ok(())
}

fn status_service(service: &Service) -> Result<(), SvcError> {
    // TODO: more detail
    // wmic process where "ExecutablePath like '%C:\\Program Files\\...%'" get ProcessId, Name , ExecutablePath
    let output = Command::new("wmic")
        .args(&[
            "process",
            "where",
            format!("ExecutablePath like '%{}%'", service.path.replace("\\", "\\\\")).as_str(),
            "get",
            "ExecutablePath",
        ])
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    if output_str.contains(&service.path) {
        println!("Service `{}` is running (process: {}).", service.name, service.path);
        Ok(())
    } else {
        println!("Service `{}` is not running.", service.name);
        Ok(())
    }
}

// 终止服务
fn kill_service(service: &Service) -> Result<(), SvcError> {
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
    let pid: u32 = u32::from_str(pid_str).map_err(|_| SvcError::FailedToParsePID)?;

    Command::new("taskkill")
        .arg("/F")
        .arg("/PID")
        .arg(pid.to_string())
        .output()?;

    Ok(())
}

fn print_help() {
    eprintln!("Service {VERSION} by EFL\nUsage: svc <command> <service_name> \n <command>: \t enable \n\t\t disable \n\t\t status \n\t\t kill");
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

    let service_map: HashMap<String, Service> = config.into_iter()
        .map(|s| (s.name.clone(), s))
        .collect();

    if let Some(service) = service_map.get(service_name) {
        match command.as_str() {
            "run" => run_service(service),
            "enable" => enable_service(service),
            "disable" => disable_service(service),
            "status" => status_service(service),
            "kill" => kill_service(service),
            _ => {
                eprintln!("Invalid command `{}`", command);
                exit(1);
            }
        }
    } else {
        eprintln!("Service `{}` not found in the configuration.", service_name);
        exit(1);
    }
}