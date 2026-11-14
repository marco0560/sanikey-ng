//! Fixture sintetiche per assemblaggio e validazione dell'immagine USB.

use super::{
    assemble_usb_image, collect_files, escape_html, frontend_hrefs, normalize_relative, overlaps,
    percent_decode, quoted_attribute_values, safe_relative_path, validate_usb_image,
};
use crate::build::{BuildOptions, build_patient};
use crate::config::AccountsProfile;
use crate::logging::{Component, Level, Session};
use crate::operation::{CancellationToken, OperationError, OperationOutcome};
use crate::usb_copy::mirror_validated_usb_image;
#[cfg(target_os = "linux")]
use crate::usb_copy::mirror_validated_usb_image_on_linux_volume;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

/// Gestisce la root temporanea usata dalle fixture USB.
impl TestDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sanikey-ng-usb-image-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("directory temporanea sintetica");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Rimuove file e directory temporanei creati dal test.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn profile(directory: &TestDirectory) -> AccountsProfile {
    let mut people = String::new();
    for (id, name) in [("patient-a", "Persona A & B"), ("patient-b", "Persona B")] {
        let source = directory.path().join("patients").join(id).join("documents");
        let metadata = directory.path().join("patients").join(id).join("metadata");
        let local_build = directory.path().join("generated").join(id);
        fs::create_dir_all(&source).expect("documenti sintetici");
        fs::create_dir_all(&metadata).expect("metadati sintetici");
        fs::write(
            source.join("report.txt"),
            format!("Contenuto sintetico {id}"),
        )
        .expect("documento sintetico");
        people.push_str(&format!(
            "[[person]]\nid = \"{id}\"\ndisplay_name = \"{name}\"\nsource_documents = \"{}\"\nmetadata_directory = \"{}\"\nlocal_build = \"{}\"\nusb_uuid = \"1A2B-3C4D\"\nenabled = true\n",
            source.display(), metadata.display(), local_build.display()
        ));
    }
    let config_directory = directory.path().join("config");
    fs::create_dir_all(&config_directory).expect("directory config sintetica");
    let profile_path = config_directory.join("accounts.toml");
    fs::write(
        profile_path,
        format!("[global]\nconfig_version = 1\n{people}"),
    )
    .expect("profilo TOML sintetico");
    AccountsProfile::import(config_directory.join("accounts.toml"))
        .expect("profilo sintetico valido")
}

fn build_people(profile: &AccountsProfile) -> Vec<crate::build::PatientBuildResult> {
    profile
        .accounts()
        .enabled_people()
        .map(|person| build_patient(person, BuildOptions::default()).expect("build sintetica"))
        .collect()
}

fn traced_path(path: &Path) -> Value {
    if let Some(text) = path.to_str() {
        return json!({
            "kind": "data",
            "value": {"encoding": "utf8", "value": text},
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        json!({
            "kind": "data",
            "value": {
                "encoding": "unix_bytes",
                "value": path.as_os_str().as_bytes(),
            },
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        json!({
            "kind": "data",
            "value": {
                "encoding": "windows_utf16",
                "value": path.as_os_str().encode_wide().collect::<Vec<_>>(),
            },
        })
    }
}

fn traced_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "rust_debug",
            "value": crate::logging::redact_secret_assignments(&format!("{value:#?}")),
        },
    })
}

fn traced_argument(name: &str, content: Value) -> Value {
    json!({"name": name, "content": content})
}

fn traced_exit(debug_value: &str, native_paths: Value) -> Value {
    json!({
        "kind": "data",
        "value": {
            "format": "rust_debug",
            "value": crate::logging::redact_secret_assignments(debug_value),
            "native_paths": native_paths,
        },
    })
}

fn assert_usb_image_trace(records: &[Value], function: &str, arguments: Value, result: Value) {
    assert_usb_image_traces(records, function, vec![(arguments, result)]);
}

fn assert_usb_image_traces(records: &[Value], function: &str, mut expected: Vec<(Value, Value)>) {
    let entries: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "function_enter" && record["function"] == function)
        .collect();
    assert_eq!(entries.len(), expected.len(), "entrate di {function}");
    let mut unmatched = entries;
    for (arguments, result) in expected.drain(..) {
        let entry_index = unmatched
            .iter()
            .position(|entry| entry["arguments"] == arguments)
            .unwrap_or_else(|| panic!("argomenti attesi assenti per {function}: {arguments}"));
        let entry = unmatched.remove(entry_index);
        let call_id = entry["call_id"].as_u64().expect("call_id numerico");
        let exits: Vec<_> = records
            .iter()
            .filter(|record| {
                record["type"] == "function_exit"
                    && record["function"] == function
                    && record["call_id"] == call_id
            })
            .collect();
        assert_eq!(exits.len(), 1, "una sola uscita per {function}#{call_id}");
        assert_eq!(exits[0]["outcome"], "returned");
        assert_eq!(exits[0]["result"], result, "risultato di {function}");
        assert!(
            exits[0]["duration_ms"].as_u64().is_some(),
            "durata mancante per {function}#{call_id}"
        );
    }
    assert!(unmatched.is_empty());
}

