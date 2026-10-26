#![forbid(unsafe_code)]

use std::process::{Command, ExitCode};

const PINNED_TOOLCHAIN: &str = "1.98.1";
const HELP: &str = "Uso: cargo xtask <comando>\n\nComandi disponibili:\n  verify-toolchain  Verifica la toolchain Rust fissata\n";

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
