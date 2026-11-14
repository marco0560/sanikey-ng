//! Comandi CLI per le build locali SaniKey.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sanikey_core::build::{BuildMode, BuildOptions, PatientBuildResult, build_all, build_patient};
use sanikey_core::config::{default_accounts_path, load_accounts};
use sanikey_core::logging::{
    Activity, Component, FailureCategory, Level, Outcome, Session, TraceField, TraceOmission,
    TraceValue, redact_secret_assignments, trace_current,
};
use sanikey_core::operation::OperationOutcome;
use sanikey_core::privacy::validate_runtime_privacy;
use sanikey_core::{project_info, version};

const HELP: &str = "Uso: sanikey [opzioni-log] [--version] [info|build-patient|build-all] [opzioni]\n\nComandi disponibili:\n  info                         Mostra le informazioni essenziali del progetto\n  build-patient <persona>      Costruisce gli artefatti locali di una persona\n  build-all [config.toml]      Costruisce gli artefatti locali di tutte le persone abilitate\n\nOpzioni log per il solo processo, disattivate per default:\n  --log-level <livello>        events, diagnostic o trace\n  --log-dir <directory>        Directory log (default: directory locale SaniKey)\n  --confirm-sensitive-logging  Consenso esplicito richiesto per diagnostic e trace\n\nDiagnostic e trace possono includere dati sanitari o privati. La traccia delle\nchiamate e dei dati è parziale; gli elementi non strumentati non vengono registrati.\n\nOpzioni build:\n  -c, --config <file>          File account per build-patient\n  -m, --mode <modalita>        full, incremental (default) o validation\n  -O, --force-pdf-ocr          Forza una nuova estrazione OCR dei PDF\n  -q, --no-progress            Sopprime la nota sull'avanzamento non disponibile\n  -r, --repo-root <directory>  Root per controlli privacy (default: directory corrente)\n\nQuesta versione non riceve eventi di avanzamento dettagliato dal core; senza -q\nlo segnala su stderr prima della build.\n";

const BUILD_PATIENT_HELP: &str = "Uso: sanikey build-patient <persona> [-c|--config <file>] [-m|--mode full|incremental|validation] [-O|--force-pdf-ocr] [-q|--no-progress] [-r|--repo-root <directory>]\n\nEsegue la pipeline di build locale per una sola persona abilitata.\n";

const BUILD_ALL_HELP: &str = "Uso: sanikey build-all [config.toml] [-m|--mode full|incremental|validation] [-O|--force-pdf-ocr] [-q|--no-progress] [-r|--repo-root <directory>]\n\nEsegue la pipeline di build locale per tutte le persone abilitate.\n";

#[derive(Debug, Eq, PartialEq)]
struct CommandResult {
    stdout: String,
    stderr: String,
    status: u8,
}

/// Costruisce le risposte testuali e lo status del comando CLI.
impl CommandResult {
    fn success(stdout: String, stderr: String) -> Self {
        let stdout_len = stdout.len() as u64;
        let stderr_len = stderr.len() as u64;
        trace_cli_call(
            "cli::CommandResult::success",
            || {
                vec![
                    TraceField::new("stdout_bytes", TraceValue::data(stdout_len)),
                    TraceField::new("stderr_bytes", TraceValue::data(stderr_len)),
                ]
            },
            || Self {
                stdout,
                stderr,
                status: 0,
            },
            |result| TraceValue::data(u64::from(result.status)),
        )
    }

    fn usage_error(message: impl AsRef<str>) -> Self {
        trace_cli_call(
            "cli::CommandResult::usage_error",
            || {
                vec![TraceField::new(
                    "message",
                    TraceValue::data(redact_secret_assignments(message.as_ref())),
                )]
            },
            || Self {
                stdout: String::new(),
                stderr: format!("sanikey: errore: {}\n", message.as_ref()),
                status: 2,
            },
            |result| TraceValue::data(u64::from(result.status)),
        )
    }