#[test]
fn synthetic_usb_image_helpers_trace_all_arguments_results_and_durations() {
    let directory = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");

    let percent_input = "patients/patient-a/report%20one.pdf";
    let percent_result = "patients/patient-a/report one.pdf".to_owned();
    let relative_input = "patients/patient-a/report.txt";
    let relative_path = PathBuf::from(relative_input);
    let normalized_input = PathBuf::from("patients/patient-a/./report.txt");
    let normalized_result = "patients/patient-a/report.txt".to_owned();
    let overlap_left = directory.path().join("patients/patient-a");
    let overlap_right = directory.path().join("patients");
    let secret_html = "api_key = synthetic-usb-image-secret";
    let html_output = secret_html.to_owned();
    let attribute_html = "<a href=\"index.html\"><img href=\"viewer.html\">";
    let attribute_prefix = "href=\"";
    let frontend_input = json!({
        "href": "index.html",
        "nested": [{"viewer_href": "viewer.html"}],
    });
    let direct_href = frontend_input["href"].clone();
    let nested_value = frontend_input["nested"].clone();
    let nested_object = nested_value[0].clone();
    let nested_href = nested_object["viewer_href"].clone();
    let files_root = directory.path().join("manifest-files");
    fs::create_dir_all(files_root.join("nested")).expect("directory sintetica");
    fs::write(files_root.join("top.txt"), "contenuto A").expect("file sintetico A");
    fs::write(files_root.join("nested/leaf.txt"), "contenuto B").expect("file sintetico B");
    let expected_files = vec![
        files_root.join("nested/leaf.txt"),
        files_root.join("top.txt"),
    ];

    let results = session.scoped(|| {
        (
            percent_decode(percent_input).expect("percent decode valido"),
            safe_relative_path(relative_input).expect("percorso relativo valido"),
            normalize_relative(&normalized_input),
            overlaps(&overlap_left, &overlap_right),
            escape_html(secret_html),
            quoted_attribute_values(attribute_html, attribute_prefix),
            frontend_hrefs(&frontend_input),
            collect_files(&files_root).expect("raccolta file sintetici"),
        )
    });
    assert_eq!(results.0, percent_result);
    assert_eq!(results.1, relative_path);
    assert_eq!(results.2, normalized_result);
    assert!(results.3);
    assert_eq!(results.4, html_output);
    assert_eq!(results.5, ["index.html", "viewer.html"]);
    assert_eq!(results.6, ["index.html", "viewer.html"]);
    assert_eq!(results.7, expected_files);
    drop(session);

    let records: Vec<Value> = fs::read_to_string(&log_path)
        .expect("log sintetico leggibile")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record JSON valido"))
        .collect();
    let no_paths = json!({"kind": "data", "value": []});
    assert_usb_image_trace(
        &records,
        "usb_image::percent_decode",
        json!([traced_argument("value", traced_debug(&percent_input))]),
        traced_exit(
            &format!(
                "{:#?}",
                Ok::<String, OperationError>(percent_result.clone())
            ),
            no_paths.clone(),
        ),
    );

    let relative_result = Ok::<PathBuf, OperationError>(relative_path.clone());
    assert_usb_image_trace(
        &records,
        "usb_image::safe_relative_path",
        json!([
            traced_argument("value", traced_debug(&relative_input)),
            traced_argument("input_native_path", traced_path(Path::new(relative_input)),),
        ]),
        traced_exit(
            &format!("{relative_result:#?}"),
            json!({
                "kind": "data",
                "value": {
                    "state": "complete",
                    "path": traced_path(&relative_path),
                },
            }),
        ),
    );
    assert_usb_image_trace(
        &records,
        "usb_image::normalize_relative",
        json!([traced_argument("path", traced_path(&normalized_input),)]),
        traced_exit(&format!("{normalized_result:#?}"), no_paths.clone()),
    );
    assert_usb_image_trace(
        &records,
        "usb_image::overlaps",
        json!([
            traced_argument("left", traced_path(&overlap_left)),
            traced_argument("right", traced_path(&overlap_right)),
        ]),
        traced_exit(&format!("{:#?}", true), no_paths.clone()),
    );

    assert_usb_image_trace(
        &records,
        "usb_image::escape_html",
        json!([traced_argument("value", traced_debug(&secret_html))]),
        traced_exit("\"api_key = [secret_omitted]\"", no_paths.clone()),
    );
    assert_usb_image_trace(
        &records,
        "usb_image::quoted_attribute_values",
        json!([
            traced_argument("html", traced_debug(&attribute_html)),
            traced_argument("prefix", traced_debug(&attribute_prefix)),
        ]),
        traced_debug(&results.5),
    );
    assert_usb_image_traces(
        &records,
        "usb_image::frontend_hrefs",
        vec![
            (
                json!([traced_argument("value", traced_debug(&frontend_input))]),
                traced_debug(&results.6),
            ),
            (
                json!([traced_argument("value", traced_debug(&direct_href))]),
                traced_debug(&Vec::<&str>::new()),
            ),
            (
                json!([traced_argument("value", traced_debug(&nested_value))]),
                traced_debug(&vec!["viewer.html"]),
            ),
            (
                json!([traced_argument("value", traced_debug(&nested_object))]),
                traced_debug(&vec!["viewer.html"]),
            ),
            (
                json!([traced_argument("value", traced_debug(&nested_href))]),
                traced_debug(&Vec::<&str>::new()),
            ),
        ],
    );

    let files_result = Ok::<Vec<PathBuf>, OperationError>(expected_files.clone());
    let file_paths = json!({
        "kind": "data",
        "value": {
            "state": "complete",
            "paths": {
                "kind": "data",
                "value": expected_files
                    .iter()
                    .enumerate()
                    .map(|(index, path)| json!({"index": index, "path": traced_path(path)}))
                    .collect::<Vec<_>>(),
            },
        },
    });
    assert_usb_image_trace(
        &records,
        "usb_image::collect_files",
        json!([traced_argument("root", traced_path(&files_root))]),
        traced_exit(&format!("{files_result:#?}"), file_paths),
    );

    let serialized = serde_json::to_string(&records).expect("serializzazione log");
    assert!(serialized.contains("secret_omitted"));
    assert!(!serialized.contains("synthetic-usb-image-secret"));
}

