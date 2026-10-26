#![forbid(unsafe_code)]

use std::process::ExitCode;

use sanikey_core::version;

const HELP: &str = "Uso: sanikey [--version] [info]\n\nComandi disponibili:\n  info       Mostra le informazioni essenziali del progetto\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match execute(&args) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("sanikey: errore: {message}");
            ExitCode::from(2)
        }
    }
}

fn execute(args: &[String]) -> Result<String, String> {
    match args {
        [] => Ok(HELP.to_owned()),
        [flag] if flag == "--help" || flag == "-h" => Ok(HELP.to_owned()),
        [flag] if flag == "--version" || flag == "-V" => Ok(format!("sanikey {}\n", version())),
        [command] if command == "info" => Ok(format!(
            "progetto=sanikey\npackage=sanikey\ncli=sanikey\nversione={}\n",
            version()
        )),
        [command] => Err(format!("comando non supportato: {command}")),
        [command, ..] if command == "info" => {
            Err("il comando info non accetta argomenti".to_owned())
        }
        [command, ..] => Err(format!("comando non supportato: {command}")),
    }
}

#[cfg(test)]
mod tests {
    use super::execute;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn info_reports_only_available_project_identity() {
        assert_eq!(
            execute(&args(&["info"])),
            Ok("progetto=sanikey\npackage=sanikey\ncli=sanikey\nversione=0.1.0\n".to_owned())
        );
    }

    #[test]
    fn version_flag_reports_the_package_version() {
        assert_eq!(
            execute(&args(&["--version"])),
            Ok("sanikey 0.1.0\n".to_owned())
        );
    }

    #[test]
    fn unimplemented_commands_fail_instead_of_succeeding_as_placeholders() {
        assert_eq!(
            execute(&args(&["build-all"])),
            Err("comando non supportato: build-all".to_owned())
        );
    }
}