    fn operation_error(message: impl AsRef<str>) -> Self {
        trace_cli_call(
            "cli::CommandResult::operation_error",
            || {
                vec![TraceField::new(
                    "message",
                    TraceValue::data(redact_secret_assignments(message.as_ref())),
                )]
            },
            || Self {
                stdout: format!("ERRORE: {}\n", message.as_ref()),
                stderr: String::new(),
                status: 1,
            },
            |result| TraceValue::data(u64::from(result.status)),
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BuildArguments {
    mode: BuildMode,
    force_pdf_ocr: bool,
    no_progress: bool,
    repo_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LogArguments {
    level: Option<Level>,
    directory: Option<PathBuf>,
    confirmed_sensitive: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = execute_with_progress_notice(&args, |notice| eprint!("{notice}"));
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    ExitCode::from(result.status)
}

#[cfg(test)]
fn execute(args: &[String]) -> CommandResult {
    let mut progress_messages = String::new();
    let mut result =
        execute_with_progress_notice(args, |notice| progress_messages.push_str(notice));
    progress_messages.push_str(&result.stderr);
    result.stderr = progress_messages;
    result
}

fn execute_with_progress_notice(
    args: &[String],
    mut report_progress_notice: impl FnMut(&str),
) -> CommandResult {
    // Il parsing delle opzioni log e l'apertura della sessione sono bootstrap:
    // nessun contesto Session è attivo finché Session non è stata creata.
    let (log_args, command_args) = match parse_log_arguments(args) {
        Ok(parsed) => parsed,
        Err(error) => return CommandResult::usage_error(error),
    };
    let Some(level) = log_args.level else {
        return execute_command(command_args, &mut report_progress_notice);
    };

    let directory = match log_args.directory {
        Some(directory) => Ok(directory),
        None => sanikey_core::logging::default_log_directory(),
    };
    let directory = match directory {
        Ok(directory) => directory,
        Err(error) => {
            let mut result = execute_command(command_args, &mut report_progress_notice);
            result.stderr.push_str(&format!(
                "sanikey: registrazione non avviata; log incompleto: {error}\n"
            ));
            return result;
        }
    };
    let (session, path) =
        match Session::create_in_directory_with_level(&directory, Component::Cli, level) {
            Ok(created) => created,
            Err(error) => {
                let mut result = execute_command(command_args, &mut report_progress_notice);
                result.stderr.push_str(&format!(
                    "sanikey: registrazione non avviata; log incompleto: {error}\n"
                ));
                return result;
            }
        };
    let activity = match command_args.first().map(String::as_str) {
        Some("info") => Activity::CliInfo,
        Some("build-patient") => Activity::CliBuildPatient,
        Some("build-all") => Activity::CliBuildAll,
        Some("--version" | "-V") => Activity::CliVersion,
        Some("--help" | "-h") | None => Activity::CliHelp,
        _ => Activity::Command,
    };
    let operation = session.start_operation(Component::Cli, activity);
    let mut result = match &operation {
        Ok(operation) => {
            operation.scoped(|| execute_command(command_args, &mut report_progress_notice))
        }
        Err(_) => session.scoped(|| execute_command(command_args, &mut report_progress_notice)),
    };
    match operation {
        Ok(operation) => {
            let outcome = match result.status {
                0 => Outcome::Succeeded,
                2 => Outcome::FailedWith(FailureCategory::Usage),
                _ => Outcome::FailedWith(FailureCategory::Other),
            };
            if let Err(error) = operation.finish(outcome) {
                result.stderr.push_str(&format!(
                    "sanikey: errore di scrittura del log; log incompleto: {error}\n"
                ));
            }
        }
        Err(error) => result.stderr.push_str(&format!(
            "sanikey: errore di scrittura del log; log incompleto: {error}\n"
        )),
    }
    if let Err(error) = session.finish(Component::Cli) {
        result.stderr.push_str(&format!(
            "sanikey: errore di scrittura del log; log incompleto: {error}\n"
        ));
    }
    if session.has_failed() && !result.stderr.contains("log incompleto") {
        result
            .stderr
            .push_str("sanikey: registrazione interrotta; log incompleto\n");
    }
    result
        .stderr
        .push_str(&format!("sanikey: log sessione: {}\n", path.display()));
    #[cfg(not(unix))]
    result.stderr.push_str(
        "sanikey: i permessi del log dipendono dalle ACL ereditate; verificarli prima della condivisione.\n",
    );
    result
}

fn execute_command(
    args: &[String],
    report_progress_notice: &mut impl FnMut(&str),
) -> CommandResult {
    let trace = trace_current(Component::Cli, "cli::execute_command", || {
        vec![
            TraceField::new(
                "args",
                TraceValue::data(
                    args.iter()
                        .map(|argument| redact_secret_assignments(argument))
                        .collect::<Vec<_>>(),
                ),
            ),
            TraceField::new(
                "report_progress_notice",
                TraceValue::Omitted(TraceOmission::UnsupportedType),
            ),
        ]
    });
    let result = match args {
        [] => CommandResult::success(HELP.to_owned(), String::new()),
        [flag] if flag == "--help" || flag == "-h" => {
            CommandResult::success(HELP.to_owned(), String::new())
        }
        [flag] if flag == "--version" || flag == "-V" => {
            CommandResult::success(format!("sanikey {}\n", version()), String::new())
        }
        [command] if command == "info" => match project_info().outcome {
            OperationOutcome::Succeeded(output) => CommandResult::success(output, String::new()),
            OperationOutcome::Failed(error) => CommandResult::operation_error(error.message()),
            OperationOutcome::Cancelled => CommandResult::operation_error("operazione annullata"),
        },
        [command, ..] if command == "build-patient" && has_help_flag(&args[1..]) => {
            CommandResult::success(BUILD_PATIENT_HELP.to_owned(), String::new())
        }
        [command, ..] if command == "build-all" && has_help_flag(&args[1..]) => {
            CommandResult::success(BUILD_ALL_HELP.to_owned(), String::new())
        }
        [command, ..] if command == "build-patient" => {
            execute_build_patient(&args[1..], report_progress_notice)
        }
        [command, ..] if command == "build-all" => {
            execute_build_all(&args[1..], report_progress_notice)
        }
        [command, ..] if command == "info" => {
            CommandResult::usage_error("il comando info non accetta argomenti")
        }
        [command, ..] => CommandResult::usage_error(format!("comando non supportato: {command}")),
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::record(&[
            TraceField::new(
                "stdout",
                TraceValue::data(redact_secret_assignments(&result.stdout)),
            ),
            TraceField::new(
                "stderr",
                TraceValue::data(redact_secret_assignments(&result.stderr)),
            ),
            TraceField::new("status", TraceValue::data(u64::from(result.status))),
        ]));
    }
    result
}

fn trace_cli_call<T>(
    function: &'static str,
    arguments: impl FnOnce() -> Vec<TraceField>,
    action: impl FnOnce() -> T,
    result: impl FnOnce(&T) -> TraceValue,
) -> T {
    let trace = trace_current(Component::Cli, function, arguments);
    let value = action();
    if let Some(trace) = trace {
        let _ = trace.finish(result(&value));
    }
    value
}

fn parse_log_arguments(args: &[String]) -> Result<(LogArguments, &[String]), String> {
    let mut parsed = LogArguments::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--log-level" {
            index += 1;
            parsed.level = Some(parse_log_level(
                args.get(index)
                    .map(String::as_str)
                    .ok_or_else(|| "--log-level: atteso events, diagnostic o trace".to_owned())?,
            )?);
        } else if let Some(value) = argument.strip_prefix("--log-level=") {
            parsed.level = Some(parse_log_level(value)?);
        } else if argument == "--log-dir" {
            index += 1;
            let value = args
                .get(index)
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| "--log-dir: atteso un percorso".to_owned())?;
            parsed.directory = Some(PathBuf::from(value));
        } else if let Some(value) = argument.strip_prefix("--log-dir=") {
            ensure_value(value, "--log-dir")?;
            parsed.directory = Some(PathBuf::from(value));
        } else if argument == "--confirm-sensitive-logging" {
            parsed.confirmed_sensitive = true;
        } else {
            break;
        }
        index += 1;
    }
    if parsed.directory.is_some() && parsed.level.is_none() {
        return Err("--log-dir richiede --log-level".to_owned());
    }
    if parsed.confirmed_sensitive && parsed.level.is_none() {
        return Err("--confirm-sensitive-logging richiede --log-level".to_owned());
    }
    if matches!(parsed.level, Some(Level::Diagnostic | Level::Trace)) && !parsed.confirmed_sensitive
    {
        return Err(
            "Diagnostic e trace possono contenere dati sanitari o privati; aggiungere --confirm-sensitive-logging per acconsentire".to_owned(),
        );
    }
    Ok((parsed, &args[index..]))
}