#[test]
fn assembles_and_validates_a_multi_person_usb_image() {
    let directory = TestDirectory::new();
    let profile = profile(&directory);
    let builds = build_people(&profile);
    let target = directory.path().join("exports/usb-image");

    let result = assemble_usb_image(profile.accounts(), &builds, &target)
        .expect("immagine multi-persona assemblata");

    assert_eq!(result.patients, 2);
    assert!(result.files > 5);
    assert!(result.bytes > 0);
    assert!(result.manifest.is_file());
    assert!(
        target
            .join("patients/patient-a/medical_archive.db")
            .is_file()
    );
    assert!(
        target
            .join("patients/patient-b/medical_archive.db")
            .is_file()
    );
    assert!(
        target
            .join("patients/patient-a/documents/report.txt")
            .is_file()
    );
    let index = fs::read_to_string(target.join("index.html")).expect("indice USB");
    assert!(index.contains("Persona A &amp; B"));
    validate_usb_image(&target).expect("immagine USB valida");

    let unlisted = target.join("patients/patient-a/unlisted.txt");
    fs::write(&unlisted, "contenuto aggiunto senza checksum").expect("file extra sintetico");
    assert!(validate_usb_image(&target).is_err());
    fs::remove_file(unlisted).expect("rimozione file extra sintetico");

    let physical_volume = directory.path().join("synthetic-usb");
    fs::create_dir(&physical_volume).expect("target sintetico esistente");
    let copy = mirror_validated_usb_image(&target, &physical_volume, &CancellationToken::default());
    assert!(matches!(copy.outcome, OperationOutcome::Succeeded(_)));
    validate_usb_image(&physical_volume).expect("copia USB multi-persona valida");

    #[cfg(target_os = "linux")]
    {
        let linux_volume = directory.path().join("synthetic-linux-volume");
        fs::create_dir(&linux_volume).expect("volume Linux sintetico");
        let copy = mirror_validated_usb_image_on_linux_volume(
            profile.accounts(),
            &target,
            &linux_volume,
            &CancellationToken::default(),
        );
        assert!(matches!(copy.outcome, OperationOutcome::Succeeded(_)));
        validate_usb_image(linux_volume.join("SaniKey"))
            .expect("immagine validata nella cartella applicativa del volume");
    }

    fs::write(
        target.join("patients/patient-b/documents/report.txt"),
        "modifica sintetica",
    )
    .expect("alterazione sintetica");
    assert!(validate_usb_image(&target).is_err());
}

