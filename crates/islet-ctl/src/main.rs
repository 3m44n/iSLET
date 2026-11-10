//! `islet-ctl` — CLI client for controlling the Islet Wayland overlay daemon via `$XDG_RUNTIME_DIR/islet.sock`.
//!
//! Examples:
//!   islet-ctl show volume 70
//!   islet-ctl show brightness 80
//!   islet-ctl toggle
//!   islet-ctl notify "Title" "Body"

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use islet_core::{socket_path, IpcCommand};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = parse_cli_args(&args) else {
        print_usage();
        return ExitCode::from(2);
    };

    let sock = socket_path();
    let mut stream = match UnixStream::connect(&sock) {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "islet-ctl: could not connect to Islet daemon at {}: {err}",
                sock.display()
            );
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = stream.write_all(cmd.to_wire_line().as_bytes()) {
        eprintln!("islet-ctl: failed to send command: {err}");
        return ExitCode::FAILURE;
    }

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    if reader.read_line(&mut response).is_ok() && response.starts_with("OK") {
        ExitCode::SUCCESS
    } else {
        eprintln!("islet-ctl: daemon returned {}", response.trim());
        ExitCode::FAILURE
    }
}

fn parse_cli_args(args: &[String]) -> Option<IpcCommand> {
    match args.first()?.as_str() {
        "toggle" => Some(IpcCommand::Toggle),
        "ping" => Some(IpcCommand::Ping),
        "show" => {
            let kind = args.get(1)?.as_str();
            let val: u8 = args.get(2)?.parse().ok()?;
            match kind {
                "volume" | "vol" => Some(IpcCommand::ShowVolume(val.min(150))),
                "brightness" | "bri" => Some(IpcCommand::ShowBrightness(val.min(100))),
                "battery" | "bat" => {
                    let charging = args.get(3).is_some_and(|s| {
                        s.eq_ignore_ascii_case("charging") || s == "1" || s.eq_ignore_ascii_case("true")
                    });
                    Some(IpcCommand::ShowBattery { percent: val.min(100), charging })
                }
                _ => None,
            }
        }
        "notify" => {
            let title = args.get(1)?.clone();
            let body = args.get(2).cloned().unwrap_or_default();
            Some(IpcCommand::Notify { title, body })
        }
        _ => None,
    }
}

fn print_usage() {
    eprintln!(
        "Usage:
           islet-ctl show volume <0..150>
           islet-ctl show brightness <0..100>
           islet-ctl show battery <0..100> [charging]
           islet-ctl toggle
           islet-ctl notify <Title> [Body]
           islet-ctl ping"
    );
}