fn parse_log_level(value: &str) -> Result<Level, String> {
    match value {
        "events" => Ok(Level::Events),
        "diagnostic" => Ok(Level::Diagnostic),
        "trace" => Ok(Level::Trace),
        _ => Err("livello log non valido: scegliere events, diagnostic o trace".to_owned()),
    }
}

fn has_help_flag(args: &[String]) -> bool {
    trace_cli_call(
        "cli::has_help_flag",
        || {
            vec![TraceField::new(
                "argument_count",
                TraceValue::data(args.len() as u64),
            )]
        },
        || {
            args.iter()
                .take_while(|argument| argument.as_str() != "--")
                .any(|argument| argument == "--help" || argument == "-h")
        },
        |found| TraceValue::data(*found),
    )
}

fn execute_build_patient(
    args: &[String],
    report_progress_notice: &mut impl FnMut(&str),
) -> CommandResult {
    trace_cli_call(
        "cli::execute_build_patient",
        || {
            vec![
                TraceField::new("argument_count", TraceValue::data(args.len() as u64)),
                TraceField::new(
                    "progress_callback",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ]
        },
        || {
            let (person_id, config_path, build_args) = match parse_build_patient_args(args) {
                Ok(parsed) => parsed,
                Err(error) => return CommandResult::usage_error(error),
            };
            let config_path = match config_path {
                Some(path) => path,
                None => match default_accounts_path(None) {
                    Ok(path) => path,
                    Err(error) => return CommandResult::operation_error(error.message()),
                },
            };
            let config = match load_accounts(&config_path) {
                Ok(config) => config,
                Err(error) => return CommandResult::operation_error(error.message()),
            };
            let repo_root = match resolved_repo_root(build_args.repo_root.as_deref()) {
                Ok(path) => path,
                Err(error) => return CommandResult::operation_error(error),
            };
            if let Err(error) = validate_runtime_privacy(&config, repo_root) {
                return CommandResult::operation_error(error.message());
            }
            let Some(person) = config
                .enabled_people()
                .find(|person| person.id == person_id)
            else {
                return CommandResult::operation_error(format!(
                    "paziente non trovato o disabilitato: {person_id}"
                ));
            };

            if let Some(notice) = progress_limit_notice(
                &format!("build-patient {}", person.id),
                build_args.no_progress,
            ) {
                report_progress_notice(&notice);
            }
            match build_patient(person, build_options(&build_args)) {
                Ok(result) => CommandResult::success(format_build_result(&result), String::new()),
                Err(error) => CommandResult::operation_error(error.message()),
            }
        },
        |result| TraceValue::data(u64::from(result.status)),
    )
}

fn execute_build_all(
    args: &[String],
    report_progress_notice: &mut impl FnMut(&str),
) -> CommandResult {
    trace_cli_call(
        "cli::execute_build_all",
        || {
            vec![
                TraceField::new("argument_count", TraceValue::data(args.len() as u64)),
                TraceField::new(
                    "progress_callback",
                    TraceValue::Omitted(TraceOmission::UnsupportedType),
                ),
            ]
        },
        || {
            let (config_path, build_args) = match parse_build_all_args(args) {
                Ok(parsed) => parsed,
                Err(error) => return CommandResult::usage_error(error),
            };
            let config_path = match config_path {
                Some(path) => path,
                None => match default_accounts_path(None) {
                    Ok(path) => path,
                    Err(error) => return CommandResult::operation_error(error.message()),
                },
            };
            let config = match load_accounts(&config_path) {
                Ok(config) => config,
                Err(error) => return CommandResult::operation_error(error.message()),
            };
            let repo_root = match resolved_repo_root(build_args.repo_root.as_deref()) {
                Ok(path) => path,
                Err(error) => return CommandResult::operation_error(error),
            };
            if let Err(error) = validate_runtime_privacy(&config, repo_root) {
                return CommandResult::operation_error(error.message());
            }
            let enabled_people: Vec<_> = config
                .enabled_people()
                .map(|person| person.id.as_str())
                .collect();
            if !enabled_people.is_empty()
                && let Some(notice) = progress_limit_notice("build-all", build_args.no_progress)
            {
                report_progress_notice(&notice);
            }
            match build_all(&config, build_options(&build_args)) {
                Ok(results) => CommandResult::success(
                    results
                        .iter()
                        .map(format_build_result)
                        .collect::<Vec<_>>()
                        .concat(),
                    String::new(),
                ),
                Err(error) => CommandResult::operation_error(error.message()),
            }
        },
        |result| TraceValue::data(u64::from(result.status)),
    )
}

fn parse_build_patient_args(
    args: &[String],
) -> Result<(String, Option<PathBuf>, BuildArguments), String> {
    trace_cli_call(
        "cli::parse_build_patient_args",
        || {
            vec![TraceField::new(
                "argument_count",
                TraceValue::data(args.len() as u64),
            )]
        },
        || {
            let mut patient_id = None;
            let mut config_path = None;
            let mut build = BuildArguments::default();
            let mut index = 0;
            let mut positional_only = false;
            while index < args.len() {
                let argument = &args[index];
                if positional_only {
                    set_patient_id(&mut patient_id, argument)?;
                } else if argument == "--" {
                    positional_only = true;
                } else if argument == "-c" || argument == "--config" {
                    config_path = Some(PathBuf::from(next_value(args, &mut index, argument)?));
                } else if let Some(value) = argument.strip_prefix("--config=") {
                    ensure_value(value, "--config")?;
                    config_path = Some(PathBuf::from(value));
                } else if argument == "-m" || argument == "--mode" {
                    build.mode = parse_mode(next_value(args, &mut index, argument)?)?;
                } else if let Some(value) = argument.strip_prefix("--mode=") {
                    build.mode = parse_mode(value)?;
                } else if argument == "-O" || argument == "--force-pdf-ocr" {
                    build.force_pdf_ocr = true;
                } else if argument == "-q" || argument == "--no-progress" {
                    build.no_progress = true;
                } else if argument == "-r" || argument == "--repo-root" {
                    build.repo_root = Some(PathBuf::from(next_value(args, &mut index, argument)?));
                } else if let Some(value) = argument.strip_prefix("--repo-root=") {
                    ensure_value(value, "--repo-root")?;
                    build.repo_root = Some(PathBuf::from(value));
                } else if argument.starts_with('-') {
                    return Err(format!(
                        "opzione non riconosciuta per build-patient: {argument}"
                    ));
                } else {
                    set_patient_id(&mut patient_id, argument)?;
                }
                index += 1;
            }
            let patient_id =
                patient_id.ok_or_else(|| "argomento obbligatorio mancante: persona".to_owned())?;
            Ok((patient_id, config_path, build))
        },
        |result| match result {
            Ok((person_id, config_path, build)) => {
                let mut fields = vec![
                    TraceField::new("person_id", trace_text(person_id.as_str())),
                    TraceField::new("mode", TraceValue::data(format!("{:?}", build.mode))),
                    TraceField::new("force_pdf_ocr", TraceValue::data(build.force_pdf_ocr)),
                    TraceField::new("no_progress", TraceValue::data(build.no_progress)),
                ];
                if let Some(path) = config_path {
                    fields.push(TraceField::new("config_path", TraceValue::path(path)));
                }
                if let Some(path) = &build.repo_root {
                    fields.push(TraceField::new("repo_root", TraceValue::path(path)));
                }
                TraceValue::record(&fields)
            }
            Err(_) => TraceValue::data(false),
        },
    )
}

fn parse_build_all_args(args: &[String]) -> Result<(Option<PathBuf>, BuildArguments), String> {
    trace_cli_call(
        "cli::parse_build_all_args",
        || {
            vec![TraceField::new(
                "argument_count",
                TraceValue::data(args.len() as u64),
            )]
        },
        || {
            let mut config_path = None;
            let mut build = BuildArguments::default();
            let mut index = 0;
            let mut positional_only = false;
            while index < args.len() {
                let argument = &args[index];
                if positional_only {
                    set_config_path(&mut config_path, argument)?;
                } else if argument == "--" {
                    positional_only = true;
                } else if argument == "-m" || argument == "--mode" {
                    build.mode = parse_mode(next_value(args, &mut index, argument)?)?;
                } else if let Some(value) = argument.strip_prefix("--mode=") {
                    build.mode = parse_mode(value)?;
                } else if argument == "-O" || argument == "--force-pdf-ocr" {
                    build.force_pdf_ocr = true;
                } else if argument == "-q" || argument == "--no-progress" {
                    build.no_progress = true;
                } else if argument == "-r" || argument == "--repo-root" {
                    build.repo_root = Some(PathBuf::from(next_value(args, &mut index, argument)?));
                } else if let Some(value) = argument.strip_prefix("--repo-root=") {
                    ensure_value(value, "--repo-root")?;
                    build.repo_root = Some(PathBuf::from(value));
                } else if argument.starts_with('-') {
                    return Err(format!(
                        "opzione non riconosciuta per build-all: {argument}"
                    ));
                } else {
                    set_config_path(&mut config_path, argument)?;
                }
                index += 1;
            }
            Ok((config_path, build))
        },
        |result| match result {
            Ok((config_path, build)) => {
                let mut fields = vec![
                    TraceField::new("mode", TraceValue::data(format!("{:?}", build.mode))),
                    TraceField::new("force_pdf_ocr", TraceValue::data(build.force_pdf_ocr)),
                    TraceField::new("no_progress", TraceValue::data(build.no_progress)),
                ];
                if let Some(path) = config_path {
                    fields.push(TraceField::new("config_path", TraceValue::path(path)));
                }
                if let Some(path) = &build.repo_root {
                    fields.push(TraceField::new("repo_root", TraceValue::path(path)));
                }
                TraceValue::record(&fields)
            }
            Err(_) => TraceValue::data(false),
        },
    )
}

fn set_patient_id(target: &mut Option<String>, value: &str) -> Result<(), String> {
    trace_cli_call(
        "cli::set_patient_id",
        || vec![TraceField::new("person_id", trace_text(value))],
        || {
            if target.is_some() {
                return Err("è consentito un solo argomento persona".to_owned());
            }
            target.replace(value.to_owned());
            Ok(())
        },
        |result| TraceValue::data(result.is_ok()),
    )
}

fn set_config_path(target: &mut Option<PathBuf>, value: &str) -> Result<(), String> {
    trace_cli_call(
        "cli::set_config_path",
        || vec![TraceField::new("config_path", trace_text(value))],
        || {
            if target.is_some() {
                return Err("è consentito un solo percorso di configurazione".to_owned());
            }
            target.replace(PathBuf::from(value));
            Ok(())
        },
        |result| TraceValue::data(result.is_ok()),
    )
}

fn next_value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, String> {
    trace_cli_call(
        "cli::next_value",
        || {
            vec![
                TraceField::new("option", TraceValue::data(option)),
                TraceField::new("argument_count", TraceValue::data(args.len() as u64)),
            ]
        },
        || {
            *index += 1;
            args.get(*index)
                .map(String::as_str)
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| format!("{option}: atteso un argomento"))
        },
        |result| match result {
            Ok(value) => trace_text(value),
            Err(_) => TraceValue::data(false),
        },
    )
}