#[test]
fn rejects_existing_targets_and_builds_missing_an_enabled_person() {
    let directory = TestDirectory::new();
    let profile = profile(&directory);
    let builds = build_people(&profile);
    let existing = directory.path().join("existing-image");
    fs::create_dir(&existing).expect("target esistente");
    assert!(assemble_usb_image(profile.accounts(), &builds, &existing).is_err());
    assert!(
        assemble_usb_image(
            profile.accounts(),
            &builds[..1],
            directory.path().join("missing-build")
        )
        .is_err()
    );
    assert!(
        assemble_usb_image(
            profile.accounts(),
            &[builds[0].clone(), builds[0].clone()],
            directory.path().join("duplicate-builds")
        )
        .is_err()
    );
}

#[test]
fn usb_image_trace_records_full_native_paths_only_at_trace_level() {
    use crate::logging::{Component, Level, Session};

    for level in [Level::Events, Level::Trace] {
        let directory = TestDirectory::new();
        let profile = profile(&directory);
        let builds = build_people(&profile);
        let target = directory.path().join("exports/traced-image");
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.path().join("logs"),
            Component::Core,
            level,
        )
        .expect("sessione sintetica");

        let result = session.scoped(|| {
            let image = assemble_usb_image(profile.accounts(), &builds, &target)?;
            validate_usb_image(&target)?;
            Ok::<_, crate::operation::OperationError>(image)
        });
        assert!(result.is_ok());
        drop(session);

        let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
        if level == Level::Trace {
            assert!(log.contains("usb_image::assemble_usb_image"));
            assert!(log.contains("usb_image::assemble_staging"));
            assert!(log.contains("usb_image::copy_source_documents_inventory"));
            assert!(log.contains("usb_image::validate_usb_image"));
            assert!(log.contains("native_paths"));
            assert!(log.contains("patients/patient-a"));
        } else {
            assert!(!log.contains("usb_image::"));
            assert!(!log.contains("patient-a"));
        }
    }
}

#[cfg(unix)]
#[test]
fn usb_image_path_projections_preserve_non_unicode_nested_paths() {
    use std::os::unix::ffi::OsStringExt;

    use super::{UsbImageResult, trace_config_paths, trace_usb_result_paths};

    let directory = TestDirectory::new();
    let profile = profile(&directory);
    let mut config = profile.accounts().clone();
    config.path = PathBuf::from(std::ffi::OsString::from_vec(b"accounts-\xff.toml".to_vec()));
    config.people[0].ui.background_image = Some(PathBuf::from(std::ffi::OsString::from_vec(
        b"image-\xfe.png".to_vec(),
    )));
    let config_paths = trace_config_paths(&config).to_json();
    assert_eq!(
        config_paths["value"]["config_path"]["value"]["encoding"],
        "unix_bytes"
    );
    assert!(
        config_paths["value"]["config_path"]["value"]["value"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(0xff))
    );
    let nested = config_paths["value"]["people"][0]["paths"]["value"]
        .as_array()
        .unwrap();
    assert_eq!(nested[3]["path"]["value"]["encoding"], "unix_bytes");

    let image = Ok(UsbImageResult {
        root: PathBuf::from(std::ffi::OsString::from_vec(b"image-\xfd".to_vec())),
        manifest: PathBuf::from(std::ffi::OsString::from_vec(b"manifest-\xfc.json".to_vec())),
        patients: 1,
        files: 2,
        bytes: 3,
    });
    let image_paths = trace_usb_result_paths(&image).to_json();
    assert_eq!(
        image_paths["value"][0]["path"]["value"]["encoding"],
        "unix_bytes"
    );
}

#[test]
fn usb_image_error_trace_redacts_secret_like_debug_text_without_changing_error() {
    let directory = TestDirectory::new();
    let (session, log_path) = Session::create_in_directory_with_level(
        &directory.path().join("logs"),
        Component::Core,
        Level::Trace,
    )
    .expect("sessione trace sintetica");
    let message = "api_key = synthetic-usb-secret";
    let error = session.scoped(|| super::configuration_error(message));
    assert_eq!(error.message(), message);
    drop(session);

    let log = fs::read_to_string(log_path).expect("log sintetico leggibile");
    assert!(log.contains("secret_omitted"));
    assert!(!log.contains("synthetic-usb-secret"));
}
