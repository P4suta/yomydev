use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

#[cfg(target_os = "macos")]
mod camera;
#[cfg(target_os = "macos")]
mod desk;
mod frames;
mod mcp;
mod verdict;

const USAGE: &str = "usage: yomydev <mcp|check|grant>
  mcp    serve the presence tool to an MCP client over stdio
  check  print one presence verdict as JSON
  grant  ask macOS for camera access
";

fn main() -> ExitCode {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let command = match arguments.as_slice() {
        [only] => only.to_str(),
        _ => None,
    };
    match run(command) {
        Ok(code) => code,
        Err(problem) => {
            let _reported = writeln!(io::stderr().lock(), "yomydev: {problem}");
            ExitCode::FAILURE
        },
    }
}

#[cfg(target_os = "macos")]
fn observe() -> verdict::Verdict {
    let desk = desk::observe();
    verdict::Verdict {
        basis: yomydev_core::decide(desk, camera::look),
        idle: desk.idle,
    }
}

#[cfg(target_os = "macos")]
fn run(command: Option<&str>) -> io::Result<ExitCode> {
    match command {
        Some("mcp") => {
            mcp::serve(io::stdin().lock(), io::stdout().lock(), observe)?;
            Ok(ExitCode::SUCCESS)
        },
        Some("check") => {
            writeln!(io::stdout().lock(), "{}", observe().to_json())?;
            Ok(ExitCode::SUCCESS)
        },
        Some("grant") => {
            let access = camera::grant();
            writeln!(io::stdout().lock(), "camera access: {access}")?;
            Ok(if access == camera::Access::Authorized {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        },
        Some(_) | None => {
            io::stderr().lock().write_all(USAGE.as_bytes())?;
            Ok(ExitCode::from(2))
        },
    }
}

#[cfg(not(target_os = "macos"))]
fn run(_command: Option<&str>) -> io::Result<ExitCode> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "yomydev supports macOS only",
    ))
}