fn ensure_value(value: &str, option: &str) -> Result<(), String> {
    trace_cli_call(
        "cli::ensure_value",
        || {
            vec![
                TraceField::new("option", TraceValue::data(option)),
                TraceField::new("value", trace_text(value)),
            ]
        },
        || {
            if value.is_empty() {
                Err(format!("{option}: atteso un argomento"))
            } else {
                Ok(())
            }
        },
        |result| TraceValue::data(result.is_ok()),
    )
}

fn parse_mode(value: &str) -> Result<BuildMode, String> {
    trace_cli_call(
        "cli::parse_mode",
        || vec![TraceField::new("mode", trace_text(value))],
        || match value {
            "full" => Ok(BuildMode::Full),
            "incremental" => Ok(BuildMode::Incremental),
            "validation" => Ok(BuildMode::Validation),
            _ => Err(format!(
                "modalità non valida: {value}; scegliere full, incremental o validation"
            )),
        },
        |result| match result {
            Ok(mode) => TraceValue::data(format!("{mode:?}")),
            Err(_) => TraceValue::data(false),
        },
    )
}

fn build_options(arguments: &BuildArguments) -> BuildOptions {
    trace_cli_call(
        "cli::build_options",
        || {
            vec![
                TraceField::new("mode", TraceValue::data(format!("{:?}", arguments.mode))),
                TraceField::new("force_pdf_ocr", TraceValue::data(arguments.force_pdf_ocr)),
            ]
        },
        || BuildOptions {
            mode: arguments.mode,
            force_pdf_ocr: arguments.force_pdf_ocr,
            parameter_debug: false,
        },
        |options| {
            TraceValue::record(&[
                TraceField::new("mode", TraceValue::data(format!("{:?}", options.mode))),
                TraceField::new("force_pdf_ocr", TraceValue::data(options.force_pdf_ocr)),
            ])
        },
    )
}

