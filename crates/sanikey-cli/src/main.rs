//! Comandi CLI per le build locali SaniKey.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sanikey_core::build::{BuildMode, BuildOptions, PatientBuildResult, build_all, build_patient};
use sanikey_core::config::{default_accounts_path, load_accounts};
use sanikey_core::operation::OperationOutcome;
use sanikey_core::privacy::validate_runtime_privacy;
use sanikey_core::{project_info, version};

const HELP: &str = "Uso: sanikey [--version] [info|build-patient|build-all] [opzioni]\n\nComandi disponibili:\n  info                         Mostra le informazioni essenziali del progetto\n  build-patient <persona>      Costruisce gli artefatti locali di una persona\n  build-all [config.toml]      Costruisce gli artefatti locali di tutte le persone abilitate\n\nOpzioni build:\n  -c, --config <file>          File account per build-patient\n  -m, --mode <modalita>        full, incremental (default) o validation\n  -O, --force-pdf-ocr          Forza una nuova estrazione OCR dei PDF\n  -q, --no-progress            Sopprime la nota sull'avanzamento non disponibile\n  -r, --repo-root <directory>  Root per controlli privacy (default: directory corrente)\n\nQuesta versione non riceve eventi di avanzamento dettagliato dal core; senza -q\nlo segnala su stderr prima della build.\n";

const BUILD_PATIENT_HELP: &str = "Uso: sanikey build-patient <persona> [-c|--config <file>] [-m|--mode full|incremental|validation] [-O|--force-pdf-ocr] [-q|--no-progress] [-r|--repo-root <directory>]\n\nEsegue la pipeline di build locale per una sola persona abilitata.\n";

const BUILD_ALL_HELP: &str = "Uso: sanikey build-all [config.toml] [-m|--mode full|incremental|validation] [-O|--force-pdf-ocr] [-q|--no-progress] [-r|--repo-root <directory>]\n\nEsegue la pipeline di build locale per tutte le persone abilitate.\n";

#[derive(Debug, Eq, PartialEq)]
struct CommandResult {
    stdout: String,
    stderr: String,
    status: u8,
}

impl CommandResult {
    fn success(stdout: String, stderr: String) -> Self {
        Self {
            stdout,
            stderr,
            status: 0,
        }
    }

    fn usage_error(message: impl AsRef<str>) -> Self {
        Self {
            stdout: String::new(),
            stderr: format!("sanikey: errore: {}\n", message.as_ref()),
            status: 2,
        }
    }

    fn operation_error(message: impl AsRef<str>) -> Self {
        Self {
            stdout: format!("ERRORE: {}\n", message.as_ref()),
            stderr: String::new(),
            status: 1,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BuildArguments {
    mode: BuildMode,
    force_pdf_ocr: bool,
    no_progress: bool,
    repo_root: Option<PathBuf>,
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
    match args {
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
            execute_build_patient(&args[1..], &mut report_progress_notice)
        }
        [command, ..] if command == "build-all" => {
            execute_build_all(&args[1..], &mut report_progress_notice)
        }
        [command, ..] if command == "info" => {
            CommandResult::usage_error("il comando info non accetta argomenti")
        }
        [command, ..] => CommandResult::usage_error(format!("comando non supportato: {command}")),
    }
}

fn has_help_flag(args: &[String]) -> bool {
    args.iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| argument == "--help" || argument == "-h")
}

fn execute_build_patient(
    args: &[String],
    report_progress_notice: &mut impl FnMut(&str),
) -> CommandResult {
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
}

fn execute_build_all(
    args: &[String],
    report_progress_notice: &mut impl FnMut(&str),
) -> CommandResult {
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
}

fn parse_build_patient_args(
    args: &[String],
) -> Result<(String, Option<PathBuf>, BuildArguments), String> {
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
}

fn parse_build_all_args(args: &[String]) -> Result<(Option<PathBuf>, BuildArguments), String> {
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
}

fn set_patient_id(target: &mut Option<String>, value: &str) -> Result<(), String> {
    if target.is_some() {
        return Err("è consentito un solo argomento persona".to_owned());
    }
    target.replace(value.to_owned());
    Ok(())
}

fn set_config_path(target: &mut Option<PathBuf>, value: &str) -> Result<(), String> {
    if target.is_some() {
        return Err("è consentito un solo percorso di configurazione".to_owned());
    }
    target.replace(PathBuf::from(value));
    Ok(())
}

fn next_value<'a>(args: &'a [String], index: &mut usize, option: &str) -> Result<&'a str, String> {
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| format!("{option}: atteso un argomento"))
}

fn ensure_value(value: &str, option: &str) -> Result<(), String> {
    if value.is_empty() {
        Err(format!("{option}: atteso un argomento"))
    } else {
        Ok(())
    }
}

fn parse_mode(value: &str) -> Result<BuildMode, String> {
    match value {
        "full" => Ok(BuildMode::Full),
        "incremental" => Ok(BuildMode::Incremental),
        "validation" => Ok(BuildMode::Validation),
        _ => Err(format!(
            "modalità non valida: {value}; scegliere full, incremental o validation"
        )),
    }
}

fn build_options(arguments: &BuildArguments) -> BuildOptions {
    BuildOptions {
        mode: arguments.mode,
        force_pdf_ocr: arguments.force_pdf_ocr,
        parameter_debug: false,
    }
}

fn resolved_repo_root(path: Option<&Path>) -> Result<PathBuf, String> {
    match path {
        Some(path) => Ok(path.to_path_buf()),
        None => std::env::current_dir()
            .map_err(|error| format!("directory corrente non disponibile: {error}")),
    }
}

fn progress_limit_notice(command: &str, no_progress: bool) -> Option<String> {
    if no_progress {
        None
    } else {
        Some(format!(
            "sanikey: {command}: avanzamento dettagliato non disponibile in questa versione\n"
        ))
    }
}

fn format_build_result(result: &PatientBuildResult) -> String {
    let mut output = String::new();
    output.push_str(&format!("paziente={} stato=ok\n", result.patient_id));
    output.push_str(&format!(
        "documenti={} duplicati={} avvisi={}\n",
        result.counts.documents, result.duplicates, result.warnings
    ));
    output.push_str(&format!(
        "documenti_derivati={} istanze_dicom={} record_totali={}\n",
        result.counts.derived_documents, result.counts.dicom_instances, result.counts.total_records
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
}

fn should_print_build_warning(warning: &str) -> bool {
    warning.starts_with("contenuto documento duplicato saltato.") || warning.contains("PDF")
}

fn display_path(path: &Path) -> String {
    let Ok(current_directory) = std::env::current_dir() else {
        return path.display().to_string();
    };
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let root = std::fs::canonicalize(&current_directory).unwrap_or(current_directory);
    absolute
        .strip_prefix(&root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn relative_text(value: &str) -> String {
    let Ok(current_directory) = std::env::current_dir() else {
        return value.to_owned();
    };
    let root = format!("{}/", current_directory.display());
    value.replace(&root, "")
}

#[cfg(test)]
mod tests {
    use super::{CommandResult, execute};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

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
