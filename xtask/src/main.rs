#![forbid(unsafe_code)]

use std::path::Path;
use std::process::{Command, ExitCode};

const PINNED_TOOLCHAIN: &str = "1.98.1";
const HELP: &str = "Uso: cargo xtask <comando>\n\nComandi disponibili:\n  check             Esegue il gate locale completo\n  verify-toolchain  Verifica la toolchain Rust fissata\n";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("verify-toolchain") if args.next().is_none() => match verify_toolchain() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("xtask: errore: {message}");
                ExitCode::FAILURE
            }
        },
        Some("check") if args.next().is_none() => match check() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("xtask: errore: {message}");
                ExitCode::FAILURE
            }
        },
        Some("--help" | "-h") | None => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(command) => {
            eprintln!("xtask: errore: comando non supportato: {command}");
            ExitCode::from(2)
        }
    }
}

fn verify_toolchain() -> Result<(), String> {
    verify_version("rustc")?;
    verify_version("cargo")?;
    println!("toolchain=rustc {PINNED_TOOLCHAIN}");
    Ok(())
}

fn verify_version(program: &str) -> Result<(), String> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|error| format!("impossibile eseguire {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} --version è terminato con {}",
            output.status
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let actual = version.split_whitespace().nth(1).unwrap_or_default();
    if actual != PINNED_TOOLCHAIN {
        return Err(format!(
            "versione {program} non supportata: attesa {PINNED_TOOLCHAIN}, rilevata {actual}"
        ));
    }
    Ok(())
}

fn check() -> Result<(), String> {
    verify_toolchain()?;
    run_step("cargo", &["fmt", "--all", "--", "--check"], false)?;
    run_step(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        false,
    )?;
    run_step(
        "cargo",
        &["test", "--workspace", "--lib", "--bins", "--locked"],
        false,
    )?;
    run_step(
        "cargo",
        &["test", "--doc", "--workspace", "--locked"],
        false,
    )?;
    run_step("cargo", &["build", "--workspace", "--locked"], false)?;
    run_step(
        "cargo",
        &["run", "--package", "sanikey-cli", "--locked", "--", "info"],
        false,
    )?;
    run_step(
        "cargo",
        &[
            "run",
            "--package",
            "sanikey-cli",
            "--locked",
            "--",
            "--version",
        ],
        false,
    )?;
    let python = if cfg!(windows) { "python" } else { "python3" };
    run_step(python, &["scripts/check_publishable.py"], false)?;
    run_step(
        "cargo",
        &["doc", "--workspace", "--no-deps", "--locked"],
        true,
    )?;
    println!("gate Rust e documentale: superato");
    Ok(())
}

fn run_step(program: &str, args: &[&str], rustdoc_warnings: bool) -> Result<(), String> {
    println!("==> {} {}", program, args.join(" "));
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    if rustdoc_warnings {
        command.env("RUSTDOCFLAGS", "-D warnings");
    }
    let status = command
        .status()
        .map_err(|error| format!("impossibile eseguire {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {} è terminato con {status}",
            args.join(" ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::PINNED_TOOLCHAIN;

    #[test]
    fn pinned_toolchain_is_a_full_release_version() {
        let parts: Vec<&str> = PINNED_TOOLCHAIN.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|part| part.parse::<u32>().is_ok()));
    }
}