fn resolved_repo_root(path: Option<&Path>) -> Result<PathBuf, String> {
    trace_cli_call(
        "cli::resolved_repo_root",
        || {
            vec![TraceField::new(
                "repo_root",
                path.map_or(TraceValue::data(false), TraceValue::path),
            )]
        },
        || match path {
            Some(path) => Ok(path.to_path_buf()),
            None => std::env::current_dir()
                .map_err(|error| format!("directory corrente non disponibile: {error}")),
        },
        |result| match result {
            Ok(path) => TraceValue::path(path),
            Err(_) => TraceValue::data(false),
        },
    )
}

fn progress_limit_notice(command: &str, no_progress: bool) -> Option<String> {
    trace_cli_call(
        "cli::progress_limit_notice",
        || {
            vec![
                TraceField::new("command", trace_text(command)),
                TraceField::new("no_progress", TraceValue::data(no_progress)),
            ]
        },
        || {
            if no_progress {
                None
            } else {
                Some(format!(
                    "sanikey: {command}: avanzamento dettagliato non disponibile in questa versione\n"
                ))
            }
        },
        |notice| match notice {
            Some(value) => trace_text(value.as_str()),
            None => TraceValue::data(false),
        },
    )
}

fn format_build_result(result: &PatientBuildResult) -> String {
    trace_cli_call(
        "cli::format_build_result",
        || {
            vec![
                TraceField::new("person_id", trace_text(&result.patient_id)),
                TraceField::new("warning_count", TraceValue::data(result.warnings as u64)),
            ]
        },
        || {
            let mut output = String::new();
            output.push_str(&format!("paziente={} stato=ok\n", result.patient_id));
            output.push_str(&format!(
                "documenti={} duplicati={} avvisi={}\n",
                result.counts.documents, result.duplicates, result.warnings
            ));
            output.push_str(&format!(
                "documenti_derivati={} istanze_dicom={} record_totali={}\n",
                result.counts.derived_documents,
                result.counts.dicom_instances,
                result.counts.total_records
            ));
            output.push_str(&format!(
                "documenti_estratti={} documenti_cached={}\n",
                result.counts.extracted_documents, result.counts.cached_documents
            ));
            output.push_str(&format!(
                "build_root={}\n",
                display_path(&result.build_root)
            ));
            output.push_str(&format!("database={}\n", display_path(&result.database)));
            output.push_str(&format!("manifest={}\n", display_path(&result.manifest)));
            output.push_str(&format!("checksums={}\n", display_path(&result.checksums)));
            output.push_str(&format!("report={}\n", display_path(&result.report)));
            for warning in &result.warning_messages {
                if should_print_build_warning(warning) {
                    output.push_str(&format!("AVVISO: {}\n", relative_text(warning)));
                }
            }
            if result.warnings > 0 {
                output.push_str("Avvisi=vedi report\n");
            }
            output
        },
        |output| trace_text(output),
    )
}

fn should_print_build_warning(warning: &str) -> bool {
    trace_cli_call(
        "cli::should_print_build_warning",
        || vec![TraceField::new("warning", trace_text(warning))],
        || warning.starts_with("contenuto documento duplicato saltato.") || warning.contains("PDF"),
        |should_print| TraceValue::data(*should_print),
    )
}

fn display_path(path: &Path) -> String {
    trace_cli_call(
        "cli::display_path",
        || vec![TraceField::new("path", TraceValue::path(path))],
        || {
            let Ok(current_directory) = std::env::current_dir() else {
                return path.display().to_string();
            };
            let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let root = std::fs::canonicalize(&current_directory).unwrap_or(current_directory);
            absolute
                .strip_prefix(&root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.display().to_string())
        },
        |display| trace_text(display),
    )
}

fn relative_text(value: &str) -> String {
    trace_cli_call(
        "cli::relative_text",
        || vec![TraceField::new("text", trace_text(value))],
        || {
            let Ok(current_directory) = std::env::current_dir() else {
                return value.to_owned();
            };
            let root = format!("{}/", current_directory.display());
            value.replace(&root, "")
        },
        |text| trace_text(text),
    )
}

fn trace_text(value: &str) -> TraceValue {
    TraceValue::data(redact_secret_assignments(value))
}

#[cfg(test)]
mod tests {
    use super::{
        CommandResult, Component, Level, PatientBuildResult, Session, execute, format_build_result,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    /// Gestisce una directory temporanea isolata per i test CLI.
    impl TestDirectory {
        fn new() -> Self {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sanikey-cli-build-test-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("directory temporanea sintetica");
            Self(path)
        }
    }

    /// Elimina la directory temporanea alla fine del test.
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_config(root: &Path, people: &[(&str, bool)]) -> PathBuf {
        let mut config = String::from("[global]\nconfig_version = 1\n\n");
        for (id, enabled) in people {
            let base = root.join("patients").join(id);
            config.push_str(&format!(
                "[[person]]\nid = \"{id}\"\ndisplay_name = \"Persona {id}\"\nenabled = {enabled}\nsource_documents = \"{}\"\nmetadata_directory = \"{}\"\nlocal_build = \"{}\"\nusb_uuid = \"SYNTHETIC-UUID\"\n\n",
                base.join("documents").display(),
                base.join("metadata").display(),
                root.join("generated").join(id).display(),
            ));
            fs::create_dir_all(base.join("documents")).expect("directory documenti sintetici");
            fs::write(
                base.join("documents/report.txt"),
                format!("Referto sintetico {id}"),
            )
            .expect("documento sintetico");
        }
        let path = root.join("accounts.toml");
        fs::write(&path, config).expect("configurazione sintetica");
        path
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn read_log(root: &Path) -> String {
        let path = fs::read_dir(root)
            .expect("directory sintetica")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".sanikey-log.jsonl"))
            })
            .expect("file log sintetico");
        fs::read_to_string(path).expect("log sintetico leggibile")
    }

    #[test]
    fn info_command_preserves_project_identity_output() {
        let result = execute(&args(&["info"]));

        assert_eq!(
            result,
            CommandResult::success(
                "progetto=sanikey\npackage=sanikey\ncli=sanikey\nversione=0.1.0\n".to_owned(),
                String::new(),
            )
        );
    }

    #[test]
    fn version_flag_preserves_package_version_output() {
        let result = execute(&args(&["--version"]));

        assert_eq!(
            result,
            CommandResult::success("sanikey 0.1.0\n".to_owned(), String::new())
        );
    }

    #[test]
    fn logging_is_off_by_default_and_sensitive_levels_need_consent() {
        let default = execute(&args(&["--version"]));
        assert_eq!(default.status, 0);
        assert!(default.stderr.is_empty());

        for level in ["diagnostic", "trace"] {
            let result = execute(&args(&["--log-level", level, "info"]));
            assert_eq!(result.status, 2);
            assert!(result.stderr.contains("--confirm-sensitive-logging"));
        }

        let temp = TestDirectory::new();
        let result = execute(&args(&[
            "--confirm-sensitive-logging",
            "--log-level",
            "events",
            "--log-dir",
            temp.0.to_str().expect("path UTF-8"),
            "info",
        ]));
        assert_eq!(result.status, 0);
        let log = read_log(&temp.0);
        assert!(log.contains("\"activity\":\"cli_info\""));
    }

    #[test]
    fn events_log_records_only_fixed_cli_lifecycle_fields() {
        let temp = TestDirectory::new();
        let private_argument = "PRIVATE-CLI-ARGUMENT-SYNTHETIC";
        let result = execute(&args(&[
            "--log-level=events",
            "--log-dir",
            temp.0.to_str().expect("path UTF-8"),
            private_argument,
        ]));

        assert_eq!(result.status, 2);
        assert!(result.stderr.contains("log sessione:"));
        let entries: Vec<_> = fs::read_dir(&temp.0)
            .expect("directory log sintetica")
            .collect();
        assert_eq!(entries.len(), 1);
        let log = fs::read_to_string(entries[0].as_ref().unwrap().path())
            .expect("log sintetico leggibile");
        assert!(log.contains("\"component\":\"cli\""));
        assert!(log.contains("\"activity\":\"command\""));
        assert!(log.contains("\"failure_category\":\"usage\""));
        assert!(!log.contains(private_argument));
        assert!(!log.contains("comando non supportato"));
    }

    #[test]
    fn trace_level_reaches_instrumented_core_from_cli() {
        let temp = TestDirectory::new();
        let result = execute(&args(&[
            "--log-level=trace",
            "--confirm-sensitive-logging",
            "--log-dir",
            temp.0.to_str().expect("path UTF-8"),
            "info",
        ]));
        assert_eq!(result.status, 0);
        let log = read_log(&temp.0);
        assert!(log.contains("\"function\":\"cli::execute_command\""));
        assert!(log.contains("\"function\":\"project_info\""));
        assert!(log.contains("\"function\":\"operation::run_operation\""));
        assert!(log.contains("\"parent_call_id\":2"));
        assert!(log.contains("\"parent_operation_id\":1"));
    }

    #[test]
    fn trace_build_records_cli_parser_and_build_helpers() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("synthetic-person", true)]);
        let config_option = format!("--config={}", config_path.display());
        let result = execute(&args(&[
            "--log-level=trace",
            "--confirm-sensitive-logging",
            "--log-dir",
            temp.0.to_str().expect("path UTF-8"),
            "build-patient",
            "synthetic-person",
            &config_option,
            "--mode",
            "full",
            "--no-progress",
        ]));

        assert_eq!(result.status, 0);
        assert!(result.stdout.contains("paziente=synthetic-person stato=ok"));
        let log = read_log(&temp.0);
        for function in [
            "cli::execute_build_patient",
            "cli::parse_build_patient_args",
            "cli::set_patient_id",
            "cli::next_value",
            "cli::ensure_value",
            "cli::parse_mode",
            "cli::resolved_repo_root",
            "cli::build_options",
            "cli::progress_limit_notice",
            "cli::format_build_result",
            "cli::display_path",
            "cli::CommandResult::success",
        ] {
            assert!(
                log.contains(&format!("\"function\":\"{function}\"")),
                "{function}"
            );
        }
    }

    #[test]
    fn trace_build_all_records_its_parser_and_positional_config() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("synthetic-person", true)]);
        let result = execute(&args(&[
            "--log-level=trace",
            "--confirm-sensitive-logging",
            "--log-dir",
            temp.0.to_str().expect("path UTF-8"),
            "build-all",
            config_path.to_str().expect("config UTF-8"),
            "--mode=full",
            "-q",
        ]));

        assert_eq!(result.status, 0);
        assert!(result.stdout.contains("paziente=synthetic-person stato=ok"));
        let log = read_log(&temp.0);
        for function in [
            "cli::execute_build_all",
            "cli::parse_build_all_args",
            "cli::set_config_path",
            "cli::parse_mode",
            "cli::build_options",
            "cli::format_build_result",
        ] {
            assert!(
                log.contains(&format!("\"function\":\"{function}\"")),
                "{function}"
            );
        }
    }

    #[test]
    fn trace_records_unclassified_command_text_with_explicit_consent() {
        let temp = TestDirectory::new();
        let result = execute(&args(&[
            "--log-level=trace",
            "--confirm-sensitive-logging",
            "--log-dir",
            temp.0.to_str().unwrap(),
            "COMANDO-PRIVATO-SINTETICO",
        ]));
        assert_eq!(result.status, 2);
        let entry = fs::read_dir(&temp.0).unwrap().next().unwrap().unwrap();
        let log = fs::read_to_string(entry.path()).unwrap();
        assert!(log.contains("COMANDO-PRIVATO-SINTETICO"));
        assert!(log.contains("\"function\":\"cli::execute_command\""));
    }

    #[test]
    fn trace_redacts_identified_secrets_but_preserves_cli_output() {
        let temp = TestDirectory::new();
        let secret = "SECRET-SYNTHETIC-CLI";
        let argument = format!("api_key={secret}");
        let result = execute(&[
            "--log-level=trace".to_owned(),
            "--confirm-sensitive-logging".to_owned(),
            "--log-dir".to_owned(),
            temp.0.to_str().expect("path UTF-8").to_owned(),
            argument.clone(),
        ]);

        assert_eq!(result.status, 2);
        assert!(result.stderr.contains(&argument));
        let log = read_log(&temp.0);
        assert!(!log.contains(secret));
        assert!(log.contains("secret_omitted"));
        assert!(log.contains("cli::CommandResult::usage_error"));
    }

    #[test]
    fn trace_redacts_secret_assignment_in_build_result_patient_id() {
        let temp = TestDirectory::new();
        let secret = "SECRET-SYNTHETIC-PATIENT-ID";
        let patient_id = format!("api_key={secret}");
        let result = PatientBuildResult {
            patient_id: patient_id.clone(),
            build_root: temp.0.join("build"),
            counts: Default::default(),
            duplicates: 0,
            warnings: 0,
            warning_messages: Vec::new(),
            database: temp.0.join("build/database.sqlite"),
            manifest: temp.0.join("build/manifest.json"),
            checksums: temp.0.join("build/checksums.json"),
            report: temp.0.join("build/report.json"),
        };
        let (session, _) =
            Session::create_in_directory_with_level(&temp.0, Component::Cli, Level::Trace)
                .expect("sessione sintetica");

        let stdout = session.scoped(|| format_build_result(&result));
        session.finish(Component::Cli).expect("chiusura sessione");

        assert!(stdout.contains(&patient_id));
        let log = read_log(&temp.0);
        assert!(!log.contains(secret));
        assert!(log.contains("secret_omitted"));
        assert!(log.contains("cli::format_build_result"));
    }

    #[test]
    fn logging_options_require_a_level_and_recognized_values() {
        for options in [
            vec!["--log-dir", "/synthetic/logs", "info"],
            vec!["--log-level", "verbose", "info"],
            vec!["--confirm-sensitive-logging", "info"],
        ] {
            let result = execute(&args(&options));
            assert_eq!(result.status, 2);
            assert!(result.stdout.is_empty());
        }
    }

    #[test]
    fn unsupported_commands_fail_with_usage_status() {
        let result = execute(&args(&["deploy-usb"]));

        assert_eq!(result.status, 2);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("comando non supportato: deploy-usb"));
    }

    #[test]
    fn build_patient_uses_baseline_options_and_prints_result_paths() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("patient-a", true)]);
        let result = execute(&args(&[
            "build-patient",
            "patient-a",
            "--config",
            config_path.to_str().expect("path UTF-8"),
            "--mode",
            "full",
            "--no-progress",
        ]));

        assert_eq!(result.status, 0);
        assert!(result.stderr.is_empty());
        assert!(result.stdout.contains("paziente=patient-a stato=ok"));
        assert!(result.stdout.contains("documenti=1 duplicati=0 avvisi=0"));
        assert!(
            result
                .stdout
                .contains("documenti_estratti=1 documenti_cached=0")
        );
        assert!(result.stdout.contains("database="));
        assert!(result.stdout.contains("manifest="));
        assert!(
            temp.0
                .join("generated/patient-a/manifests/manifest.json")
                .is_file()
        );
    }

    #[test]
    fn build_all_uses_positional_config_and_skips_disabled_people() {
        let temp = TestDirectory::new();
        let config_path = write_config(
            &temp.0,
            &[
                ("patient-a", true),
                ("disabled", false),
                ("patient-b", true),
            ],
        );
        let result = execute(&args(&[
            "build-all",
            config_path.to_str().expect("path UTF-8"),
            "-m",
            "full",
            "-q",
        ]));

        assert_eq!(result.status, 0);
        assert!(result.stderr.is_empty());
        let first = result
            .stdout
            .find("paziente=patient-a stato=ok")
            .expect("prima persona");
        let second = result
            .stdout
            .find("paziente=patient-b stato=ok")
            .expect("seconda persona");
        assert!(first < second);
        assert!(!result.stdout.contains("paziente=disabled"));
        assert!(
            temp.0
                .join("generated/patient-a/manifests/manifest.json")
                .is_file()
        );
        assert!(
            temp.0
                .join("generated/patient-b/manifests/manifest.json")
                .is_file()
        );
        assert!(!temp.0.join("generated/disabled").exists());
    }

    #[test]
    fn runtime_failures_return_one_and_usage_failures_return_two() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("patient-a", true)]);
        let missing_person = execute(&args(&[
            "build-patient",
            "missing",
            "--config",
            config_path.to_str().expect("path UTF-8"),
            "-q",
        ]));
        assert_eq!(missing_person.status, 1);
        assert!(
            missing_person
                .stdout
                .contains("ERRORE: paziente non trovato o disabilitato")
        );
        assert!(missing_person.stderr.is_empty());

        let invalid_mode = execute(&args(&["build-all", "--mode", "other"]));
        assert_eq!(invalid_mode.status, 2);
        assert!(invalid_mode.stderr.contains("modalità non valida"));
        assert!(invalid_mode.stdout.is_empty());

        let missing_patient = execute(&args(&["build-patient", "--no-progress"]));
        assert_eq!(missing_patient.status, 2);
        assert!(
            missing_patient
                .stderr
                .contains("argomento obbligatorio mancante: persona")
        );
    }

    #[test]
    fn repo_root_privacy_preflight_rejects_versioned_paths_before_writing() {
        let temp = TestDirectory::new();
        let repo_root = temp.0.join("repository");
        let source = repo_root.join("src/private");
        fs::create_dir_all(&source).expect("sorgente sintetica sotto repository");
        fs::write(source.join("report.txt"), "Dato sintetico").expect("referto sintetico");
        let config_path = temp.0.join("accounts.toml");
        fs::write(
            &config_path,
            format!(
                "[global]\nconfig_version = 1\n\n[[person]]\nid = \"patient-a\"\ndisplay_name = \"Persona A\"\nsource_documents = \"{}\"\nmetadata_directory = \"{}\"\nlocal_build = \"{}\"\nusb_uuid = \"SYNTHETIC-UUID\"\n",
                source.display(),
                temp.0.join("metadata").display(),
                temp.0.join("generated/patient-a").display(),
            ),
        )
        .expect("configurazione sintetica");
        let config_text = config_path.to_str().expect("path UTF-8");
        let root_text = repo_root.to_str().expect("root UTF-8");

        let single = execute(&args(&[
            "build-patient",
            "patient-a",
            "--config",
            config_text,
            "--repo-root",
            root_text,
            "-q",
        ]));
        assert_eq!(single.status, 1);
        assert!(
            single
                .stdout
                .contains("contenuto versionato del repository")
        );
        assert!(!temp.0.join("generated/patient-a").exists());

        let all = execute(&args(&[
            "build-all",
            config_text,
            "--repo-root",
            root_text,
            "-q",
        ]));
        assert_eq!(all.status, 1);
        assert!(all.stdout.contains("contenuto versionato del repository"));
        assert!(!temp.0.join("generated/patient-a").exists());
    }

    #[test]
    fn progress_limit_notice_is_emitted_before_build_writes_outputs() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("patient-a", true)]);
        let args = args(&[
            "build-patient",
            "patient-a",
            "--config",
            config_path.to_str().expect("path UTF-8"),
        ]);
        let manifest = temp.0.join("generated/patient-a/manifests/manifest.json");
        let mut notice_seen = false;
        let result = super::execute_with_progress_notice(&args, |notice| {
            assert!(notice.contains("avanzamento dettagliato non disponibile"));
            assert!(!manifest.exists());
            notice_seen = true;
        });

        assert!(notice_seen);
        assert_eq!(result.status, 0);
        assert!(manifest.is_file());
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn force_pdf_ocr_disables_extraction_cache_and_progress_limit_is_explicit() {
        let temp = TestDirectory::new();
        let config_path = write_config(&temp.0, &[("patient-a", true)]);
        let config_text = config_path.to_str().expect("path UTF-8");
        let first = execute(&args(&[
            "build-patient",
            "patient-a",
            "-c",
            config_text,
            "-q",
        ]));
        assert_eq!(first.status, 0);
        assert!(
            first
                .stdout
                .contains("documenti_estratti=1 documenti_cached=0")
        );
        let incremental = execute(&args(&[
            "build-patient",
            "patient-a",
            "-c",
            config_text,
            "-q",
        ]));
        assert_eq!(incremental.status, 0);
        assert!(
            incremental
                .stdout
                .contains("documenti_estratti=0 documenti_cached=1")
        );
        let forced = execute(&args(&[
            "build-patient",
            "patient-a",
            "-c",
            config_text,
            "--force-pdf-ocr",
        ]));
        assert_eq!(forced.status, 0);
        assert!(
            forced
                .stdout
                .contains("documenti_estratti=1 documenti_cached=0")
        );
        assert!(
            forced
                .stderr
                .contains("avanzamento dettagliato non disponibile")
        );
        let quiet = execute(&args(&[
            "build-patient",
            "patient-a",
            "-c",
            config_text,
            "-q",
        ]));
        assert_eq!(quiet.status, 0);
        assert!(quiet.stderr.is_empty());
    }

    #[test]
    fn progress_limit_notice_has_no_synthetic_percentage() {
        let result = CommandResult::success(String::new(), String::new());
        assert_eq!(result.status, 0);
        assert!(super::BUILD_PATIENT_HELP.contains("full|incremental|validation"));
        assert!(super::BUILD_PATIENT_HELP.contains("--no-progress"));
        assert!(super::HELP.contains("non riceve eventi di avanzamento dettagliato"));
        assert!(super::HELP.contains("Root per controlli privacy"));
    }

    #[test]
    fn help_flag_works_after_build_options() {
        let patient_help = execute(&args(&["build-patient", "patient-a", "-q", "--help"]));
        let all_help = execute(&args(&[
            "build-all",
            "accounts.toml",
            "--mode",
            "full",
            "-h",
        ]));

        assert_eq!(patient_help.status, 0);
        assert!(patient_help.stdout.contains("Uso: sanikey build-patient"));
        assert_eq!(all_help.status, 0);
        assert!(all_help.stdout.contains("Uso: sanikey build-all"));
    }
}
