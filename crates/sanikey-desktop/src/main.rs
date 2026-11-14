//! Interfaccia desktop SaniKey per importare un profilo e scegliere persone e cartelle.

mod presentation;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
static NEXT_USB_IMAGE: AtomicUsize = AtomicUsize::new(0);

use eframe::egui::{self, Key, Modifiers};
use presentation::{Classification, PresentedText};
use sanikey_core::aifa::{
    AifaCandidate, AifaCandidateSearch, ConfirmedReferenceDecision, LeafletDownloadFailure,
    confirm_aifa_candidate, lookup_aifa_candidate_search, medication_fingerprint,
    probe_leaflet_documents, update_confirmed_reference,
};
use sanikey_core::build::{BuildOptions, PatientBuildResult, build_patient};
use sanikey_core::config::{AccountsConfig, AccountsProfile, PersonConfig};
use sanikey_core::inventory::{find_duplicate_documents, scan_document_inventory};
#[cfg(target_os = "linux")]
use sanikey_core::linux_volume::{LinuxVolumeInfo, inspect_linux_volume};
use sanikey_core::logging::{
    Activity, Component, DisplayControl, DisplayPart, FailureCategory, Level, NativeDialog,
    Operation, Outcome, ProgressPhase, Session, TraceField, TraceLink, TraceOmission, TraceValue,
    default_log_directory, trace_current, trace_sensitive_text,
};
use sanikey_core::metadata::{
    CuratedMetadata, Medication, MedicationLeaflet, MedicationLeafletExclusion,
    load_curated_metadata, prepare_people_metadata,
};
use sanikey_core::observation_imports::import_observations;
#[cfg(target_os = "linux")]
use sanikey_core::operation::OperationOutcome;
use sanikey_core::operation::{
    CancellationToken, OperationEvent, OperationPhase, run_operation_with_trace_result,
};
#[cfg(target_os = "linux")]
use sanikey_core::usb_copy::{UsbCopyResult, mirror_validated_usb_image_on_linux_volume};
use sanikey_core::usb_image::UsbImageResult;
#[cfg(target_os = "linux")]
use sanikey_core::usb_image::assemble_usb_image;

// The same classified value is passed to the widget and the capture sink.
macro_rules! presented {
    ($app:ident, $control:expr, $classification:expr, $value:expr) => {{
        let value = ($value).to_string();
        let presented = PresentedText::new(&value, $classification);
        presented
            .capture($app.log_session.as_ref(), &mut $app.log_failure, $control)
            .to_owned()
    }};
}

macro_rules! trace_desktop_call {
    ($function:literal, fields [$($name:literal => $value:expr),* $(,)?], output $output:expr, $body:expr) => {{
        let trace = trace_current(Component::Desktop, $function, || {
            Vec::from([$(TraceField::new($name, $value),)*])
        });
        let result = run_desktop_body($body);
        if let Some(trace) = trace {
            let _ = trace.finish(($output)(&result));
        }
        result
    }};
    ($function:literal, fields [$($name:literal => $value:expr),* $(,)?], $body:expr) => {
        trace_desktop_call!($function, fields [$($name => $value),*], output trace_desktop_debug, $body)
    };
}

fn run_desktop_body<R>(body: impl FnOnce() -> R) -> R {
    body()
}

fn trace_desktop_debug<T: std::fmt::Debug + ?Sized>(value: &T) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("format", TraceValue::data("rust_debug")),
        TraceField::new("value", trace_desktop_text(&format!("{value:#?}"))),
    ])
}

fn trace_desktop_path(path: &Path) -> TraceValue {
    TraceValue::path(path)
}

fn trace_desktop_text(value: &str) -> TraceValue {
    trace_sensitive_text(value)
}

fn trace_egui_boundary() -> TraceValue {
    TraceValue::Omitted(TraceOmission::ExternalComponent)
}

fn trace_person_paths(person: &PersonConfig) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("id", trace_desktop_text(&person.id)),
        TraceField::new(
            "source_documents",
            trace_desktop_path(&person.source_documents),
        ),
        TraceField::new(
            "metadata_directory",
            trace_desktop_path(&person.metadata_directory),
        ),
        TraceField::new("local_build", trace_desktop_path(&person.local_build)),
    ])
}

fn trace_people(people: &[PersonConfig]) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("count", TraceValue::data(people.len())),
        TraceField::new(
            "details",
            TraceValue::data(
                people
                    .iter()
                    .map(|person| {
                        TraceValue::record(&[
                            TraceField::new("person", trace_desktop_debug(person)),
                            TraceField::new("native_paths", trace_person_paths(person)),
                        ])
                        .to_json()
                    })
                    .collect::<Vec<_>>(),
            ),
        ),
    ])
}

fn trace_selections(selections: &[PersonSelection]) -> TraceValue {
    TraceValue::data(
        selections
            .iter()
            .map(|selection| {
                TraceValue::record(&[
                    TraceField::new("selected", TraceValue::data(selection.selected)),
                    TraceField::new(
                        "source_documents",
                        trace_desktop_path(&selection.source_documents),
                    ),
                    TraceField::new(
                        "metadata_directory",
                        trace_desktop_path(&selection.metadata_directory),
                    ),
                    TraceField::new("local_build", trace_desktop_path(&selection.local_build)),
                ])
                .to_json()
            })
            .collect::<Vec<_>>(),
    )
}

fn trace_config_paths(config: &AccountsConfig) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("config_path", trace_desktop_path(&config.path)),
        TraceField::new("people", trace_people(&config.people)),
    ])
}

fn trace_optional_config(config: &Option<AccountsConfig>) -> TraceValue {
    config.as_ref().map_or(TraceValue::data(()), |config| {
        TraceValue::record(&[
            TraceField::new("config", trace_desktop_debug(config)),
            TraceField::new("native_paths", trace_config_paths(config)),
        ])
    })
}

fn trace_path_result(result: &Path) -> TraceValue {
    trace_desktop_path(result)
}

fn trace_selection_result(result: &PersonSelection) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("selected", TraceValue::data(result.selected)),
        TraceField::new(
            "source_documents",
            trace_desktop_path(&result.source_documents),
        ),
        TraceField::new(
            "metadata_directory",
            trace_desktop_path(&result.metadata_directory),
        ),
        TraceField::new("local_build", trace_desktop_path(&result.local_build)),
    ])
}

fn trace_review_result(result: &Result<ReviewSummary, String>) -> TraceValue {
    TraceValue::record(&[
        TraceField::new(
            "state",
            TraceValue::data(if result.is_ok() { "complete" } else { "error" }),
        ),
        TraceField::new("value", trace_desktop_debug(result)),
        TraceField::new(
            "duplicate_paths",
            result
                .as_ref()
                .ok()
                .map_or(TraceValue::data(()), |summary| {
                    TraceValue::data(
                        summary
                            .duplicate_groups
                            .iter()
                            .map(|(_, _, paths)| {
                                TraceValue::data(
                                    paths
                                        .iter()
                                        .map(|path| trace_desktop_path(path).to_json())
                                        .collect::<Vec<_>>(),
                                )
                                .to_json()
                            })
                            .collect::<Vec<_>>(),
                    )
                }),
        ),
    ])
}

fn trace_aifa_people_result(result: &Result<Vec<AifaPersonReview>, String>) -> TraceValue {
    TraceValue::record(&[
        TraceField::new(
            "state",
            TraceValue::data(if result.is_ok() { "complete" } else { "error" }),
        ),
        TraceField::new("value", trace_desktop_debug(result)),
        TraceField::new(
            "metadata_directories",
            result.as_ref().ok().map_or(TraceValue::data(()), |people| {
                TraceValue::data(
                    people
                        .iter()
                        .map(|person| trace_desktop_path(&person.metadata_directory).to_json())
                        .collect::<Vec<_>>(),
                )
            }),
        ),
    ])
}

fn trace_app_summary(app: &DesktopApp) -> TraceValue {
    TraceValue::record(&[
        TraceField::new("profile_loaded", TraceValue::data(app.profile.is_some())),
        TraceField::new(
            "selected_people",
            TraceValue::data(
                app.selections
                    .iter()
                    .filter(|person| person.selected)
                    .count(),
            ),
        ),
        TraceField::new("review_loaded", TraceValue::data(app.review.is_some())),
        TraceField::new(
            "worker_active",
            TraceValue::data(app.worker_handle.is_some()),
        ),
        TraceField::new("log_active", TraceValue::data(app.log_session.is_some())),
    ])
}

fn trace_deferred_log_action(action: &Option<DeferredLogSessionAction>) -> TraceValue {
    match action {
        Some(DeferredLogSessionAction::Start { path, level, .. }) => TraceValue::record(&[
            TraceField::new("action", TraceValue::data("start")),
            TraceField::new("path", trace_desktop_path(path)),
            TraceField::new("level", trace_desktop_debug(level)),
        ]),
        Some(DeferredLogSessionAction::Stop) => {
            TraceValue::record(&[TraceField::new("action", TraceValue::data("stop"))])
        }
        None => TraceValue::data("no pending change"),
    }
}

const STRAW_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(248, 241, 198);
const FIELD_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(255, 252, 229);
const BLUE_TEXT: egui::Color32 = egui::Color32::from_rgb(0, 51, 204);
const MIN_PATH_FIELD_WIDTH: f32 = 64.0;
const WINDOW_TITLE: &str = "SaniKey";

#[derive(Clone, Debug)]
struct PersonSelection {
    selected: bool,
    source_documents: PathBuf,
    metadata_directory: PathBuf,
    local_build: PathBuf,
}

#[derive(Clone, Debug, Default)]
struct ReviewSummary {
    document_count: usize,
    duplicate_groups: Vec<(String, String, Vec<PathBuf>)>,
    selected_people: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct ObservationImportSummary {
    people: usize,
    series: usize,
    points: usize,
    warnings: Vec<String>,
}

enum DesktopEvent {
    Preflight(Result<ReviewSummary, String>),
    ObservationsImported(Result<ObservationImportSummary, String>),
    AifaMetadataLoaded(Result<Vec<AifaPersonReview>, String>),
    AifaSearchFinished {
        person_id: String,
        medication_id: String,
        result: Option<AifaCandidateSearch>,
    },
    AifaDecisionSaved {
        person_id: String,
        medication_id: String,
        result: Result<AifaDecisionSaved, String>,
    },
    PatientBuilt(PatientBuildResult),
    ImagePrepared(Result<UsbImageResult, String>),
    #[cfg(target_os = "linux")]
    TargetInspected(Result<LinuxVolumeInfo, String>),
    #[cfg(target_os = "linux")]
    UsbCopied(OperationOutcome<UsbCopyResult>),
    Progress(OperationEvent),
    Terminal(OperationEvent),
}

#[derive(Clone, Debug)]
struct AifaPersonReview {
    id: String,
    display_name: String,
    metadata_directory: PathBuf,
    metadata: CuratedMetadata,
    medications: Vec<AifaMedicationReview>,
}

#[derive(Clone, Debug)]
struct AifaMedicationReview {
    medication: Medication,
    query: String,
    search: Option<AifaCandidateSearch>,
    selected_candidate: Option<usize>,
    status: String,
}

#[derive(Clone, Debug)]
struct AifaDecisionSaved {
    metadata: CuratedMetadata,
    status: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingLogAction {
    Start(Level),
    Stop,
}

enum DeferredLogSessionAction {
    Start {
        session: Session,
        path: PathBuf,
        level: Level,
    },
    Stop,
}

#[derive(Clone, Debug)]
enum AifaDecision {
    Confirm(Box<AifaCandidate>),
    MarkNotApplicable,
}

enum AifaUiAction {
    Search(usize, usize, bool),
    Confirm(usize, usize, Box<AifaCandidate>),
    MarkNotApplicable(usize, usize),
}

struct DesktopApp {
    log_session: Option<Session>,
    log_failure: bool,
    log_path: Option<PathBuf>,
    log_directory: Option<PathBuf>,
    selected_log_level: Level,
    active_log_level: Option<Level>,
    confirm_sensitive_logging: bool,
    pending_log_action: Option<PendingLogAction>,
    log_action_error: Option<String>,
    worker_handle: Option<JoinHandle<()>>,
    profile_path: String,
    profile: Option<AccountsProfile>,
    selections: Vec<PersonSelection>,
    status: String,
    receiver: Option<Receiver<DesktopEvent>>,
    cancellation: Option<CancellationToken>,
    review: Option<ReviewSummary>,
    observation_import: Option<ObservationImportSummary>,
    aifa_people: Option<Vec<AifaPersonReview>>,
    progress: Option<f32>,
    completed_people: Vec<String>,
    completed_builds: Vec<PatientBuildResult>,
    usb_image: Option<UsbImageResult>,
    #[cfg(target_os = "linux")]
    usb_target: Option<PathBuf>,
    #[cfg(target_os = "linux")]
    usb_volume: Option<LinuxVolumeInfo>,
    #[cfg(target_os = "linux")]
    confirm_usb_target: bool,
    #[cfg(target_os = "linux")]
    usb_copy: Option<UsbCopyResult>,
}

/// Inizializza la schermata in attesa dell'importazione di un profilo.
impl Default for DesktopApp {
    fn default() -> Self {
        trace_desktop_call!("desktop::DesktopApp::default", fields [], output trace_app_summary, || {
        let (log_directory, log_action_error) = match default_log_directory() {
            Ok(path) => (Some(path), None),
            Err(error) => (
                None,
                Some(format!(
                    "Cartella predefinita dei log non disponibile: {error}"
                )),
            ),
        };
        Self {
            log_session: None,
            log_failure: false,
            log_path: None,
            log_directory,
            selected_log_level: Level::Events,
            active_log_level: None,
            confirm_sensitive_logging: false,
            pending_log_action: None,
            log_action_error,
            worker_handle: None,
            profile_path: String::new(),
            profile: None,
            selections: Vec::new(),
            status: "Aprire un profilo account per iniziare".to_owned(),
            receiver: None,
            cancellation: None,
            review: None,
            observation_import: None,
            aifa_people: None,
            progress: Some(0.0),
            completed_people: Vec::new(),
            completed_builds: Vec::new(),
            usb_image: None,
            #[cfg(target_os = "linux")]
            usb_target: None,
            #[cfg(target_os = "linux")]
            usb_volume: None,
            #[cfg(target_os = "linux")]
            confirm_usb_target: false,
            #[cfg(target_os = "linux")]
            usb_copy: None,
        }
        })
    }
}

/// Gestisce il profilo di sessione e gli eventi di preflight e build.
impl DesktopApp {
    fn launch_operation_worker(
        &mut self,
        activity: Activity,
        worker: impl FnOnce(Option<&Operation>) -> Outcome + Send + 'static,
    ) {
        trace_desktop_call!("desktop::DesktopApp::launch_operation_worker", fields [
            "activity" => trace_desktop_debug(&activity),
            "worker" => TraceValue::Omitted(TraceOmission::UnsupportedType),
        ], || {
        let operation = self.log_session.as_ref().and_then(|session| {
            match session.start_operation(Component::Desktop, activity) {
                Ok(operation) => Some(operation),
                Err(_) => {
                    self.log_failure = true;
                    None
                }
            }
        });
        self.worker_handle = Some(spawn_logged_worker(operation, None, move |operation| {
            Some(worker(operation))
        }));
        })
    }

    fn launch_session_worker(&mut self, worker: impl FnOnce() + Send + 'static) {
        trace_desktop_call!("desktop::DesktopApp::launch_session_worker", fields [
            "trace_link_present" => TraceValue::data(self.log_session.is_some()),
            "worker" => TraceValue::Omitted(TraceOmission::UnsupportedType),
        ], || {
        self.worker_handle = Some(spawn_logged_worker(
            None,
            self.log_session.as_ref().map(Session::trace_link),
            move |_| {
                worker();
                None
            },
        ));
        })
    }

    fn import_profile(&mut self, path: &Path) {
        trace_desktop_call!("desktop::DesktopApp::import_profile", fields ["path" => trace_desktop_path(path)], || {
        self.profile_path = path.display().to_string();
        let operation = self.log_session.as_ref().and_then(|session| {
            match session.start_operation(Component::Desktop, Activity::Profile) {
                Ok(operation) => Some(operation),
                Err(_) => {
                    self.log_failure = true;
                    None
                }
            }
        });
        let imported = operation.as_ref().map_or_else(
            || AccountsProfile::import(path),
            |operation| operation.scoped(|| AccountsProfile::import(path)),
        );
        let outcome = if imported.is_ok() {
            Outcome::Succeeded
        } else {
            Outcome::FailedWith(FailureCategory::Configuration)
        };
        match imported {
            Ok(profile) => {
                self.selections = profile
                    .accounts()
                    .people
                    .iter()
                    .map(selection_from_person)
                    .collect();
                self.profile_path = profile.source_path().display().to_string();
                self.status = format!("Profilo caricato: {} persone", self.selections.len());
                self.profile = Some(profile);
                self.review = None;
                self.observation_import = None;
                self.aifa_people = None;
                self.completed_people.clear();
                self.completed_builds.clear();
                self.usb_image = None;
                #[cfg(target_os = "linux")]
                {
                    self.usb_target = None;
                    self.usb_volume = None;
                    self.usb_copy = None;
                    self.confirm_usb_target = false;
                }
            }
            Err(error) => {
                self.profile = None;
                self.selections.clear();
                self.review = None;
                self.observation_import = None;
                self.aifa_people = None;
                self.status = format!("{}: {}", error.code().as_str(), error.message());
            }
        }
        if let Some(operation) = operation
            && operation.finish(outcome).is_err()
        {
            self.log_failure = true;
        }
        })
    }

    fn open_profile_dialog(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::open_profile_dialog", fields ["file_dialog" => trace_egui_boundary()], || {
        record_native_dialog_omission(
            self.log_session.as_ref(),
            &mut self.log_failure,
            NativeDialog::ProfileFile,
        );
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Profili account TOML", &["toml"])
            .pick_file()
        {
            self.import_profile(&path);
        }
        })
    }

    fn effective_config(&self) -> Option<AccountsConfig> {
        trace_desktop_call!("desktop::DesktopApp::effective_config", fields [
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selections" => trace_selections(&self.selections),
        ], output trace_optional_config, || {
        let profile = self.profile.as_ref()?;
        let base_directory = profile.base_directory();
        let mut config = profile.accounts().clone();
        for (person, selection) in config.people.iter_mut().zip(&self.selections) {
            person.enabled = selection.selected;
            person.source_documents =
                resolve_session_path(base_directory, &selection.source_documents);
            person.metadata_directory =
                resolve_session_path(base_directory, &selection.metadata_directory);
            person.local_build = resolve_session_path(base_directory, &selection.local_build);
        }
        Some(config)
        })
    }

    fn start_preflight(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::start_preflight", fields [
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
        ], || {
        if self
            .selections
            .iter()
            .filter(|person| person.selected)
            .any(|person| {
                [
                    &person.source_documents,
                    &person.metadata_directory,
                    &person.local_build,
                ]
                .iter()
                .any(|path| path.as_os_str().is_empty())
            })
        {
            self.status =
                "Completa tutti i percorsi delle persone selezionate prima del preflight."
                    .to_owned();
            return;
        }
        let Some(config) = self.effective_config() else {
            return;
        };
        let Some(profile) = &self.profile else {
            return;
        };
        let root = profile.base_directory().to_path_buf();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.review = None;
        self.observation_import = None;
        self.completed_people.clear();
        self.completed_builds.clear();
        self.usb_image = None;
        #[cfg(target_os = "linux")]
        {
            self.usb_target = None;
            self.usb_volume = None;
            self.usb_copy = None;
            self.confirm_usb_target = false;
        }
        self.status = "Controllo privacy, metadati, documenti e duplicati…".to_owned();
        self.launch_operation_worker(Activity::Preflight, move |_| {
            let result = review_profile(&config, &root);
            let outcome = outcome_from_result(&result, FailureCategory::Other);
            let _ = sender.send(DesktopEvent::Preflight(result));
            outcome
        });
        })
    }

    fn start_build(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::start_build", fields [
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
        ], || {
        let Some(config) = self.effective_config() else {
            return;
        };
        let selected = config
            .people
            .iter()
            .filter(|person| person.enabled)
            .cloned()
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.status = "Seleziona almeno una persona prima di avviare la build.".to_owned();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let cancellation = CancellationToken::default();
        self.receiver = Some(receiver);
        self.cancellation = Some(cancellation.clone());
        self.progress = Some(0.0);
        self.completed_people.clear();
        self.completed_builds.clear();
        self.usb_image = None;
        #[cfg(target_os = "linux")]
        {
            self.usb_copy = None;
        }
        self.status =
            "Build in corso; l'annullamento viene applicato al termine della persona corrente."
                .to_owned();
        self.launch_session_worker(move || build_selected_people(selected, cancellation, sender));
        })
    }

    fn start_observation_import(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::start_observation_import", fields [
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
        ], || {
        let Some(config) = self.effective_config() else {
            return;
        };
        let selected = config.enabled_people().cloned().collect::<Vec<_>>();
        if selected.is_empty() {
            self.status =
                "Seleziona almeno una persona prima di importare le osservazioni.".to_owned();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.observation_import = None;
        self.completed_people.clear();
        self.completed_builds.clear();
        self.usb_image = None;
        self.status = "Importazione delle osservazioni selezionate…".to_owned();
        self.launch_operation_worker(Activity::Observations, move |_| {
            let result = import_selected_observations(&selected);
            let outcome = outcome_from_result(&result, FailureCategory::Other);
            let _ = sender.send(DesktopEvent::ObservationsImported(result));
            outcome
        });
        })
    }

    fn start_aifa_review(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::start_aifa_review", fields [
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
        ], || {
        let Some(config) = self.effective_config() else {
            return;
        };
        let people = config.enabled_people().cloned().collect::<Vec<_>>();
        if people.is_empty() {
            self.status =
                "Seleziona almeno una persona prima di rivedere i farmaci AIFA.".to_owned();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.status = "Caricamento dei farmaci e dei riferimenti AIFA…".to_owned();
        self.launch_operation_worker(Activity::Aifa, move |_| {
            let result = load_aifa_people(&people);
            let outcome = outcome_from_result(&result, FailureCategory::Other);
            let _ = sender.send(DesktopEvent::AifaMetadataLoaded(result));
            outcome
        });
        })
    }

    fn start_aifa_search(&mut self, person_index: usize, medication_index: usize, manual: bool) {
        trace_desktop_call!("desktop::DesktopApp::start_aifa_search", fields [
            "person_index" => TraceValue::data(person_index),
            "medication_index" => TraceValue::data(medication_index),
            "manual_query" => TraceValue::data(manual),
            "query_text" => TraceValue::data("captured by PresentedText"),
        ], || {
        let Some(people) = self.aifa_people.as_ref() else {
            return;
        };
        let Some(person) = people.get(person_index) else {
            return;
        };
        let Some(medication) = person.medications.get(medication_index) else {
            return;
        };
        let query = if manual {
            let value = medication.query.trim();
            if value.is_empty() {
                self.status = "Inserisci un testo per la ricerca manuale AIFA.".to_owned();
                return;
            }
            Some(value.to_owned())
        } else {
            None
        };
        let person_id = person.id.clone();
        let medication_id = medication.medication.id.clone();
        let medication = medication.medication.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.status = format!("Ricerca AIFA in corso per {medication_id}…");
        self.launch_operation_worker(Activity::Aifa, move |_| {
            let result = lookup_aifa_candidate_search(&medication, query.as_deref());
            let outcome = if result.is_some() {
                Outcome::Succeeded
            } else {
                Outcome::FailedWith(FailureCategory::Other)
            };
            let _ = sender.send(DesktopEvent::AifaSearchFinished {
                person_id,
                medication_id,
                result,
            });
            outcome
        });
        })
    }

    fn start_aifa_decision(
        &mut self,
        person_index: usize,
        medication_index: usize,
        decision: AifaDecision,
    ) {
        trace_desktop_call!("desktop::DesktopApp::start_aifa_decision", fields [
            "person_index" => TraceValue::data(person_index),
            "medication_index" => TraceValue::data(medication_index),
            "decision_kind" => TraceValue::data(match &decision { AifaDecision::Confirm(_) => "confirm", AifaDecision::MarkNotApplicable => "mark_not_applicable" }),
            "candidate_details" => TraceValue::data("captured by PresentedText"),
        ], || {
        let Some(people) = self.aifa_people.as_ref() else {
            return;
        };
        let Some(person) = people.get(person_index) else {
            return;
        };
        let Some(medication) = person.medications.get(medication_index) else {
            return;
        };
        let person_id = person.id.clone();
        let medication_id = medication.medication.id.clone();
        let metadata_directory = person.metadata_directory.clone();
        let metadata = person.metadata.clone();
        let medication = medication.medication.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.status = format!("Verifica e salvataggio AIFA in corso per {medication_id}…");
        self.launch_operation_worker(Activity::Aifa, move |_| {
            let result = save_aifa_decision(
                &metadata_directory,
                metadata,
                &medication,
                decision,
                probe_leaflet_documents,
            );
            let outcome = outcome_from_result(&result, FailureCategory::Other);
            let _ = sender.send(DesktopEvent::AifaDecisionSaved {
                person_id,
                medication_id,
                result,
            });
            outcome
        });
        })
    }

    #[cfg(target_os = "linux")]
    fn prepare_usb_image(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::prepare_usb_image", fields [
            "completed_builds" => TraceValue::data(self.completed_builds.len()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
        ], || {
        let Some(config) = self.effective_config() else {
            return;
        };
        if self.completed_builds.len() != config.enabled_people().count() {
            self.status =
                "Completa tutte le build selezionate prima di preparare l'immagine USB.".to_owned();
            return;
        }
        let builds = self.completed_builds.clone();
        let Some(first) = config.enabled_people().next() else {
            return;
        };
        let parent = first.local_build.parent().unwrap_or(&first.local_build);
        let destination = parent.join(format!(
            ".sanikey-usb-image-{}-{}",
            std::process::id(),
            NEXT_USB_IMAGE.fetch_add(1, Ordering::Relaxed)
        ));
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.status = "Preparazione e validazione dell'immagine USB multi-persona…".to_owned();
        self.launch_operation_worker(Activity::Usb, move |_| {
            let result = assemble_usb_image(&config, &builds, destination);
            let outcome = outcome_from_result(&result, FailureCategory::Usb);
            let result =
                result.map_err(|error| format!("{}: {}", error.code().as_str(), error.message()));
            let _ = sender.send(DesktopEvent::ImagePrepared(result));
            outcome
        });
        })
    }

    #[cfg(target_os = "linux")]
    fn choose_usb_target(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::choose_usb_target", fields [
            "file_dialog" => trace_egui_boundary(),
        ], || {
        record_native_dialog_omission(
            self.log_session.as_ref(),
            &mut self.log_failure,
            NativeDialog::UsbTargetFolder,
        );
        let Some(target) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let worker_target = target.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.status = "Lettura del filesystem e dello spazio libero del volume…".to_owned();
        self.launch_operation_worker(Activity::Usb, move |_| {
            let result = inspect_linux_volume(&worker_target);
            let outcome = outcome_from_result(&result, FailureCategory::Usb);
            let result =
                result.map_err(|error| format!("{}: {}", error.code().as_str(), error.message()));
            let _ = sender.send(DesktopEvent::TargetInspected(result));
            outcome
        });
        self.usb_target = Some(target);
        self.usb_volume = None;
        self.confirm_usb_target = false;
        self.usb_copy = None;
        })
    }

    #[cfg(target_os = "linux")]
    fn copy_usb_image(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::copy_usb_image", fields [
            "image_ready" => TraceValue::data(self.usb_image.is_some()),
            "target_selected" => TraceValue::data(self.usb_target.is_some()),
            "target_confirmed" => TraceValue::data(self.confirm_usb_target),
        ], || {
        let (Some(config), Some(image), Some(target)) = (
            self.effective_config(),
            self.usb_image.clone(),
            self.usb_target.clone(),
        ) else {
            return;
        };
        if !self.confirm_usb_target {
            self.status = "Conferma esplicitamente la destinazione prima della copia.".to_owned();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.status = "Copia USB in corso; verifica finale in attesa…".to_owned();
        self.launch_session_worker(move || {
            let run = mirror_validated_usb_image_on_linux_volume(
                &config,
                &image.root,
                &target,
                &CancellationToken::default(),
            );
            let _ = sender.send(DesktopEvent::UsbCopied(run.outcome));
        });
        })
    }

    fn receive_worker_events(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::receive_worker_events", fields [
            "receiver_active" => TraceValue::data(self.receiver.is_some()),
            "event_payloads" => TraceValue::Omitted(TraceOmission::UnsupportedType),
        ], || {
        let mut messages = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = &self.receiver {
            loop {
                match receiver.try_recv() {
                    Ok(message) => messages.push(message),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for message in messages {
            match message {
                DesktopEvent::Preflight(Ok(review)) => {
                    self.status = format!(
                        "Controlli completati: {} persone, {} documenti, {} gruppi duplicati. Esamina il riepilogo prima della build.",
                        review.selected_people.len(),
                        review.document_count,
                        review.duplicate_groups.len()
                    );
                    self.review = Some(review);
                    self.receiver = None;
                }
                DesktopEvent::Preflight(Err(error)) => {
                    self.status = format!("Preflight fallito: {error}");
                    self.receiver = None;
                    self.review = None;
                }
                DesktopEvent::ObservationsImported(Ok(summary)) => {
                    self.status = format!(
                        "Importazione completata: {} persone, {} serie, {} osservazioni, {} avvisi.",
                        summary.people,
                        summary.series,
                        summary.points,
                        summary.warnings.len()
                    );
                    self.observation_import = Some(summary);
                    self.receiver = None;
                }
                DesktopEvent::ObservationsImported(Err(error)) => {
                    self.status = format!("Importazione osservazioni fallita: {error}");
                    self.receiver = None;
                }
                DesktopEvent::AifaMetadataLoaded(Ok(people)) => {
                    let medication_count = people
                        .iter()
                        .map(|person| person.medications.len())
                        .sum::<usize>();
                    self.status = format!(
                        "Farmaci caricati: {medication_count} in {} persone. Cerca e conferma i riferimenti AIFA.",
                        people.len()
                    );
                    self.aifa_people = Some(people);
                    self.receiver = None;
                }
                DesktopEvent::AifaMetadataLoaded(Err(error)) => {
                    self.status = format!("Caricamento metadati AIFA fallito: {error}");
                    self.aifa_people = None;
                    self.receiver = None;
                }
                DesktopEvent::AifaSearchFinished {
                    person_id,
                    medication_id,
                    result,
                } => {
                    if let Some(medication) = self
                        .aifa_people
                        .as_mut()
                        .and_then(|people| people.iter_mut().find(|person| person.id == person_id))
                        .and_then(|person| {
                            person
                                .medications
                                .iter_mut()
                                .find(|item| item.medication.id == medication_id)
                        })
                    {
                        medication.selected_candidate = None;
                        medication.search = result.clone();
                        medication.status = match result {
                            None => "Servizio AIFA non disponibile; nessun riferimento modificato."
                                .to_owned(),
                            Some(search) if search.all_candidates.is_empty() => {
                                "Nessun candidato restituito da AIFA.".to_owned()
                            }
                            Some(search) => format!(
                                "{} candidati esatti, {} da rivedere.",
                                search.exact.len(),
                                search.review.len()
                            ),
                        };
                        self.status = format!("Ricerca AIFA completata per {medication_id}.");
                    }
                    self.receiver = None;
                }
                DesktopEvent::AifaDecisionSaved {
                    person_id,
                    medication_id,
                    result,
                } => {
                    match result {
                        Ok(saved) => {
                            if let Some(person) = self.aifa_people.as_mut().and_then(|people| {
                                people.iter_mut().find(|person| person.id == person_id)
                            }) {
                                person.metadata = saved.metadata;
                                if let Some(medication) = person
                                    .medications
                                    .iter_mut()
                                    .find(|item| item.medication.id == medication_id)
                                {
                                    medication.search = None;
                                    medication.selected_candidate = None;
                                    medication.status = saved.status.clone();
                                }
                            }
                            self.status = saved.status;
                        }
                        Err(error) => {
                            self.status = format!("Riferimento AIFA non salvato: {error}");
                        }
                    }
                    self.receiver = None;
                }
                DesktopEvent::PatientBuilt(result) => {
                    self.completed_people.push(result.patient_id.clone());
                    self.completed_builds.push(result);
                }
                DesktopEvent::ImagePrepared(Ok(image)) => {
                    self.status = format!(
                        "Immagine pronta e validata: {} persone, {} file, {} byte.",
                        image.patients, image.files, image.bytes
                    );
                    self.usb_image = Some(image);
                    self.receiver = None;
                }
                DesktopEvent::ImagePrepared(Err(error)) => {
                    self.status = format!("Preparazione immagine fallita: {error}");
                    self.receiver = None;
                }
                #[cfg(target_os = "linux")]
                DesktopEvent::TargetInspected(Ok(volume)) => {
                    self.status =
                        "Volume Linux letto. Verifica target e conferma prima della copia."
                            .to_owned();
                    self.usb_volume = Some(volume);
                    self.receiver = None;
                }
                #[cfg(target_os = "linux")]
                DesktopEvent::TargetInspected(Err(error)) => {
                    self.status = format!("Target USB non disponibile: {error}");
                    self.usb_target = None;
                    self.receiver = None;
                }
                #[cfg(target_os = "linux")]
                DesktopEvent::UsbCopied(OperationOutcome::Succeeded(result)) => {
                    self.status = "Copia completata e immagine USB validata sul volume.".to_owned();
                    self.usb_copy = Some(result);
                    self.receiver = None;
                }
                #[cfg(target_os = "linux")]
                DesktopEvent::UsbCopied(OperationOutcome::Cancelled) => {
                    self.status = "Copia annullata; il target può essere parziale e può essere riallineato ripetendo la copia.".to_owned();
                    self.receiver = None;
                }
                #[cfg(target_os = "linux")]
                DesktopEvent::UsbCopied(OperationOutcome::Failed(error)) => {
                    self.status = format!(
                        "Copia USB fallita ({}): {}",
                        error.code().as_str(),
                        error.message()
                    );
                    self.receiver = None;
                }
                DesktopEvent::Progress(event) => match event {
                    OperationEvent::Started { .. } => {
                        self.status = "Build avviata…".to_owned();
                    }
                    OperationEvent::Progress {
                        phase,
                        completed,
                        total,
                    } => {
                        self.progress = total
                            .filter(|total| *total > 0)
                            .map(|total| completed as f32 / total as f32);
                        self.status = match phase {
                            OperationPhase::Preparing => "Preparazione build…".to_owned(),
                            OperationPhase::Processing => format!(
                                "Build completate: {completed}{}",
                                total.map_or(String::new(), |total| format!("/{total}"))
                            ),
                            OperationPhase::Finalizing => "Finalizzazione build…".to_owned(),
                        };
                    }
                    _ => {}
                },
                DesktopEvent::Terminal(event) => {
                    self.status = match event {
                        OperationEvent::Succeeded => "Build completata.".to_owned(),
                        OperationEvent::Cancelled => "Build annullata al punto sicuro.".to_owned(),
                        OperationEvent::Failed(error) => format!(
                            "Build fallita ({}): {}",
                            error.code().as_str(),
                            error.message()
                        ),
                        _ => "Evento terminale inatteso.".to_owned(),
                    };
                    self.receiver = None;
                    self.cancellation = None;
                }
            }
        }
        if disconnected && self.receiver.is_some() {
            self.receiver = None;
            self.cancellation = None;
            self.status = "Operazione interrotta senza esito; controlla gli artefatti e ripeti la build incrementale.".to_owned();
        }
        if self.receiver.is_none()
            && self
                .worker_handle
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
        {
            self.join_worker();
        }
        })
    }

    fn join_worker(&mut self) {
        trace_desktop_call!("desktop::DesktopApp::join_worker", fields [
            "worker_present" => TraceValue::data(self.worker_handle.is_some()),
            "worker_finished" => TraceValue::data(self.worker_handle.as_ref().is_some_and(JoinHandle::is_finished)),
        ], || {
        if let Some(worker) = self.worker_handle.take()
            && worker.join().is_err()
        {
            self.log_action_error =
                Some("Il worker desktop si è interrotto in modo inatteso.".to_owned());
        }
        })
    }
}

fn resolve_session_path(base_directory: &Path, path: &Path) -> PathBuf {
    trace_desktop_call!("desktop::resolve_session_path", fields ["base_directory" => trace_desktop_path(base_directory), "path" => trace_desktop_path(path)], output trace_path_result, || {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_directory.join(path)
    }
    })
}

fn review_profile(config: &AccountsConfig, root: &Path) -> Result<ReviewSummary, String> {
    trace_desktop_call!("desktop::review_profile", fields ["config" => trace_desktop_debug(config), "native_paths" => trace_config_paths(config), "root" => trace_desktop_path(root)], output trace_review_result, || {
    prepare_people_metadata(config, root)
        .map_err(|error| format!("{}: {}", error.code().as_str(), error.message()))?;
    let mut summary = ReviewSummary::default();
    for person in config.enabled_people() {
        summary.selected_people.push(person.display_name.clone());
        let inventory = scan_document_inventory(person)
            .map_err(|error| format!("{}: {}", error.code().as_str(), error.message()))?;
        summary.document_count += inventory.len();
        for (digest, documents) in find_duplicate_documents(&inventory) {
            summary.duplicate_groups.push((
                person.display_name.clone(),
                digest,
                documents
                    .into_iter()
                    .map(|document| document.path)
                    .collect(),
            ));
        }
    }
    Ok(summary)
    })
}

fn import_selected_observations(
    people: &[PersonConfig],
) -> Result<ObservationImportSummary, String> {
    trace_desktop_call!("desktop::import_selected_observations", fields ["people" => trace_people(people)], || {
    let mut summary = ObservationImportSummary::default();
    for person in people {
        let imported = import_observations(&person.metadata_directory, &person.source_documents)
            .map_err(|error| {
                format!(
                    "{} ({}): {}",
                    person.display_name,
                    error.code().as_str(),
                    error.message()
                )
            })?;
        summary.people += 1;
        summary.series += imported.series.len();
        summary.points += imported.points.len();
        summary.warnings.extend(
            imported
                .warnings
                .into_iter()
                .map(|warning| format!("{}: {warning}", person.display_name)),
        );
    }
    Ok(summary)
    })
}

fn load_aifa_people(people: &[PersonConfig]) -> Result<Vec<AifaPersonReview>, String> {
    trace_desktop_call!("desktop::load_aifa_people", fields ["people" => trace_people(people)], output trace_aifa_people_result, || {
    people
        .iter()
        .map(|person| {
            let metadata = load_curated_metadata(&person.metadata_directory).map_err(|error| {
                format!(
                    "{} ({}): {}",
                    person.display_name,
                    error.code().as_str(),
                    error.message()
                )
            })?;
            let medications = metadata
                .medications
                .iter()
                .cloned()
                .map(|medication| AifaMedicationReview {
                    medication,
                    query: String::new(),
                    search: None,
                    selected_candidate: None,
                    status: String::new(),
                })
                .collect();
            Ok(AifaPersonReview {
                id: person.id.clone(),
                display_name: person.display_name.clone(),
                metadata_directory: person.metadata_directory.clone(),
                metadata,
                medications,
            })
        })
        .collect()
    })
}

fn candidates_for_review(search: &AifaCandidateSearch) -> &[AifaCandidate] {
    trace_desktop_call!("desktop::candidates_for_review", fields ["search" => trace_desktop_debug(search)], || {
    if search.exact.is_empty() {
        &search.review
    } else {
        &search.exact
    }
    })
}

fn show_aifa_candidate(
    ui: &mut egui::Ui,
    candidate: &AifaCandidate,
    session: Option<&Session>,
    log_failure: &mut bool,
    person_index: usize,
    medication_index: usize,
) {
    trace_desktop_call!("desktop::show_aifa_candidate", fields [
        "ui" => trace_egui_boundary(),
        "candidate_fields" => TraceValue::data("captured by PresentedText"),
        "person_index" => TraceValue::data(person_index),
        "medication_index" => TraceValue::data(medication_index),
        "presentation_values" => TraceValue::data("captured by PresentedText"),
    ], || {
    let fields = [
        (
            "aifa_ingredients",
            format!(
                "Principi attivi: {}",
                candidate.active_ingredients.join(", ")
            ),
        ),
        (
            "aifa_form",
            format!(
                "Forma: {}",
                candidate.form.as_deref().unwrap_or("non indicata")
            ),
        ),
        (
            "aifa_strength",
            format!(
                "Dosaggio: {}",
                candidate.strength.as_deref().unwrap_or("non indicato")
            ),
        ),
        (
            "aifa_company",
            format!(
                "Titolare: {}",
                candidate.company.as_deref().unwrap_or("non indicato")
            ),
        ),
        ("aifa_atc", format!("ATC: {}", candidate.atc.join(", "))),
        (
            "aifa_routes",
            format!("Vie: {}", candidate.routes.join(", ")),
        ),
        (
            "aifa_packages",
            format!("Confezioni: {}", candidate.packages.join("; ")),
        ),
        (
            "aifa_identifier",
            format!(
                "Identificativo AIFA: {}/{}",
                candidate.codice_sis, candidate.aic6
            ),
        ),
    ];
    for (name, text) in &fields {
        let text = PresentedText::new(text, Classification::Private);
        ui.label(text.capture(
            session,
            log_failure,
            DisplayControl::indexed(name, person_index, Some(medication_index)),
        ));
    }
    if !candidate.mismatches.is_empty() {
        let text = format!("Campi da verificare: {}", candidate.mismatches.join(", "));
        let text = PresentedText::new(&text, Classification::Private);
        ui.colored_label(
            warning_text_color(ui.visuals().dark_mode),
            text.capture(
                session,
                log_failure,
                DisplayControl::indexed("aifa_mismatches", person_index, Some(medication_index)),
            ),
        );
    }
    })
}

fn save_aifa_decision(
    metadata_directory: &Path,
    metadata: CuratedMetadata,
    medication: &Medication,
    decision: AifaDecision,
    probe: impl FnOnce(&MedicationLeaflet) -> Vec<LeafletDownloadFailure>,
) -> Result<AifaDecisionSaved, String> {
    trace_desktop_call!("desktop::save_aifa_decision", fields [
        "metadata_directory" => trace_desktop_path(metadata_directory),
        "metadata" => trace_desktop_debug(&metadata),
        "medication" => trace_desktop_debug(medication),
        "decision_kind" => TraceValue::data(match &decision { AifaDecision::Confirm(_) => "confirm", AifaDecision::MarkNotApplicable => "mark_not_applicable" }),
        "probe" => TraceValue::Omitted(TraceOmission::UnsupportedType),
    ], || {
        save_aifa_decision_with(metadata_directory, metadata, medication, decision, probe)
    })
}

fn save_aifa_decision_with(
    metadata_directory: &Path,
    metadata: CuratedMetadata,
    medication: &Medication,
    decision: AifaDecision,
    probe: impl FnOnce(&MedicationLeaflet) -> Vec<LeafletDownloadFailure>,
) -> Result<AifaDecisionSaved, String> {
    trace_desktop_call!("desktop::save_aifa_decision_with", fields [
        "metadata_directory" => trace_desktop_path(metadata_directory),
        "metadata" => trace_desktop_debug(&metadata),
        "medication" => trace_desktop_debug(medication),
        "decision_kind" => TraceValue::data(match &decision { AifaDecision::Confirm(_) => "confirm", AifaDecision::MarkNotApplicable => "mark_not_applicable" }),
        "probe" => TraceValue::Omitted(TraceOmission::UnsupportedType),
    ], || {
    let expected_reference = metadata
        .medication_leaflets
        .iter()
        .find(|item| item.medication_id == medication.id)
        .cloned();
    let expected_exclusion = metadata
        .medication_leaflet_exclusions
        .iter()
        .find(|item| item.medication_id == medication.id)
        .cloned();
    let expected_fingerprint = medication_fingerprint(medication);
    let (decision, status) = match decision {
        AifaDecision::Confirm(candidate) => {
            let reference = confirm_aifa_candidate(medication, &candidate);
            let failures = probe(&reference);
            if !failures.is_empty() {
                return Err(failures
                    .iter()
                    .map(|failure| format!("{} {}", failure.kind, failure.reason))
                    .collect::<Vec<_>>()
                    .join("; "));
            }
            (
                ConfirmedReferenceDecision::Confirm(reference),
                format!(
                    "Riferimento AIFA {}/{} salvato dopo verifica FI/RCP.",
                    candidate.codice_sis, candidate.aic6
                ),
            )
        }
        AifaDecision::MarkNotApplicable => (
            ConfirmedReferenceDecision::MarkNotApplicable(MedicationLeafletExclusion {
                medication_id: medication.id.clone(),
                reason: "non_aifa".to_owned(),
            }),
            format!("Stato non AIFA salvato per {}.", medication.name),
        ),
    };
    let metadata = update_confirmed_reference(
        metadata_directory,
        &medication.id,
        &expected_fingerprint,
        expected_reference.as_ref(),
        expected_exclusion.as_ref(),
        decision,
    )
    .map_err(|error| format!("{}: {}", error.code().as_str(), error.message()))?;
    Ok(AifaDecisionSaved { metadata, status })
    })
}

fn build_selected_people(
    people: Vec<PersonConfig>,
    cancellation: CancellationToken,
    sender: Sender<DesktopEvent>,
) {
    trace_desktop_call!("desktop::build_selected_people", fields [
        "people" => trace_desktop_debug(&people),
        "cancellation_requested" => TraceValue::data(cancellation.is_cancelled()),
        "event_sender" => TraceValue::Omitted(TraceOmission::UnsupportedType),
    ], || {
    build_selected_people_with(people, cancellation, sender, |person| {
        build_patient(person, BuildOptions::default())
    });
    })
}

fn build_selected_people_with(
    people: Vec<PersonConfig>,
    cancellation: CancellationToken,
    sender: Sender<DesktopEvent>,
    mut build: impl FnMut(
        &PersonConfig,
    ) -> Result<PatientBuildResult, sanikey_core::operation::OperationError>,
) {
    trace_desktop_call!("desktop::build_selected_people_with", fields [
        "people" => trace_desktop_debug(&people),
        "cancellation_requested" => TraceValue::data(cancellation.is_cancelled()),
        "event_sender" => TraceValue::Omitted(TraceOmission::UnsupportedType),
        "build_callback" => TraceValue::Omitted(TraceOmission::UnsupportedType),
    ], || {
    let total = people.len() as u64;
    let _ = sender.send(DesktopEvent::Progress(OperationEvent::Started {
        operation: "build.generate",
    }));
    let run = run_operation_with_trace_result(
        "build.generate",
        &cancellation,
        |context| {
            context.set_phase(OperationPhase::Processing);
            for (index, person) in people.into_iter().enumerate() {
                if context.is_cancelled() {
                    break;
                }
                context.report_progress(index as u64, None);
                let _ = sender.send(DesktopEvent::Progress(OperationEvent::Progress {
                    phase: OperationPhase::Processing,
                    completed: index as u64,
                    total: None,
                }));
                let result = build(&person)?;
                let _ = sender.send(DesktopEvent::PatientBuilt(result));
                let completed = index as u64 + 1;
                context.report_progress(completed, Some(total));
                let _ = sender.send(DesktopEvent::Progress(OperationEvent::Progress {
                    phase: OperationPhase::Processing,
                    completed,
                    total: Some(total),
                }));
            }
            Ok(())
        },
        |_: &()| TraceValue::data(()),
    );
    if let Some(event) = run.events.last() {
        let _ = sender.send(DesktopEvent::Terminal(event.clone()));
    }
    let _outcome = run.outcome;
    })
}

fn selection_from_person(person: &PersonConfig) -> PersonSelection {
    trace_desktop_call!("desktop::selection_from_person", fields [
        "person" => trace_desktop_debug(person),
        "paths" => TraceValue::record(&[
            TraceField::new("source_documents", trace_desktop_path(&person.source_documents)),
            TraceField::new("metadata_directory", trace_desktop_path(&person.metadata_directory)),
            TraceField::new("local_build", trace_desktop_path(&person.local_build)),
        ]),
    ], output trace_selection_result, || {
    PersonSelection {
        selected: person.enabled,
        source_documents: person.source_documents.clone(),
        metadata_directory: person.metadata_directory.clone(),
        local_build: person.local_build.clone(),
    }
    })
}

fn spawn_logged_worker(
    operation: Option<Operation>,
    log_link: Option<TraceLink>,
    worker: impl FnOnce(Option<&Operation>) -> Option<Outcome> + Send + 'static,
) -> JoinHandle<()> {
    trace_desktop_call!("desktop::spawn_logged_worker", fields [
        "operation_present" => TraceValue::data(operation.is_some()),
        "trace_link_present" => TraceValue::data(log_link.is_some()),
        "worker_callback" => TraceValue::Omitted(TraceOmission::UnsupportedType),
    ], output |_| TraceValue::data("worker thread spawned"), || {
    thread::spawn(move || {
        let mut worker = Some(worker);
        if let Some(operation) = operation {
            let outcome = operation.scoped(|| {
                let trace = trace_current(Component::Desktop, "desktop::spawn_logged_worker::run", || {
                    Vec::from([
                        TraceField::new("operation_present", TraceValue::data(true)),
                        TraceField::new("worker_callback", TraceValue::Omitted(TraceOmission::UnsupportedType)),
                    ])
                });
                let _ = operation.phase(ProgressPhase::Processing);
                let outcome = worker
                    .take()
                    .expect("worker callback used exactly once")(Some(&operation))
                    .unwrap_or(Outcome::Failed);
                if let Some(trace) = trace {
                    let _ = trace.finish(trace_desktop_debug(&outcome));
                }
                outcome
            });
            let _ = operation.finish(outcome);
        } else if let Some(link) = log_link {
            link.scoped(|| {
                let trace = trace_current(Component::Desktop, "desktop::spawn_logged_worker::run", || {
                    Vec::from([
                        TraceField::new("trace_link_present", TraceValue::data(true)),
                        TraceField::new("worker_callback", TraceValue::Omitted(TraceOmission::UnsupportedType)),
                    ])
                });
                let result = worker.take().expect("worker callback used exactly once")(None);
                if let Some(trace) = trace {
                    let _ = trace.finish(trace_desktop_debug(&result));
                }
                result
            });
        } else {
            worker.take().expect("worker callback used exactly once")(None);
        }
    })
    })
}

fn outcome_from_result<T, E>(result: &Result<T, E>, category: FailureCategory) -> Outcome {
    trace_desktop_call!("desktop::outcome_from_result", fields [
        "success" => TraceValue::data(result.is_ok()),
        "failure_category" => trace_desktop_debug(&category),
    ], || {
    match result {
        Ok(_) => Outcome::Succeeded,
        Err(_) => Outcome::FailedWith(category),
    }
    })
}

fn record_ui_action(
    session: Option<&Session>,
    log_failure: &mut bool,
    activity: Activity,
    target: DisplayControl,
) {
    trace_desktop_call!("desktop::record_ui_action", fields [
        "session_active" => TraceValue::data(session.is_some()),
        "activity" => trace_desktop_debug(&activity),
        "target" => trace_desktop_debug(&target),
    ], || {
    let Some(session) = session else {
        return;
    };
    match session.start_control_action(activity, target) {
        Ok(operation) => {
            if operation.finish(Outcome::Succeeded).is_err() {
                *log_failure = true;
            }
        }
        Err(_) => *log_failure = true,
    }
    })
}

fn record_native_dialog_omission(
    session: Option<&Session>,
    log_failure: &mut bool,
    dialog: NativeDialog,
) {
    let Some(session) = session else {
        return;
    };
    if session.record_native_dialog_omission(dialog).is_err() {
        *log_failure = true;
    }
}

fn choose_folder(
    ui: &mut egui::Ui,
    label_text: &str,
    path: &mut PathBuf,
    session: Option<&Session>,
    log_failure: &mut bool,
    control: DisplayControl,
) {
    trace_desktop_call!("desktop::choose_folder", fields [
        "ui" => trace_egui_boundary(),
        "control" => trace_desktop_debug(&control),
        "label_text" => TraceValue::data("captured by PresentedText"),
        "path_text" => TraceValue::data("captured by PresentedText"),
    ], || {
    let label_text = PresentedText::new(label_text, Classification::Public);
    let label =
        ui.label(label_text.capture(session, log_failure, control.with_part(DisplayPart::Label)));
    let mut text = path.display().to_string();
    ui.horizontal_wrapped(|ui| {
        let response = add_responsive_path_edit(ui, label.id, &mut text, 100.0);
        if response.changed() {
            *path = PathBuf::from(&text);
            record_ui_action(session, log_failure, Activity::GuiFolderEdited, control);
        }
        let button = PresentedText::new("Scegli…", Classification::Public);
        if ui
            .button(button.capture(session, log_failure, control.with_part(DisplayPart::Action)))
            .clicked()
        {
            record_native_dialog_omission(session, log_failure, NativeDialog::PathFolder);
            if let Some(chosen) = rfd::FileDialog::new().pick_folder() {
                *path = chosen;
                record_ui_action(
                session,
                log_failure,
                Activity::GuiFolderChosen,
                control.with_part(DisplayPart::Action),
                );
            }
        }
    });
    PresentedText::new(&text, Classification::Private).capture(session, log_failure, control);
    })
}

fn add_responsive_path_edit(
    ui: &mut egui::Ui,
    label_id: egui::Id,
    text: &mut String,
    trailing_controls_width: f32,
) -> egui::Response {
    trace_desktop_call!("desktop::add_responsive_path_edit", fields [
        "ui" => trace_egui_boundary(),
        "label_id" => trace_egui_boundary(),
        "text" => TraceValue::data("captured by caller presentation"),
        "trailing_controls_width" => trace_desktop_debug(&trailing_controls_width),
    ], output |_| trace_egui_boundary(), || {
    let width = (ui.available_width() - trailing_controls_width).max(MIN_PATH_FIELD_WIDTH);
    ui.add_sized(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::TextEdit::singleline(text),
    )
    .labelled_by(label_id)
    })
}

fn desktop_visuals() -> egui::Visuals {
    trace_desktop_call!("desktop::desktop_visuals", fields [], || {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = STRAW_BACKGROUND;
    visuals.window_fill = STRAW_BACKGROUND;
    visuals.faint_bg_color = egui::Color32::from_rgb(241, 232, 179);
    visuals.extreme_bg_color = FIELD_BACKGROUND;
    visuals.text_edit_bg_color = Some(FIELD_BACKGROUND);
    visuals.code_bg_color = FIELD_BACKGROUND;
    visuals.override_text_color = Some(BLUE_TEXT);
    visuals.weak_text_color = Some(BLUE_TEXT);
    visuals.disabled_alpha = 1.0;
    visuals.hyperlink_color = BLUE_TEXT;
    visuals.selection.bg_fill = egui::Color32::from_rgb(190, 211, 239);
    visuals.selection.stroke.color = BLUE_TEXT;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.fg_stroke.color = BLUE_TEXT;
    }
    visuals
    })
}

fn configure_desktop_theme(context: &egui::Context) {
    trace_desktop_call!("desktop::configure_desktop_theme", fields ["context" => trace_egui_boundary()], || {
    context.set_theme(egui::Theme::Light);
    context.set_visuals(desktop_visuals());
    })
}

#[cfg(test)]
fn show_path(ui: &mut egui::Ui, label_text: &str, path: &Path) {
    render_path(ui, label_text, &path.display().to_string());
}

fn render_path(ui: &mut egui::Ui, label_text: &str, text: &str) {
    trace_desktop_call!("desktop::render_path", fields [
        "ui" => trace_egui_boundary(),
        "label_text" => TraceValue::data("captured by caller presentation"),
        "path_text" => TraceValue::data("captured by caller presentation"),
    ], || {
    ui.label(label_text);
    ui.add(
        egui::Label::new(egui::RichText::new(text).monospace()).wrap_mode(egui::TextWrapMode::Wrap),
    );
    })
}

fn show_captured_path(
    ui: &mut egui::Ui,
    label_text: &str,
    path: &Path,
    session: Option<&Session>,
    log_failure: &mut bool,
    control: DisplayControl,
) {
    trace_desktop_call!("desktop::show_captured_path", fields [
        "ui" => trace_egui_boundary(),
        "session_active" => TraceValue::data(session.is_some()),
        "control" => trace_desktop_debug(&control),
        "label_and_path" => TraceValue::data("captured by PresentedText"),
    ], || {
    let path_text = path.display().to_string();
    let presented = PresentedText::new(&path_text, Classification::Private);
    let label = PresentedText::new(label_text, Classification::Public);
    let label = label.capture(session, log_failure, control.with_part(DisplayPart::Label));
    render_path(ui, label, presented.capture(session, log_failure, control));
    })
}

/// Sceglie un rosso leggibile sia sul fondo chiaro sia su quello scuro.
fn warning_text_color(dark_mode: bool) -> egui::Color32 {
    trace_desktop_call!("desktop::warning_text_color", fields ["dark_mode" => TraceValue::data(dark_mode)], || {
    if dark_mode {
        egui::Color32::LIGHT_RED
    } else {
        egui::Color32::DARK_RED
    }
    })
}

/// Gestisce i controlli di registrazione e le viste operative della GUI.
impl DesktopApp {
    fn can_start_logging(&self) -> bool {
        trace_desktop_call!("desktop::DesktopApp::can_start_logging", fields [
            "receiver_active" => TraceValue::data(self.receiver.is_some()),
            "worker_active" => TraceValue::data(self.worker_handle.is_some()),
            "active_level" => trace_desktop_debug(&self.active_log_level),
            "selected_level" => trace_desktop_debug(&self.selected_log_level),
            "log_directory_configured" => TraceValue::data(self.log_directory.is_some()),
            "sensitive_logging_confirmed" => TraceValue::data(self.confirm_sensitive_logging),
        ], || {
        self.receiver.is_none()
            && self.worker_handle.is_none()
            && self.active_log_level != Some(self.selected_log_level)
            && self.log_directory.is_some()
            && (self.selected_log_level < Level::Diagnostic || self.confirm_sensitive_logging)
        })
    }

    fn show_logging_controls(&mut self, ui: &mut egui::Ui) {
        trace_desktop_call!("desktop::DesktopApp::show_logging_controls", fields [
            "ui" => trace_egui_boundary(),
            "selected_level" => trace_desktop_debug(&self.selected_log_level),
            "active_level" => trace_desktop_debug(&self.active_log_level),
            "presentation_values" => TraceValue::data("captured by PresentedText"),
        ], || {
        ui.group(|ui| {
            ui.label(presented!(
                self,
                DisplayControl::new("logging_heading"),
                Classification::Public,
                "Registrazione log per questa sessione"
            ));
            let levels = [
                (Level::Events, "Eventi"),
                (Level::Diagnostic, "Diagnostica"),
                (Level::Trace, "Traccia completa"),
            ];
            let worker_idle = self.receiver.is_none() && self.worker_handle.is_none();
            ui.add_enabled_ui(worker_idle, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (level, label) in levels {
                        let control = DisplayControl::new(match level {
                            Level::Events => "logging_level_events",
                            Level::Diagnostic => "logging_level_diagnostic",
                            Level::Trace => "logging_level_trace",
                        });
                        if ui
                            .radio_value(
                                &mut self.selected_log_level,
                                level,
                                presented!(self, control, Classification::Public, label),
                            )
                            .changed()
                        {
                            record_ui_action(
                                self.log_session.as_ref(),
                                &mut self.log_failure,
                                Activity::GuiLogLevelSelected,
                                DisplayControl::new("logging_level"),
                            );
                            self.confirm_sensitive_logging = false;
                        }
                    }
                });
            });
            let directory_text = self
                .log_directory
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "cartella utente non disponibile".to_owned());
            ui.label(presented!(
                self,
                DisplayControl::new("logging_directory"),
                Classification::Private,
                format!("Cartella log: {directory_text}")
            ));
            if let Some(path) = &self.log_path {
                ui.label(presented!(
                    self,
                    DisplayControl::new("logging_active_path"),
                    Classification::Private,
                    format!("File della sessione: {}", path.display())
                ));
            }
            let sensitive = self.selected_log_level >= Level::Diagnostic;
            match self.selected_log_level {
                Level::Events => {
                    ui.label(presented!(
                        self,
                        DisplayControl::new("logging_events_description"),
                        Classification::Public,
                        "Registra eventi operativi senza testo privato della GUI."
                    ));
                }
                Level::Diagnostic => {
                    ui.label(presented!(
                        self,
                        DisplayControl::new("logging_diagnostic_warning"),
                        Classification::Public,
                        "Può registrare dati sanitari, percorsi e altro testo privato mostrato nella GUI."
                    ));
                }
                Level::Trace => {
                    ui.label(presented!(
                        self,
                        DisplayControl::new("logging_trace_warning"),
                        Classification::Public,
                        "Può includere dati privati. La copertura delle funzioni è progressiva e il log può avere omissioni."
                    ));
                }
            }
            ui.label(presented!(
                self,
                DisplayControl::new("logging_permissions_limit"),
                Classification::Public,
                "Su sistemi non Unix l'accesso ai log dipende dalle ACL ereditate, non verificate dalla GUI."
            ));
            if sensitive {
                ui.add_enabled_ui(worker_idle, |ui| {
                    if ui
                        .checkbox(
                            &mut self.confirm_sensitive_logging,
                            presented!(
                                self,
                                DisplayControl::new("logging_sensitive_confirmation"),
                                Classification::Public,
                                "Confermo di voler registrare contenuti privati per questa sessione."
                            ),
                        )
                        .changed()
                    {
                        record_ui_action(
                            self.log_session.as_ref(),
                            &mut self.log_failure,
                            Activity::GuiConsentChanged,
                            DisplayControl::new("logging_sensitive_confirmation"),
                        );
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.can_start_logging(),
                        egui::Button::new(presented!(
                            self,
                            DisplayControl::new("logging_start"),
                            Classification::Public,
                            "Attiva / applica livello"
                        )),
                    )
                    .clicked()
                {
                    self.pending_log_action = Some(PendingLogAction::Start(self.selected_log_level));
                    ui.ctx().request_repaint();
                }
                if self.log_session.is_some()
                    && self.receiver.is_none()
                    && self.worker_handle.is_none()
                    && ui
                        .button(presented!(
                            self,
                            DisplayControl::new("logging_stop"),
                            Classification::Public,
                            "Disattiva"
                        ))
                        .clicked()
                {
                    record_ui_action(
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        Activity::GuiLoggingStop,
                        DisplayControl::new("logging_stop"),
                    );
                    self.pending_log_action = Some(PendingLogAction::Stop);
                    ui.ctx().request_repaint();
                }
            });
            if let Some(error) = &self.log_action_error {
                ui.colored_label(
                    warning_text_color(ui.visuals().dark_mode),
                    presented!(
                        self,
                        DisplayControl::new("logging_action_error"),
                        Classification::Unclassified,
                        error
                    ),
                );
            }
        });
        })
    }

    fn apply_pending_log_action(&mut self) {
        let trace_session = self.log_session.clone();
        let mut apply = || {
            let action = trace_desktop_call!(
                "desktop::DesktopApp::apply_pending_log_action",
                fields [
                    "pending_action" => trace_desktop_debug(&self.pending_log_action),
                    "log_directory" => self.log_directory.as_deref().map_or(TraceValue::data(()), trace_desktop_path),
                    "active_level" => trace_desktop_debug(&self.active_log_level),
                ],
                output trace_deferred_log_action,
                || {
                    let action = self.pending_log_action.take()?;
                    self.log_action_error = None;
                    match action {
                        PendingLogAction::Start(level) => {
                            let Some(directory) = self.log_directory.as_deref() else {
                                self.log_action_error = Some(
                                    "Cartella predefinita dei log non disponibile su questo sistema.".into(),
                                );
                                return None;
                            };
                            match Session::create_in_directory_with_level(
                                directory,
                                Component::Desktop,
                                level,
                            ) {
                                Ok((session, path)) => Some(DeferredLogSessionAction::Start {
                                    session,
                                    path,
                                    level,
                                }),
                                Err(error) => {
                                    self.log_action_error = Some(format!(
                                        "Impossibile avviare il log nella cartella selezionata: {error}"
                                    ));
                                    None
                                }
                            }
                        }
                        PendingLogAction::Stop => Some(DeferredLogSessionAction::Stop),
                    }
                }
            );

            match action {
                Some(DeferredLogSessionAction::Start {
                    session,
                    path,
                    level,
                }) => {
                    self.log_failure = false;
                    self.finish_log_session();
                    self.log_session = Some(session);
                    self.log_path = Some(path);
                    self.active_log_level = Some(level);
                    self.selected_log_level = level;
                    self.confirm_sensitive_logging = false;
                }
                Some(DeferredLogSessionAction::Stop) => {
                    self.finish_log_session();
                    self.log_path = None;
                    self.active_log_level = None;
                    self.confirm_sensitive_logging = false;
                }
                None => {}
            }
        };
        if let Some(session) = trace_session {
            session.scoped(apply);
        } else {
            apply();
        }
    }

    fn finish_log_session(&mut self) {
        let session = trace_desktop_call!(
            "desktop::DesktopApp::finish_log_session",
            fields [
                "session_active" => TraceValue::data(self.log_session.is_some()),
                "active_level" => trace_desktop_debug(&self.active_log_level),
            ],
            output |session: &Option<Session>| TraceValue::data(session.is_some()),
            || self.log_session.take()
        );
        if let Some(session) = session {
            let incomplete_before_finish = session.has_failed();
            let finish_result = session.finish(Component::Desktop);
            if finish_result.is_err() {
                self.log_failure = true;
                self.log_action_error = Some(
                    "Impossibile completare il file di log; la registrazione potrebbe essere incompleta."
                        .into(),
                );
            } else if incomplete_before_finish {
                self.log_failure = true;
                self.log_action_error =
                    Some("Registrazione interrotta o incompleta; verifica il file di log.".into());
            }
        }
    }
}

/// Collega gli eventi del core ai controlli e allo stato visuale.
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let trace_session = self.log_session.clone();
        let mut render = || {
            trace_desktop_call!("desktop::eframe::App::ui", fields [
                "ui" => trace_egui_boundary(),
                "frame" => trace_egui_boundary(),
                "app_state" => TraceValue::Omitted(TraceOmission::UnsupportedType),
                "presentation_values" => TraceValue::data("captured by PresentedText"),
            ], || {
            let capture_session = self.log_session.clone();
            if let Some(session) = &capture_session {
                if session.has_failed() {
                    self.log_failure = true;
                }
                if session.begin_display_frame().is_err() {
                    self.log_failure = true;
                }
            }
            configure_desktop_theme(ui.ctx());
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, STRAW_BACKGROUND);
            egui::Frame::NONE
                .fill(STRAW_BACKGROUND)
                .inner_margin(egui::Margin {
                    left: 16,
                    ..egui::Margin::ZERO
                })
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.show_contents(ui));
                });
            if let Some(session) = &capture_session
                && session.end_display_frame().is_err()
            {
                self.log_failure = true;
            }
            if capture_session.as_ref().is_some_and(Session::has_failed) {
                self.log_failure = true;
            }
            })
        };
        if let Some(session) = trace_session {
            session.scoped(render);
        } else {
            render();
        }
        self.apply_pending_log_action();
    }

    fn on_exit(&mut self) {
        let trace_session = self.log_session.clone();
        let mut exit = || {
            let finalize_session = trace_desktop_call!(
                "desktop::eframe::App::on_exit",
                fields [
                    "cancellation_active" => TraceValue::data(self.cancellation.is_some()),
                    "worker_active" => TraceValue::data(self.worker_handle.is_some()),
                    "session_active" => TraceValue::data(self.log_session.is_some()),
                ],
                || {
                    if let Some(cancellation) = &self.cancellation {
                        cancellation.cancel();
                    }
                    if self
                        .worker_handle
                        .as_ref()
                        .is_some_and(|worker| !worker.is_finished())
                    {
                        if let Some(session) = &self.log_session
                            && session.shutdown_pending(Component::Desktop).is_err()
                        {
                            self.log_failure = true;
                        }
                        let worker = self
                            .worker_handle
                            .take()
                            .expect("worker verificato presente");
                        if let Some(session) = self.log_session.take() {
                            thread::spawn(move || {
                                let _ = worker.join();
                                let _ = session.finish(Component::Desktop);
                            });
                        }
                        false
                    } else {
                        self.join_worker();
                        true
                    }
                }
            );
            if finalize_session {
                self.finish_log_session();
            }
        };
        if let Some(session) = trace_session {
            session.scoped(exit);
        } else {
            exit();
        }
    }
}

/// Rende l'intera vista desktop scorrevole sui viewport ridotti.
impl DesktopApp {
    fn show_contents(&mut self, ui: &mut egui::Ui) {
        trace_desktop_call!("desktop::DesktopApp::show_contents", fields [
            "ui" => trace_egui_boundary(),
            "app_state" => TraceValue::Omitted(TraceOmission::UnsupportedType),
            "profile_loaded" => TraceValue::data(self.profile.is_some()),
            "selected_people" => TraceValue::data(self.selections.iter().filter(|person| person.selected).count()),
            "presentation_values" => TraceValue::data("captured by PresentedText"),
        ], || {
        self.receive_worker_events();
        let busy = self.receiver.is_some() || self.worker_handle.is_some();
        if self.worker_handle.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        if !busy
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(Modifiers::CTRL, Key::O))
        {
            record_ui_action(
                self.log_session.as_ref(),
                &mut self.log_failure,
                Activity::GuiProfileDialogOpened,
                DisplayControl::new("open_profile_keyboard"),
            );
            self.open_profile_dialog();
        }

        let title = PresentedText::new(WINDOW_TITLE, Classification::Public);
        title.capture(
            self.log_session.as_ref(),
            &mut self.log_failure,
            DisplayControl::new("window_title"),
        );
        let heading = PresentedText::new(WINDOW_TITLE, Classification::Public);
        ui.heading(heading.capture(
            self.log_session.as_ref(),
            &mut self.log_failure,
            DisplayControl::new("heading"),
        ));
        ui.label(presented!(
            self,
            DisplayControl::new("intro"),
            Classification::Public,
            "Importa un profilo e scegli persone e cartelle per questa sessione."
        ));
        let can_edit = !busy && self.review.is_none();
        let mut open_profile = false;
        ui.add_enabled_ui(can_edit, |ui| {
            let label = ui.label(presented!(
                self,
                DisplayControl::new("profile_label"),
                Classification::Public,
                "Profilo account"
            ));
            ui.horizontal_wrapped(|ui| {
                let path_edit =
                    add_responsive_path_edit(ui, label.id, &mut self.profile_path, 180.0);
                if path_edit.changed() {
                    record_ui_action(
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        Activity::GuiProfilePathEdited,
                        DisplayControl::new("profile_path"),
                    );
                }
                open_profile = ui
                    .button(presented!(
                        self,
                        DisplayControl::new("open_profile"),
                        Classification::Public,
                        "Apri profilo… (Ctrl+O)"
                    ))
                    .clicked();
            });
        });
        let profile_text = PresentedText::new(&self.profile_path, Classification::Private);
        profile_text.capture(
            self.log_session.as_ref(),
            &mut self.log_failure,
            DisplayControl::new("profile_path"),
        );
        if open_profile {
            record_ui_action(
                self.log_session.as_ref(),
                &mut self.log_failure,
                Activity::GuiProfileDialogOpened,
                DisplayControl::new("open_profile"),
            );
            self.open_profile_dialog();
        }
        ui.label(presented!(
            self,
            DisplayControl::new("profile_unchanged"),
            Classification::Public,
            "Le scelte di questa schermata non modificano il file del profilo."
        ));
        ui.label(presented!(
            self,
            DisplayControl::new("profile_relative_paths"),
            Classification::Public,
            "I percorsi relativi digitati sono risolti rispetto alla base del profilo."
        ));
        self.show_logging_controls(ui);

        if let Some(profile) = &self.profile {
            ui.separator();
            show_captured_path(
                ui,
                "Base percorsi:",
                profile.base_directory(),
                self.log_session.as_ref(),
                &mut self.log_failure,
                DisplayControl::new("profile_base"),
            );
            if !self.selections.iter().any(|person| person.selected) {
                ui.colored_label(
                    warning_text_color(ui.visuals().dark_mode),
                    presented!(
                        self,
                        DisplayControl::new("no_person_selected"),
                        Classification::Public,
                        "Nessuna persona selezionata: selezionane almeno una."
                    ),
                );
            }
            ui.add_enabled_ui(can_edit, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (person_index, (person, selection)) in profile
                        .accounts()
                        .people
                        .iter()
                        .zip(&mut self.selections)
                        .enumerate()
                    {
                        let label = format!("{} ({})", person.display_name, person.id);
                        let presented = PresentedText::new(&label, Classification::Private);
                        let label = presented.capture(
                            self.log_session.as_ref(),
                            &mut self.log_failure,
                            DisplayControl::indexed("person_heading", person_index, None),
                        );
                        let header = egui::CollapsingHeader::new(label.to_owned())
                            .default_open(true)
                            .show(ui, |ui| {
                                let label = presented!(
                                    self,
                                    DisplayControl::indexed("include_person", person_index, None),
                                    Classification::Public,
                                    "Includi questa persona"
                                );
                                if ui.checkbox(&mut selection.selected, label).changed() {
                                    record_ui_action(
                                        self.log_session.as_ref(),
                                        &mut self.log_failure,
                                        Activity::GuiPersonSelection,
                                        DisplayControl::indexed(
                                            "include_person",
                                            person_index,
                                            None,
                                        ),
                                    );
                                }
                                choose_folder(
                                    ui,
                                    "Documenti",
                                    &mut selection.source_documents,
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed("source_documents", person_index, None),
                                );
                                choose_folder(
                                    ui,
                                    "Metadati",
                                    &mut selection.metadata_directory,
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed(
                                        "metadata_directory",
                                        person_index,
                                        None,
                                    ),
                                );
                                choose_folder(
                                    ui,
                                    "Destinazione build",
                                    &mut selection.local_build,
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed("local_build", person_index, None),
                                );
                            });
                        if header.header_response.clicked() {
                            record_ui_action(
                                self.log_session.as_ref(),
                                &mut self.log_failure,
                                Activity::GuiPanelToggled,
                                DisplayControl::indexed("person_panel", person_index, None),
                            );
                        }
                    }
                });
            });
        } else {
            ui.colored_label(
                warning_text_color(ui.visuals().dark_mode),
                presented!(
                    self,
                    DisplayControl::new("profile_not_loaded"),
                    Classification::Public,
                    "Profilo non caricato."
                ),
            );
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let selected = self.selections.iter().any(|person| person.selected);
            if ui
                .add_enabled(
                    !busy && self.profile.is_some() && selected && self.review.is_none(),
                    egui::Button::new(presented!(
                        self,
                        DisplayControl::new("start_preflight"),
                        Classification::Public,
                        "Controlla preflight e anomalie"
                    )),
                )
                .clicked()
            {
                self.start_preflight();
            }
            if self.review.is_some() && !busy {
                if ui
                    .button(presented!(
                        self,
                        DisplayControl::new("edit_selection"),
                        Classification::Public,
                        "Modifica selezione"
                    ))
                    .clicked()
                {
                    record_ui_action(
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        Activity::GuiReviewEdit,
                        DisplayControl::new("edit_selection"),
                    );
                    self.review = None;
                    self.aifa_people = None;
                }
                if ui
                    .button(presented!(
                        self,
                        DisplayControl::new("repeat_preflight"),
                        Classification::Public,
                        "Rifai preflight"
                    ))
                    .clicked()
                {
                    self.review = None;
                    self.aifa_people = None;
                    self.start_preflight();
                }
                if ui
                    .add_enabled(
                        selected,
                        egui::Button::new(presented!(
                            self,
                            DisplayControl::new("import_observations"),
                            Classification::Public,
                            "Importa/aggiorna osservazioni"
                        )),
                    )
                    .clicked()
                {
                    self.start_observation_import();
                }
                if ui
                    .add_enabled(
                        selected,
                        egui::Button::new(presented!(
                            self,
                            DisplayControl::new("review_aifa"),
                            Classification::Public,
                            "Rivedi riferimenti AIFA"
                        )),
                    )
                    .clicked()
                {
                    self.start_aifa_review();
                }
                if ui
                    .add_enabled(
                        selected,
                        egui::Button::new(presented!(
                            self,
                            DisplayControl::new("start_build"),
                            Classification::Public,
                            "Avvia build incrementale"
                        )),
                    )
                    .clicked()
                {
                    self.start_build();
                }
            }
            if let Some(cancellation) = &self.cancellation
                && ui
                    .button(presented!(
                        self,
                        DisplayControl::new("cancel_build"),
                        Classification::Public,
                        "Annulla dopo la persona corrente"
                    ))
                    .clicked()
            {
                cancellation.cancel();
                self.status = "Annullamento richiesto; attendo il punto sicuro.".to_owned();
            }
        });

        #[cfg(target_os = "linux")]
        if !self.completed_builds.is_empty() && self.receiver.is_none() {
            ui.separator();
            ui.heading(presented!(
                self,
                DisplayControl::new("usb_heading"),
                Classification::Public,
                "Export USB Linux"
            ));
            if let Some(image) = &self.usb_image {
                ui.label(presented!(
                    self,
                    DisplayControl::new("usb_image_summary"),
                    Classification::Private,
                    format!(
                        "Immagine verificata: {} persone · {} file · {} byte",
                        image.patients, image.files, image.bytes
                    )
                ));
                show_captured_path(
                    ui,
                    "Percorso immagine:",
                    &image.root,
                    self.log_session.as_ref(),
                    &mut self.log_failure,
                    DisplayControl::new("usb_image"),
                );
            } else if ui
                .button(presented!(
                    self,
                    DisplayControl::new("prepare_usb"),
                    Classification::Public,
                    "Prepara e valida immagine USB"
                ))
                .clicked()
            {
                self.prepare_usb_image();
            }
            if self.usb_image.is_some() {
                if ui
                    .button(presented!(
                        self,
                        DisplayControl::new("choose_usb"),
                        Classification::Public,
                        "Scegli volume Linux…"
                    ))
                    .clicked()
                {
                    self.choose_usb_target();
                }
                if let Some(target) = &self.usb_target {
                    show_captured_path(
                        ui,
                        "Volume selezionato:",
                        target,
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        DisplayControl::new("usb_target"),
                    );
                    show_captured_path(
                        ui,
                        "Cartella di export:",
                        &target.join("SaniKey"),
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        DisplayControl::new("usb_export"),
                    );
                }
                if let Some(volume) = &self.usb_volume {
                    show_captured_path(
                        ui,
                        "Mount del volume:",
                        volume
                            .mount_target
                            .as_deref()
                            .unwrap_or(Path::new("sconosciuto")),
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        DisplayControl::new("usb_mount"),
                    );
                    ui.label(presented!(
                        self,
                        DisplayControl::new("usb_volume_summary"),
                        Classification::Private,
                        format!(
                            "Filesystem: {} · UUID: {} · liberi: {} byte",
                            volume.filesystem.as_deref().unwrap_or("sconosciuto"),
                            volume.uuid.as_deref().unwrap_or("sconosciuto"),
                            volume.available_bytes
                        )
                    ));
                    ui.colored_label(warning_text_color(ui.visuals().dark_mode),
                        presented!(self, DisplayControl::new("usb_warning"), Classification::Public, "La copia aggiorna la cartella SaniKey nel volume e rimuove da quella cartella i file che non appartengono all'immagine. Gli altri file del volume restano intatti."));
                    let confirmation = presented!(
                        self,
                        DisplayControl::new("usb_confirmation"),
                        Classification::Public,
                        "Confermo che il volume mostrato è quello corretto e autorizzo l'aggiornamento della cartella SaniKey."
                    );
                    if ui
                        .checkbox(&mut self.confirm_usb_target, confirmation)
                        .changed()
                    {
                        record_ui_action(
                            self.log_session.as_ref(),
                            &mut self.log_failure,
                            Activity::GuiUsbConfirmation,
                            DisplayControl::new("usb_confirmation"),
                        );
                    }
                    if ui
                        .add_enabled(
                            self.confirm_usb_target && self.receiver.is_none(),
                            egui::Button::new(presented!(
                                self,
                                DisplayControl::new("copy_usb"),
                                Classification::Public,
                                "Copia, verifica e mostra il risultato"
                            )),
                        )
                        .clicked()
                    {
                        self.copy_usb_image();
                    }
                }
                if let Some(copy) = &self.usb_copy {
                    ui.label(presented!(self, DisplayControl::new("usb_copy_report"), Classification::Private, format!("Report copia: {} copiati · {} riutilizzati · {} rimossi · {} byte scritti · target validato.", copy.copied_files, copy.reused_files, copy.removed_files, copy.bytes_copied)));
                }
            }
        }

        if let Some(review) = &self.review {
            ui.heading(presented!(
                self,
                DisplayControl::new("preflight_heading"),
                Classification::Public,
                "Riepilogo preflight"
            ));
            ui.label(presented!(
                self,
                DisplayControl::new("preflight_summary"),
                Classification::Private,
                format!(
                    "Persone: {} · Documenti: {} · Gruppi duplicati: {}",
                    review.selected_people.len(),
                    review.document_count,
                    review.duplicate_groups.len()
                )
            ));
            let duplicate_header = egui::CollapsingHeader::new(presented!(
                self,
                DisplayControl::new("duplicate_heading"),
                Classification::Public,
                "Rivedi i documenti duplicati"
            ))
            .default_open(!review.duplicate_groups.is_empty())
            .show(ui, |ui| {
                if review.duplicate_groups.is_empty() {
                    ui.label(presented!(
                        self,
                        DisplayControl::new("no_duplicates"),
                        Classification::Public,
                        "Nessun duplicato rilevato."
                    ));
                }
                for (group_index, (person, digest, paths)) in
                    review.duplicate_groups.iter().enumerate()
                {
                    ui.label(presented!(
                        self,
                        DisplayControl::indexed("duplicate_group", group_index, None),
                        Classification::Private,
                        format!("{person} · SHA-256 {digest}")
                    ));
                    for (path_index, path) in paths.iter().enumerate() {
                        show_captured_path(
                            ui,
                            "Documento:",
                            path,
                            self.log_session.as_ref(),
                            &mut self.log_failure,
                            DisplayControl::indexed(
                                "duplicate_document",
                                group_index,
                                Some(path_index),
                            ),
                        );
                    }
                }
            });
            if duplicate_header.header_response.clicked() {
                record_ui_action(
                    self.log_session.as_ref(),
                    &mut self.log_failure,
                    Activity::GuiPanelToggled,
                    DisplayControl::new("duplicate_panel"),
                );
            }
            ui.label(presented!(self, DisplayControl::new("observations_hint"), Classification::Public, "Importa o aggiorna le osservazioni prima della build; i metadati curati restano invariati."));
        }
        if let Some(people) = &mut self.aifa_people {
            ui.separator();
            ui.heading(presented!(
                self,
                DisplayControl::new("aifa_heading"),
                Classification::Public,
                "Riferimenti farmaceutici AIFA"
            ));
            ui.label(presented!(self, DisplayControl::new("aifa_intro"), Classification::Public, "Le ricerche online vengono eseguite fuori dalla finestra. Il riferimento si salva solo dopo conferma e verifica di FI e RCP."));
            let mut action = None;
            for (person_index, person) in people.iter_mut().enumerate() {
                let heading = format!(
                    "{} ({}) · {} farmaci",
                    person.display_name,
                    person.id,
                    person.medications.len()
                );
                let heading = PresentedText::new(&heading, Classification::Private);
                let heading = heading.capture(
                    self.log_session.as_ref(),
                    &mut self.log_failure,
                    DisplayControl::indexed("aifa_person_heading", person_index, None),
                );
                let header = egui::CollapsingHeader::new(heading.to_owned())
                    .default_open(true)
                    .show(ui, |ui| {
                        for (medication_index, item) in person.medications.iter_mut().enumerate() {
                            ui.group(|ui| {
                                let name = PresentedText::new(
                                    &item.medication.name,
                                    Classification::Private,
                                );
                                ui.heading(name.capture(
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed(
                                        "aifa_medication_name",
                                        person_index,
                                        Some(medication_index),
                                    ),
                                ));
                                let medication_id = format!("ID: {}", item.medication.id);
                                let medication_id =
                                    PresentedText::new(&medication_id, Classification::Private);
                                ui.label(medication_id.capture(
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed(
                                        "aifa_medication_id",
                                        person_index,
                                        Some(medication_index),
                                    ),
                                ));
                                for (therapy_index, therapy) in person
                                    .metadata
                                    .therapies
                                    .iter()
                                    .filter(|therapy| therapy.medication_id == item.medication.id)
                                    .enumerate()
                                {
                                    let text = format!(
                                        "Terapia curata: {} · {} · {}",
                                        therapy.role.as_deref().unwrap_or("ruolo non indicato"),
                                        therapy
                                            .dosage
                                            .as_deref()
                                            .unwrap_or("posologia non indicata"),
                                        therapy.schedule.join(", ")
                                    );
                                    let text = PresentedText::new(&text, Classification::Private);
                                    ui.label(text.capture(
                                        self.log_session.as_ref(),
                                        &mut self.log_failure,
                                        DisplayControl::nested(
                                            "aifa_therapy",
                                            person_index,
                                            medication_index,
                                            therapy_index,
                                        ),
                                    ));
                                    if let Some(instructions) = &therapy.instructions {
                                        let text = format!("Istruzioni: {instructions}");
                                        let text =
                                            PresentedText::new(&text, Classification::Unclassified);
                                        ui.label(text.capture(
                                            self.log_session.as_ref(),
                                            &mut self.log_failure,
                                            DisplayControl::nested(
                                                "aifa_instructions",
                                                person_index,
                                                medication_index,
                                                therapy_index,
                                            ),
                                        ));
                                    }
                                }
                                if let Some(reference) = person
                                    .metadata
                                    .medication_leaflets
                                    .iter()
                                    .find(|reference| reference.medication_id == item.medication.id)
                                {
                                    let text = format!(
                                        "Riferimento registrato: AIFA {}/{}",
                                        reference.codice_sis, reference.aic6
                                    );
                                    let text = PresentedText::new(&text, Classification::Private);
                                    ui.label(text.capture(
                                        self.log_session.as_ref(),
                                        &mut self.log_failure,
                                        DisplayControl::indexed(
                                            "aifa_saved_reference",
                                            person_index,
                                            Some(medication_index),
                                        ),
                                    ));
                                }
                                if person
                                    .metadata
                                    .medication_leaflet_exclusions
                                    .iter()
                                    .any(|exclusion| exclusion.medication_id == item.medication.id)
                                {
                                    ui.label(presented!(
                                        self,
                                        DisplayControl::indexed(
                                            "aifa_not_applicable",
                                            person_index,
                                            Some(medication_index)
                                        ),
                                        Classification::Public,
                                        "Stato registrato: non applicabile ad AIFA."
                                    ));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    let label = ui.label(presented!(
                                        self,
                                        DisplayControl::indexed(
                                            "aifa_query_label",
                                            person_index,
                                            Some(medication_index)
                                        ),
                                        Classification::Public,
                                        "Ricerca manuale"
                                    ));
                                    if ui
                                        .text_edit_singleline(&mut item.query)
                                        .labelled_by(label.id)
                                        .changed()
                                    {
                                        record_ui_action(
                                            self.log_session.as_ref(),
                                            &mut self.log_failure,
                                            Activity::GuiAifaQueryEdited,
                                            DisplayControl::indexed(
                                                "aifa_manual_query",
                                                person_index,
                                                Some(medication_index),
                                            ),
                                        );
                                    }
                                    PresentedText::new(&item.query, Classification::Unclassified)
                                        .capture(
                                            self.log_session.as_ref(),
                                            &mut self.log_failure,
                                            DisplayControl::indexed(
                                                "aifa_manual_query",
                                                person_index,
                                                Some(medication_index),
                                            ),
                                        );
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            egui::Button::new(presented!(
                                                self,
                                                DisplayControl::indexed(
                                                    "aifa_search_auto",
                                                    person_index,
                                                    Some(medication_index)
                                                ),
                                                Classification::Public,
                                                "Cerca nome e principio attivo"
                                            )),
                                        )
                                        .clicked()
                                    {
                                        action = Some(AifaUiAction::Search(
                                            person_index,
                                            medication_index,
                                            false,
                                        ));
                                    }
                                    if ui
                                        .add_enabled(
                                            !busy && !item.query.trim().is_empty(),
                                            egui::Button::new(presented!(
                                                self,
                                                DisplayControl::indexed(
                                                    "aifa_search_manual",
                                                    person_index,
                                                    Some(medication_index)
                                                ),
                                                Classification::Public,
                                                "Cerca testo"
                                            )),
                                        )
                                        .clicked()
                                    {
                                        action = Some(AifaUiAction::Search(
                                            person_index,
                                            medication_index,
                                            true,
                                        ));
                                    }
                                });
                                if let Some(search) = &item.search {
                                    let candidates = candidates_for_review(search);
                                    if candidates.is_empty() {
                                        ui.label(presented!(
                                            self,
                                            DisplayControl::indexed(
                                                "aifa_no_candidates",
                                                person_index,
                                                Some(medication_index)
                                            ),
                                            Classification::Public,
                                            "Nessun candidato da confermare."
                                        ));
                                    }
                                    for (candidate_index, candidate) in
                                        candidates.iter().enumerate()
                                    {
                                        let label = format!(
                                            "{} · {} · {}",
                                            candidate.title,
                                            candidate
                                                .strength
                                                .as_deref()
                                                .unwrap_or("dosaggio non indicato"),
                                            candidate
                                                .form
                                                .as_deref()
                                                .unwrap_or("forma non indicata")
                                        );
                                        let label =
                                            PresentedText::new(&label, Classification::Private);
                                        let label = label.capture(
                                            self.log_session.as_ref(),
                                            &mut self.log_failure,
                                            DisplayControl::nested(
                                                "aifa_candidate",
                                                person_index,
                                                medication_index,
                                                candidate_index,
                                            ),
                                        );
                                        if ui
                                            .selectable_value(
                                                &mut item.selected_candidate,
                                                Some(candidate_index),
                                                label.to_owned(),
                                            )
                                            .changed()
                                        {
                                            record_ui_action(
                                                self.log_session.as_ref(),
                                                &mut self.log_failure,
                                                Activity::GuiAifaCandidate,
                                                DisplayControl::nested(
                                                    "aifa_candidate",
                                                    person_index,
                                                    medication_index,
                                                    candidate_index,
                                                ),
                                            );
                                        }
                                    }
                                    if let Some(candidate) = item
                                        .selected_candidate
                                        .and_then(|index| candidates.get(index))
                                    {
                                        show_aifa_candidate(
                                            ui,
                                            candidate,
                                            self.log_session.as_ref(),
                                            &mut self.log_failure,
                                            person_index,
                                            medication_index,
                                        );
                                        if ui
                                            .add_enabled(
                                                !busy,
                                                egui::Button::new(presented!(
                                                    self,
                                                    DisplayControl::indexed(
                                                        "aifa_confirm_candidate",
                                                        person_index,
                                                        Some(medication_index)
                                                    ),
                                                    Classification::Public,
                                                    "Conferma candidato e verifica FI/RCP"
                                                )),
                                            )
                                            .clicked()
                                        {
                                            action = Some(AifaUiAction::Confirm(
                                                person_index,
                                                medication_index,
                                                Box::new(candidate.clone()),
                                            ));
                                        }
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        !busy,
                                        egui::Button::new(presented!(
                                            self,
                                            DisplayControl::indexed(
                                                "aifa_confirm_exclusion",
                                                person_index,
                                                Some(medication_index)
                                            ),
                                            Classification::Public,
                                            "Conferma stato non AIFA"
                                        )),
                                    )
                                    .clicked()
                                {
                                    action = Some(AifaUiAction::MarkNotApplicable(
                                        person_index,
                                        medication_index,
                                    ));
                                }
                                let status =
                                    PresentedText::new(&item.status, Classification::Unclassified);
                                ui.label(status.capture(
                                    self.log_session.as_ref(),
                                    &mut self.log_failure,
                                    DisplayControl::indexed(
                                        "aifa_medication_status",
                                        person_index,
                                        Some(medication_index),
                                    ),
                                ));
                            });
                        }
                    });
                if header.header_response.clicked() {
                    record_ui_action(
                        self.log_session.as_ref(),
                        &mut self.log_failure,
                        Activity::GuiPanelToggled,
                        DisplayControl::indexed("aifa_person_panel", person_index, None),
                    );
                }
            }
            match action {
                Some(AifaUiAction::Search(person, medication, manual)) => {
                    self.start_aifa_search(person, medication, manual);
                }
                Some(AifaUiAction::Confirm(person, medication, candidate)) => {
                    self.start_aifa_decision(person, medication, AifaDecision::Confirm(candidate));
                }
                Some(AifaUiAction::MarkNotApplicable(person, medication)) => {
                    self.start_aifa_decision(person, medication, AifaDecision::MarkNotApplicable);
                }
                None => {}
            }
        }
        if let Some(summary) = &self.observation_import {
            let result_panel = ui.collapsing(
                presented!(
                    self,
                    DisplayControl::new("observation_result_heading"),
                    Classification::Public,
                    "Risultato importazione osservazioni"
                ),
                |ui| {
                    ui.label(presented!(
                        self,
                        DisplayControl::new("observation_result_summary"),
                        Classification::Private,
                        format!(
                            "{} persone · {} serie · {} osservazioni · {} avvisi",
                            summary.people,
                            summary.series,
                            summary.points,
                            summary.warnings.len()
                        )
                    ));
                    for (warning_index, warning) in summary.warnings.iter().enumerate() {
                        let warning = presented!(
                            self,
                            DisplayControl::indexed("observation_warning", warning_index, None),
                            Classification::Unclassified,
                            warning
                        );
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(warning)
                                    .color(warning_text_color(ui.visuals().dark_mode)),
                            )
                            .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                    }
                },
            );
            if result_panel.header_response.clicked() {
                record_ui_action(
                    self.log_session.as_ref(),
                    &mut self.log_failure,
                    Activity::GuiPanelToggled,
                    DisplayControl::new("observation_result_panel"),
                );
            }
        }
        if !self.completed_people.is_empty() {
            ui.label(presented!(
                self,
                DisplayControl::new("completed_people"),
                Classification::Private,
                format!("Build completate: {}", self.completed_people.join(", "))
            ));
        }
        if self.receiver.is_some() || self.cancellation.is_some() {
            if let Some(progress) = self.progress {
                let percentage = format!("{}%", (progress.clamp(0.0, 1.0) * 100.0) as usize);
                PresentedText::new(&percentage, Classification::Public).capture(
                    self.log_session.as_ref(),
                    &mut self.log_failure,
                    DisplayControl::new("build_progress"),
                );
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            } else {
                indeterminate_progress_bar(ui);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        ui.separator();
        let status = PresentedText::new(&self.status, Classification::Unclassified);
        let displayed = status.capture(
            self.log_session.as_ref(),
            &mut self.log_failure,
            DisplayControl::new("status"),
        );
        ui.add(egui::Label::new(displayed).wrap_mode(egui::TextWrapMode::Wrap));
        if self.log_failure {
            ui.colored_label(
                warning_text_color(ui.visuals().dark_mode),
                presented!(
                    self,
                    DisplayControl::new("log_failure_warning"),
                    Classification::Public,
                    "Registrazione interrotta o incompleta; verifica il file di log."
                ),
            );
        }
        })
    }
}

fn indeterminate_progress_bar(ui: &mut egui::Ui) {
    trace_desktop_call!("desktop::indeterminate_progress_bar", fields ["ui" => trace_egui_boundary()], || {
    let width = ui.available_size_before_wrap().x.max(96.0);
    let height = ui.spacing().interact_size.y * 0.5;
    let (track, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    if !ui.is_rect_visible(track) {
        return;
    }

    let visuals = ui.visuals();
    let radius = height * 0.5;
    ui.painter()
        .rect_filled(track, radius, visuals.extreme_bg_color);
    let time = ui.input(|input| input.time);
    let segment = indeterminate_progress_segment(track, time);
    ui.painter()
        .with_clip_rect(track)
        .rect_filled(segment, radius, visuals.selection.bg_fill);
    })
}

fn indeterminate_progress_segment(track: egui::Rect, time: f64) -> egui::Rect {
    trace_desktop_call!("desktop::indeterminate_progress_segment", fields [
        "track" => trace_desktop_debug(&track),
        "time_seconds" => trace_desktop_debug(&time),
    ], || {
    let segment_width = track.width() * 0.28;
    let phase = (time / 1.2).rem_euclid(2.0);
    let position = (if phase <= 1.0 { phase } else { 2.0 - phase }) as f32;
    let left = track.left() + (track.width() - segment_width) * position;
    egui::Rect::from_min_size(
        egui::pos2(left, track.top()),
        egui::vec2(segment_width, track.height()),
    )
    })
}

fn main() -> eframe::Result {
    eframe::run_native(
        WINDOW_TITLE,
        eframe::NativeOptions::default(),
        Box::new(|creation_context| {
            configure_desktop_theme(&creation_context.egui_ctx);
            Ok(Box::<DesktopApp>::default())
        }),
    )
}

#[cfg(test)]
mod tests {
    //! Test di stato, accessibilità e flussi sintetici della finestra desktop.

    use super::{
        AifaCandidate, AifaCandidateSearch, AifaDecision, AifaMedicationReview, AifaPersonReview,
        BLUE_TEXT, DesktopApp, DesktopEvent, DisplayControl, FIELD_BACKGROUND,
        LeafletDownloadFailure, ReviewSummary, STRAW_BACKGROUND, add_responsive_path_edit,
        build_selected_people, build_selected_people_with, configure_desktop_theme,
        desktop_visuals, import_selected_observations, indeterminate_progress_segment,
        load_aifa_people, record_native_dialog_omission, record_ui_action, review_profile,
        save_aifa_decision_with, selection_from_person, show_path, spawn_logged_worker,
        trace_desktop_path, warning_text_color,
    };
    use eframe::egui;
    use sanikey_core::build::{BuildCounts, BuildOptions, PatientBuildResult, build_patient};
    use sanikey_core::config::AccountsProfile;
    use sanikey_core::metadata::{
        CuratedMetadata, Medication, MedicationLeafletExclusion, load_curated_metadata,
    };
    use sanikey_core::operation::{CancellationToken, OperationEvent};
    use std::{fs, path::PathBuf, thread, time::SystemTime};

    struct TestDirectory(PathBuf);

    /// Gestisce la directory temporanea usata dai test desktop.
    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("orologio successivo all'epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("sanikey-desktop-{nonce}"));
            fs::create_dir_all(&path).expect("directory temporanea");
            Self(path)
        }
    }

    /// Elimina gli artefatti temporanei del test desktop.
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile_with_document(directory: &TestDirectory) -> AccountsProfile {
        let source = directory.0.join("patients/patient-a/documents");
        let metadata = directory.0.join("patients/patient-a/metadata");
        let output = directory.0.join("generated/patient-a");
        fs::create_dir_all(&source).expect("documenti sintetici");
        fs::create_dir_all(&metadata).expect("metadati sintetici");
        fs::write(source.join("one.txt"), "documento sintetico").expect("primo documento");
        fs::write(source.join("two.txt"), "documento sintetico").expect("duplicato sintetico");
        let config_directory = directory.0.join("config");
        fs::create_dir_all(&config_directory).expect("config sintetica");
        let profile_path = config_directory.join("accounts.toml");
        let contents = format!(
            "[global]\nconfig_version = 1\n[[person]]\nid = \"patient-a\"\ndisplay_name = \"Persona A\"\nsource_documents = \"{}\"\nmetadata_directory = \"{}\"\nlocal_build = \"{}\"\nusb_uuid = \"1A2B-3C4D\"\nenabled = true\n",
            source.display(),
            metadata.display(),
            output.display(),
        );
        fs::write(&profile_path, contents).expect("profilo sintetico");
        AccountsProfile::import(profile_path).expect("profilo valido")
    }

    fn collect_events(receiver: std::sync::mpsc::Receiver<DesktopEvent>) -> Vec<DesktopEvent> {
        receiver.into_iter().collect()
    }

    fn run_app_frame(
        app: &mut DesktopApp,
        context: &egui::Context,
        frame: &mut eframe::Frame,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| eframe::App::ui(app, ui, frame),
        )
    }

    #[test]
    fn diagnostic_session_captures_changed_status_and_profile_field() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            status: "primo messaggio sintetico".to_owned(),
            profile_path: "/profilo/sintetico".to_owned(),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..2 {
            run_app_frame(&mut app, &context, &mut frame, Vec::new())
                .drop_without_applying_deltas();
        }
        app.status = "secondo messaggio sintetico".to_owned();
        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        let output = fs::read_to_string(log_path).expect("log sintetico");
        assert_eq!(output.matches("primo messaggio sintetico").count(), 1);
        assert_eq!(output.matches("secondo messaggio sintetico").count(), 1);
        assert!(output.contains("/profilo/sintetico"));
        assert!(!app.log_failure);
    }

    #[test]
    fn logging_is_off_by_default_and_private_levels_require_confirmation() {
        use sanikey_core::logging::Level;

        let mut app = DesktopApp::default();
        assert!(app.log_session.is_none());
        assert!(app.active_log_level.is_none());
        assert_eq!(app.selected_log_level, Level::Events);
        assert!(app.can_start_logging());

        app.selected_log_level = Level::Diagnostic;
        assert!(!app.can_start_logging());
        app.confirm_sensitive_logging = true;
        assert!(app.can_start_logging());

        app.selected_log_level = Level::Trace;
        app.confirm_sensitive_logging = false;
        assert!(!app.can_start_logging());
    }

    #[test]
    fn logging_controls_show_levels_path_and_sensitive_confirmation() {
        use sanikey_core::logging::Level;

        let directory = TestDirectory::new();
        let mut app = DesktopApp {
            log_directory: Some(directory.0.clone()),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();

        let first = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let first_tree = first
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit iniziale");
        let first_text = first_tree
            .nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value()])
            .flatten()
            .collect::<Vec<_>>();
        assert!(first_text.contains(&"Eventi"));
        assert!(first_text.contains(&"Diagnostica"));
        assert!(first_text.contains(&"Traccia completa"));
        assert!(
            first_text
                .iter()
                .any(|text| text.contains(directory.0.to_string_lossy().as_ref()))
        );
        first.drop_without_applying_deltas();
        assert!(app.log_session.is_none());

        app.selected_log_level = Level::Diagnostic;
        let second = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let second_tree = second
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit diagnostica");
        assert!(second_tree.nodes.iter().any(|(_, node)| {
            node.label().is_some_and(|label| {
                label.contains("Confermo di voler registrare contenuti privati")
            })
        }));
        second.drop_without_applying_deltas();
        assert!(app.log_session.is_none());
    }

    #[test]
    fn logging_session_switch_and_stop_finish_after_display_frame() {
        use sanikey_core::logging::Level;

        let directory = TestDirectory::new();
        let mut app = DesktopApp {
            log_directory: Some(directory.0.clone()),
            pending_log_action: Some(super::PendingLogAction::Start(Level::Events)),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();

        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        let events_path = app.log_path.clone().expect("percorso log eventi");
        assert_eq!(app.active_log_level, Some(Level::Events));

        app.selected_log_level = Level::Diagnostic;
        app.confirm_sensitive_logging = true;
        app.pending_log_action = Some(super::PendingLogAction::Start(Level::Diagnostic));
        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        let diagnostic_path = app.log_path.clone().expect("percorso log diagnostico");
        assert_ne!(events_path, diagnostic_path);
        assert_eq!(app.active_log_level, Some(Level::Diagnostic));
        let ended_events = fs::read_to_string(events_path).expect("log eventi terminato");
        assert!(ended_events.contains("\"event\":\"finished\""));

        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        app.pending_log_action = Some(super::PendingLogAction::Stop);
        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        assert!(app.log_session.is_none());
        assert!(app.active_log_level.is_none());
        let ended_diagnostic =
            fs::read_to_string(diagnostic_path).expect("log diagnostico terminato");
        assert!(ended_diagnostic.contains("logging_heading"));
        assert!(ended_diagnostic.contains("\"event\":\"finished\""));
    }

    #[test]
    fn logging_session_finishes_when_desktop_exits() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            log_path: Some(path.clone()),
            active_log_level: Some(Level::Events),
            ..DesktopApp::default()
        };

        eframe::App::on_exit(&mut app);

        assert!(app.log_session.is_none());
        let log = fs::read_to_string(path).expect("log terminato in uscita");
        assert!(log.contains("\"event\":\"finished\""));
    }

    #[test]
    fn desktop_exit_defers_finished_until_worker_terminates() {
        use sanikey_core::logging::{Component, DisplayControl, Level, Session};

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let worker_session = session.clone();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let worker_handle = thread::spawn(move || {
            let _ = release_receiver.recv_timeout(std::time::Duration::from_millis(150));
            worker_session
                .record_displayed_text(
                    DisplayControl::new("worker_finishing_text"),
                    Some("valore sintetico dopo attesa"),
                )
                .expect("scrittura worker prima della chiusura");
        });
        let mut app = DesktopApp {
            log_session: Some(session),
            worker_handle: Some(worker_handle),
            ..DesktopApp::default()
        };

        eframe::App::on_exit(&mut app);

        let before_release = fs::read_to_string(&path).expect("log sintetico aperto");
        assert!(!before_release.contains("\"event\":\"finished\""));
        assert!(before_release.contains("\"event\":\"shutdown_pending\""));
        release_sender.send(()).expect("rilascio worker sintetico");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let log = loop {
            let contents = fs::read_to_string(&path).expect("log sintetico");
            if contents.contains("\"event\":\"finished\"") {
                break contents;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "chiusura differita assente"
            );
            thread::sleep(std::time::Duration::from_millis(10));
        };
        let worker_record = log
            .find("worker_finishing_text")
            .expect("record worker presente");
        let finished_event = log
            .find("\"event\":\"finished\"")
            .expect("evento di fine sessione");
        assert!(worker_record < finished_event);
    }

    #[test]
    fn unfinished_worker_does_not_block_a_desktop_frame() {
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            started_sender.send(()).expect("worker avviato");
            let _ = release_receiver.recv();
        });
        started_receiver.recv().expect("worker in attesa");
        let app = DesktopApp {
            worker_handle: Some(worker),
            ..DesktopApp::default()
        };
        let (completed_sender, completed_receiver) = std::sync::mpsc::channel();
        let frame = thread::spawn(move || {
            let mut app = app;
            app.receive_worker_events();
            completed_sender
                .send(app.worker_handle.is_some())
                .expect("frame completato");
        });

        let completed = completed_receiver.recv_timeout(std::time::Duration::from_millis(500));
        release_sender.send(()).expect("rilascio worker sintetico");
        frame.join().expect("frame headless terminato");
        assert_eq!(completed, Ok(true));
    }

    #[test]
    fn trace_session_is_scoped_into_desktop_workers() {
        use sanikey_core::logging::{Component, Level, Session, TraceValue, trace_current};

        let directory = TestDirectory::new();
        let (session, path) =
            Session::create_in_directory_with_level(&directory.0, Component::Desktop, Level::Trace)
                .expect("sessione sintetica");
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = spawn_logged_worker(None, Some(session.trace_link()), move |_| {
            let span = trace_current(Component::Desktop, "synthetic_desktop_worker", Vec::new)
                .expect("trace corrente nel worker");
            span.finish(TraceValue::Secret)
                .expect("chiusura trace sintetica");
            sender.send(()).expect("segnale worker completato");
            None
        });
        receiver
            .recv()
            .expect("worker terminato prima della lettura");
        worker.join().expect("worker terminato senza panic");
        session.finish(Component::Desktop).expect("fine sessione");

        let log = fs::read_to_string(path).expect("log sintetico");
        assert!(log.contains("synthetic_desktop_worker"));
    }

    #[test]
    fn trace_session_records_view_and_shutdown_without_repeating_or_leaking_status() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, path) =
            Session::create_in_directory_with_level(&directory.0, Component::Desktop, Level::Trace)
                .expect("sessione Trace sintetica");
        let mut app = DesktopApp {
            log_session: Some(session.clone()),
            status: "Stato sintetico\napi_key = \"synthetic-view-secret\"".to_owned(),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();
        eframe::App::on_exit(&mut app);
        drop(app);

        let contents = fs::read_to_string(path).expect("log Trace sintetico");
        assert!(contents.contains("desktop::eframe::App::ui"));
        assert!(contents.contains("desktop::DesktopApp::show_contents"));
        assert!(contents.contains("desktop::eframe::App::on_exit"));
        assert!(contents.contains("desktop::DesktopApp::finish_log_session"));
        assert!(contents.contains("\"type\":\"function_enter\""));
        assert!(contents.contains("\"type\":\"function_exit\""));
        assert!(contents.contains("\"event\":\"finished\""));
        let status_capture_count = contents
            .lines()
            .filter(|line| {
                line.contains("\"type\":\"displayed_text\"")
                    && line.contains("\"control\":\"status\"")
                    && line.contains("Stato sintetico")
            })
            .count();
        assert_eq!(status_capture_count, 1);
        assert!(contents.contains("secret_omitted"));
        assert!(!contents.contains("synthetic-view-secret"));
        assert!(!contents.contains("\"outcome\":\"unfinished\""));
    }

    #[test]
    fn events_level_records_typed_worker_lifecycle_without_private_values() {
        use sanikey_core::logging::{
            Activity, Component, FailureCategory, Level, Outcome, Session,
        };

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            ..DesktopApp::default()
        };
        let private_value = "dato privato sintetico";
        app.launch_operation_worker(Activity::Preflight, move |_| {
            assert!(Session::current().is_some());
            assert_eq!(private_value, "dato privato sintetico");
            Outcome::FailedWith(FailureCategory::Privacy)
        });
        app.worker_handle
            .take()
            .expect("worker avviato")
            .join()
            .expect("worker terminato");
        eframe::App::on_exit(&mut app);

        let log = fs::read_to_string(path).expect("log sintetico");
        assert!(log.contains("\"activity\":\"preflight\""));
        assert!(log.contains("\"event\":\"started\""));
        assert!(log.contains("\"event\":\"failed\""));
        assert!(log.contains("\"failure_category\":\"privacy\""));
        assert!(log.contains("\"type\":\"operation_phase\""));
        assert!(log.contains("\"duration_ms\":"));
        assert!(!log.contains("dato privato sintetico"));
    }

    #[test]
    fn events_level_distinguishes_private_free_gui_action_categories() {
        use sanikey_core::logging::{Activity, Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        let activities = [
            (Activity::GuiProfilePathEdited, "gui_profile_path_edited"),
            (Activity::GuiFolderEdited, "gui_folder_edited"),
            (Activity::GuiFolderChosen, "gui_folder_chosen"),
            (Activity::GuiPersonSelection, "gui_person_selection"),
            (Activity::GuiReviewEdit, "gui_review_edit"),
            (Activity::GuiUsbConfirmation, "gui_usb_confirmation"),
            (Activity::GuiAifaCandidate, "gui_aifa_candidate"),
            (
                Activity::GuiProfileDialogOpened,
                "gui_profile_dialog_opened",
            ),
            (Activity::GuiAifaQueryEdited, "gui_aifa_query_edited"),
            (Activity::GuiPanelToggled, "gui_panel_toggled"),
            (Activity::GuiLogLevelSelected, "gui_log_level_selected"),
            (Activity::GuiConsentChanged, "gui_consent_changed"),
            (Activity::GuiLoggingStop, "gui_logging_stop"),
        ];
        let mut failed = false;
        for (activity, _) in activities {
            record_ui_action(
                Some(&session),
                &mut failed,
                activity,
                DisplayControl::new("synthetic_action"),
            );
        }
        assert!(!failed);
        session.finish(Component::Desktop).expect("fine sessione");

        let log = fs::read_to_string(path).expect("log sintetico");
        for (_, activity) in activities {
            assert!(log.contains(&format!("\"activity\":\"{activity}\"")));
        }
        assert!(!log.contains("dato privato sintetico"));
    }

    #[test]
    fn gui_action_events_distinguish_static_controls_and_repeated_indices() {
        use sanikey_core::logging::{Activity, Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        let mut failed = false;
        record_ui_action(
            Some(&session),
            &mut failed,
            Activity::GuiPanelToggled,
            DisplayControl::indexed("person_panel", 0, None),
        );
        record_ui_action(
            Some(&session),
            &mut failed,
            Activity::GuiPanelToggled,
            DisplayControl::indexed("person_panel", 1, None),
        );
        record_ui_action(
            Some(&session),
            &mut failed,
            Activity::GuiAifaQueryEdited,
            DisplayControl::indexed("aifa_manual_query", 0, Some(2)),
        );
        assert!(!failed);
        session.finish(Component::Desktop).expect("fine sessione");

        let log = fs::read_to_string(path).expect("log sintetico");
        assert!(log.contains(
            r#""target":{"child_index":null,"index":0,"leaf_index":null,"name":"person_panel","part":"value"}"#
        ));
        assert!(log.contains(
            r#""target":{"child_index":null,"index":1,"leaf_index":null,"name":"person_panel","part":"value"}"#
        ));
        assert!(log.contains(
            r#""target":{"child_index":2,"index":0,"leaf_index":null,"name":"aifa_manual_query","part":"value"}"#
        ));
        assert!(!log.contains("query privata sintetica"));
    }

    #[test]
    fn gui_actions_before_logging_activation_are_not_backfilled() {
        use sanikey_core::logging::{Activity, Component, Level, Session};

        let mut failed = false;
        record_ui_action(
            None,
            &mut failed,
            Activity::GuiProfileDialogOpened,
            DisplayControl::new("open_profile"),
        );
        record_ui_action(
            None,
            &mut failed,
            Activity::GuiAifaQueryEdited,
            DisplayControl::new("aifa_manual_query"),
        );
        record_ui_action(
            None,
            &mut failed,
            Activity::GuiPanelToggled,
            DisplayControl::new("duplicate_panel"),
        );
        assert!(!failed);

        let directory = TestDirectory::new();
        let (session, path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        session.finish(Component::Desktop).expect("fine sessione");
        let log = fs::read_to_string(path).expect("log sintetico");
        assert!(!log.contains("gui_profile_dialog_opened"));
        assert!(!log.contains("gui_aifa_query_edited"));
        assert!(!log.contains("gui_panel_toggled"));
    }

    #[test]
    fn logging_creation_error_is_kept_for_the_operator() {
        use sanikey_core::logging::Level;

        let directory = TestDirectory::new();
        let not_a_directory = directory.0.join("file");
        fs::write(&not_a_directory, "fixture sintetica").expect("file fixture");
        let mut app = DesktopApp {
            log_directory: Some(not_a_directory),
            pending_log_action: Some(super::PendingLogAction::Start(Level::Events)),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        run_app_frame(&mut app, &context, &mut frame, Vec::new()).drop_without_applying_deltas();

        assert!(app.log_session.is_none());
        assert!(
            app.log_action_error
                .as_deref()
                .is_some_and(|error| { error.contains("Impossibile avviare il log") })
        );
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit dell'errore");
        let error_is_accessible = tree.nodes.iter().any(|(_, node)| {
            [node.label(), node.value()]
                .into_iter()
                .flatten()
                .any(|text| text.contains("Impossibile avviare il log"))
        });
        output.drop_without_applying_deltas();
        assert!(error_is_accessible);
    }

    #[test]
    fn diagnostic_session_captures_person_heading_with_numeric_context() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            ..DesktopApp::default()
        };
        app.import_profile(profile.source_path());
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let frame_output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = frame_output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap();
        let output = fs::read_to_string(log_path).expect("log sintetico");
        let missing = tree
            .nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value()])
            .flatten()
            .filter(|text| !text.is_empty() && !output.contains(text))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        frame_output.drop_without_applying_deltas();
        assert!(
            missing.is_empty(),
            "Testo accessibile assente dal log: {missing:?}"
        );
        assert!(output.contains("Persona A (patient-a)"));
        assert!(output.contains("\"control\":\"person_heading\""));
        assert!(output.contains("\"index\":0"));
        assert!(output.contains("\"control\":\"source_documents\""));
        assert!(output.contains("\"control\":\"metadata_directory\""));
        assert!(output.contains("\"control\":\"local_build\""));
    }

    #[test]
    fn trace_session_redacts_recognized_secret_assignments_inside_paths() {
        use sanikey_core::logging::{
            Component, Level, Session, TraceField, TraceValue, trace_current,
        };

        let directory = TestDirectory::new();
        let (session, log_path) =
            Session::create_in_directory_with_level(&directory.0, Component::Desktop, Level::Trace)
                .expect("sessione Trace sintetica");
        let span = session
            .scoped(|| {
                trace_current(Component::Desktop, "desktop::synthetic_paths", || {
                    vec![
                        TraceField::new(
                            "secret_path",
                            trace_desktop_path(std::path::Path::new(
                                "/synthetic/private/api_key=SECRET-PATH-SYNTHETIC",
                            )),
                        ),
                        TraceField::new(
                            "ordinary_path",
                            trace_desktop_path(std::path::Path::new("/synthetic/ordinary")),
                        ),
                    ]
                })
            })
            .expect("span Trace sintetica");
        span.finish(TraceValue::data("synthetic complete"))
            .expect("chiusura span sintetica");
        session
            .finish(Component::Desktop)
            .expect("chiusura sessione");

        let log = fs::read_to_string(log_path).expect("log Trace sintetico");
        assert!(log.contains("\"kind\":\"secret_omitted\""));
        assert!(!log.contains("/synthetic/private/api_key"));
        assert!(log.contains("/synthetic/ordinary"));
        assert!(!log.contains("SECRET-PATH-SYNTHETIC"));
    }

    #[test]
    fn diagnostic_capture_covers_accessible_initial_screen_text() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .unwrap();
        let mut app = DesktopApp {
            log_session: Some(session),
            profile_path: "/profilo/sintetico".to_owned(),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = output.platform_output.accesskit_update.as_ref().unwrap();
        let log = fs::read_to_string(log_path).unwrap();
        let missing = tree
            .nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value()])
            .flatten()
            .filter(|text| !text.is_empty() && !log.contains(text))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        output.drop_without_applying_deltas();
        assert!(
            missing.is_empty(),
            "Testo accessibile assente dal log: {missing:?}"
        );
    }

    #[test]
    fn diagnostic_capture_records_native_window_title_with_distinct_control() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        output.drop_without_applying_deltas();

        let log = fs::read_to_string(log_path).expect("log sintetico");
        let title = log
            .lines()
            .find(|line| line.contains("\"control\":\"window_title\""))
            .expect("record titolo finestra");
        let heading = log
            .lines()
            .find(|line| line.contains("\"control\":\"heading\""))
            .expect("record heading egui");
        assert!(title.contains("\"text\":\"SaniKey\""));
        assert!(heading.contains("\"text\":\"SaniKey\""));
    }

    #[test]
    fn diagnostic_capture_records_accessible_logging_failure_warning() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            log_failure: true,
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit del messaggio di errore");
        let warning = "Registrazione interrotta o incompleta; verifica il file di log.";
        assert!(tree.nodes.iter().any(|(_, node)| {
            node.label().is_some_and(|label| label == warning)
                || node.value().is_some_and(|value| value == warning)
        }));
        output.drop_without_applying_deltas();

        let log = fs::read_to_string(log_path).expect("log sintetico");
        assert!(log.contains(warning));
        assert!(log.contains("\"control\":\"log_failure_warning\""));
    }

    #[test]
    fn native_dialog_omission_write_failure_is_shown_to_operator() {
        use sanikey_core::logging::{Component, Level, NativeDialog, Session};

        let directory = TestDirectory::new();
        let (session, _) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione diagnostica sintetica");
        session
            .finish(Component::Desktop)
            .expect("chiusura sessione");

        let mut log_failure = false;
        record_native_dialog_omission(Some(&session), &mut log_failure, NativeDialog::ProfileFile);
        assert!(log_failure);

        let mut app = DesktopApp {
            log_failure,
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit dell'avviso");
        assert!(tree.nodes.iter().any(|(_, node)| {
            [node.label(), node.value()]
                .into_iter()
                .flatten()
                .any(|text| text.contains("Registrazione interrotta o incompleta"))
        }));
        output.drop_without_applying_deltas();
    }

    #[test]
    fn desktop_surfaces_a_session_failure_at_events_level() {
        use sanikey_core::logging::{Activity, Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, _) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        session
            .finish(Component::Desktop)
            .expect("chiusura anticipata fixture");
        assert!(
            session
                .start_operation(Component::Desktop, Activity::Profile)
                .is_err()
        );
        assert!(session.has_failed());

        let mut app = DesktopApp {
            log_session: Some(session),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        assert!(app.log_failure);
        let error_is_accessible = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit errore eventi")
            .nodes
            .iter()
            .any(|(_, node)| {
                [node.label(), node.value()]
                    .into_iter()
                    .flatten()
                    .any(|text| text.contains("Registrazione interrotta o incompleta"))
            });
        output.drop_without_applying_deltas();
        assert!(error_is_accessible);
    }

    #[test]
    fn finishing_a_session_surfaces_an_unfinished_worker_operation() {
        use sanikey_core::logging::{Activity, Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Events,
        )
        .expect("sessione sintetica");
        let operation = session
            .start_operation(Component::Desktop, Activity::Build)
            .expect("operazione worker sintetica");
        drop(operation);
        assert!(session.has_failed());

        let mut app = DesktopApp {
            log_session: Some(session),
            ..DesktopApp::default()
        };
        app.finish_log_session();

        assert!(app.log_session.is_none());
        assert!(app.log_failure);
        assert_eq!(
            app.log_action_error.as_deref(),
            Some("Registrazione interrotta o incompleta; verifica il file di log.")
        );
        let log = fs::read_to_string(log_path).expect("log sintetico chiuso");
        assert!(log.contains("\"event\":\"unfinished\""));
        assert!(log.contains("\"event\":\"finished\""));
    }

    #[test]
    fn diagnostic_capture_covers_accessible_preflight_review_text() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .unwrap();
        let mut app = DesktopApp {
            log_session: Some(session),
            review: Some(ReviewSummary {
                document_count: 2,
                duplicate_groups: vec![(
                    "Persona sintetica".into(),
                    "abc123".into(),
                    vec![PathBuf::from("/documento/sintetico")],
                )],
                selected_people: vec!["persona-sintetica".into()],
            }),
            ..DesktopApp::default()
        };
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 2000.0),
                )),
                ..Default::default()
            },
            |ui| eframe::App::ui(&mut app, ui, &mut frame),
        );
        let log = fs::read_to_string(log_path).unwrap();
        let missing = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value()])
            .flatten()
            .filter(|text| !text.is_empty() && !log.contains(text))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        output.drop_without_applying_deltas();
        assert!(
            missing.is_empty(),
            "Testo accessibile assente dal log: {missing:?}"
        );
        assert!(log.contains("/documento/sintetico"));
    }

    fn focused_widget_after_key(key: egui::Key, modifiers: egui::Modifiers) -> Option<egui::Id> {
        let mut app = DesktopApp::default();
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..2 {
            let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
            output.drop_without_applying_deltas();
        }
        let output = run_app_frame(
            &mut app,
            &context,
            &mut frame,
            vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers,
            }],
        );
        output.drop_without_applying_deltas();
        context.memory(|memory| memory.focused())
    }

    fn synthetic_result(person_id: &str) -> PatientBuildResult {
        PatientBuildResult {
            patient_id: person_id.to_owned(),
            build_root: PathBuf::new(),
            counts: BuildCounts::default(),
            duplicates: 0,
            warnings: 0,
            warning_messages: Vec::new(),
            database: PathBuf::new(),
            manifest: PathBuf::new(),
            checksums: PathBuf::new(),
            report: PathBuf::new(),
        }
    }

    #[test]
    fn import_keeps_profile_unchanged_and_selects_only_enabled_people() {
        let directory = TestDirectory::new();
        let profile_path = directory.0.join("accounts.toml");
        let source = directory.0.join("documenti");
        let metadata = directory.0.join("metadati");
        let output = directory.0.join("generati");
        let original = format!(
            r#"[global]
config_version = 1
[[person]]
id = "patient-a"
display_name = "Persona A"
source_documents = "{}"
metadata_directory = "{}"
local_build = "{}"
usb_uuid = "1A2B-3C4D"
enabled = true
[[person]]
id = "patient-b"
display_name = "Persona B"
source_documents = "{}"
metadata_directory = "{}"
local_build = "{}"
usb_uuid = "1A2B-3C4D"
enabled = false
"#,
            source.display(),
            metadata.display(),
            output.display(),
            source.display(),
            metadata.display(),
            output.display(),
        );
        fs::write(&profile_path, &original).expect("profilo sintetico");

        let mut app = DesktopApp::default();
        app.import_profile(&profile_path);

        assert_eq!(app.selections.len(), 2);
        assert!(app.selections[0].selected);
        assert!(!app.selections[1].selected);
        assert_eq!(
            fs::read_to_string(profile_path).expect("profilo conservato"),
            original
        );
    }

    #[test]
    fn invalid_profile_clears_loaded_state_and_reports_error() {
        let directory = TestDirectory::new();
        let profile_path = directory.0.join("invalid.toml");
        fs::write(&profile_path, "[global]\nconfig_version = 999\n").expect("profilo invalido");

        let mut app = DesktopApp::default();
        app.import_profile(&profile_path);

        assert!(app.profile.is_none());
        assert!(app.selections.is_empty());
        assert!(app.status.contains("configuration"));
    }

    #[test]
    fn preflight_lists_duplicate_document_paths_for_review() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);

        let summary = review_profile(profile.accounts(), profile.base_directory())
            .expect("preflight sintetico");

        assert_eq!(summary.document_count, 2);
        assert_eq!(summary.selected_people, ["Persona A"]);
        assert_eq!(summary.duplicate_groups.len(), 1);
        assert_eq!(summary.duplicate_groups[0].2.len(), 2);
    }

    #[test]
    fn manually_entered_relative_paths_use_the_profile_base() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut app = DesktopApp {
            profile: Some(profile),
            ..DesktopApp::default()
        };
        app.selections = app
            .profile
            .as_ref()
            .expect("profilo caricato")
            .accounts()
            .people
            .iter()
            .map(super::selection_from_person)
            .collect();
        app.selections[0].source_documents = PathBuf::from("patients/patient-a/documents");

        let config = app.effective_config().expect("config di sessione");

        assert_eq!(
            config.people[0].source_documents,
            directory.0.join("patients/patient-a/documents")
        );
    }

    #[test]
    fn person_selection_trace_records_inputs_output_duration_and_redacts_secret_paths() {
        use sanikey_core::logging::{Component, Level, Session};
        use serde_json::Value;

        fn field_content<'a>(fields: &'a [Value], name: &str) -> &'a Value {
            fields
                .iter()
                .find(|field| field["name"] == name)
                .map(|field| &field["content"])
                .unwrap_or_else(|| panic!("campo {name} assente"))
        }

        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut person = profile.accounts().people[0].clone();
        person.display_name = "Persona sintetica privata".to_owned();
        person.source_documents =
            PathBuf::from("/synthetic/private/patients/patient-a/api_key=SECRET-SYNTHETIC");
        let (session, log_path) =
            Session::create_in_directory_with_level(&directory.0, Component::Desktop, Level::Trace)
                .expect("sessione Trace sintetica");

        let selection = session.scoped(|| selection_from_person(&person));
        session.finish(Component::Desktop).expect("chiusura Trace");

        assert!(selection.selected);
        assert_eq!(selection.source_documents, person.source_documents);
        assert_eq!(selection.metadata_directory, person.metadata_directory);
        assert_eq!(selection.local_build, person.local_build);

        let contents = fs::read_to_string(log_path).expect("log Trace sintetico");
        assert!(contents.contains("Persona sintetica privata"));
        assert!(!contents.contains("SECRET-SYNTHETIC"));
        let entries: Vec<Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
            .collect();
        let enter = entries
            .iter()
            .find(|entry| {
                entry["type"] == "function_enter"
                    && entry["function"] == "desktop::selection_from_person"
            })
            .expect("ingresso della funzione");
        let exit = entries
            .iter()
            .find(|entry| {
                entry["type"] == "function_exit"
                    && entry["function"] == "desktop::selection_from_person"
            })
            .expect("uscita della funzione");

        let arguments = enter["arguments"].as_array().expect("argomenti registrati");
        assert_eq!(arguments.len(), 2);
        assert_eq!(arguments[0]["name"], "person");
        assert_eq!(arguments[1]["name"], "paths");
        let person_fields = arguments[0]["content"]["value"]
            .as_array()
            .expect("record della persona");
        let debug_text = person_fields
            .iter()
            .find(|field| field["name"] == "value")
            .expect("valore debug della persona")["content"]["value"]["value"]
            .as_str()
            .expect("argomento privato serializzato");
        assert!(debug_text.contains("Persona sintetica privata"));
        assert_eq!(
            debug_text,
            sanikey_core::logging::redact_secret_assignments(&format!("{person:#?}"))
        );
        assert!(debug_text.contains("[secret_omitted]"));
        for field in [
            "id",
            "display_name",
            "source_documents",
            "metadata_directory",
            "local_build",
            "usb_uuid",
            "enabled",
            "ui",
            "search",
            "ingestion",
        ] {
            assert!(debug_text.contains(&format!("{field}:")), "campo {field}");
        }
        let input_paths = arguments[1]["content"]["value"]
            .as_array()
            .expect("percorsi di input");
        assert_eq!(input_paths.len(), 3);
        assert_eq!(
            field_content(input_paths, "source_documents")["kind"],
            "secret_omitted"
        );
        for (name, expected) in [
            ("metadata_directory", &person.metadata_directory),
            ("local_build", &person.local_build),
        ] {
            let content = field_content(input_paths, name);
            assert_eq!(content["kind"], "data");
            assert_eq!(content["value"]["encoding"], "utf8");
            assert_eq!(
                content["value"]["value"],
                expected.to_str().expect("percorso UTF-8")
            );
        }

        assert_eq!(exit["call_id"], enter["call_id"]);
        assert_eq!(exit["outcome"], "returned");
        assert!(exit["duration_ms"].as_u64().is_some());
        let result_fields = exit["result"]["value"]
            .as_array()
            .expect("risultato strutturato");
        assert_eq!(result_fields.len(), 4);
        assert_eq!(field_content(result_fields, "selected")["value"], true);
        assert_eq!(
            field_content(result_fields, "source_documents")["kind"],
            "secret_omitted"
        );
        for (name, expected) in [
            ("metadata_directory", &person.metadata_directory),
            ("local_build", &person.local_build),
        ] {
            let content = field_content(result_fields, name);
            assert_eq!(content["kind"], "data");
            assert_eq!(content["value"]["encoding"], "utf8");
            assert_eq!(
                content["value"]["value"],
                expected.to_str().expect("percorso UTF-8")
            );
        }
    }

    #[test]
    fn preflight_rejects_empty_selected_folder_paths() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut app = DesktopApp {
            profile: Some(profile),
            ..DesktopApp::default()
        };
        app.selections = app
            .profile
            .as_ref()
            .expect("profilo caricato")
            .accounts()
            .people
            .iter()
            .map(super::selection_from_person)
            .collect();
        app.selections[0].source_documents.clear();

        app.start_preflight();

        assert!(app.receiver.is_none());
        assert!(app.status.contains("Completa tutti i percorsi"));
    }

    #[test]
    fn preflight_starts_with_indeterminate_progress() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut app = DesktopApp::default();
        app.import_profile(profile.source_path());
        app.progress = Some(0.67);

        app.start_preflight();

        assert!(app.progress.is_none());
        assert!(app.receiver.is_some());
    }

    #[test]
    fn indeterminate_progress_bar_moves_its_segment_across_the_track() {
        let track = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 8.0));
        let start = indeterminate_progress_segment(track, 0.0);
        let middle = indeterminate_progress_segment(track, 0.6);
        let end = indeterminate_progress_segment(track, 1.2);

        assert!(start.left() < middle.left());
        assert!(middle.left() < end.left());
        assert!(track.contains_rect(start));
        assert!(track.contains_rect(middle));
        assert!(track.contains_rect(end));
    }

    #[test]
    fn long_paths_wrap_without_losing_their_beginning_or_end() {
        let path = std::path::Path::new(
            "/synthetic/data/patient-a/documents/Cardiologist/19990700 Pressure.xls",
        );
        let expected = path.display().to_string();
        let context = egui::Context::default();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 240.0),
                )),
                ..Default::default()
            },
            |ui| show_path(ui, "Documento:", path),
        );

        assert!(output.shapes.iter().any(|clipped| {
            matches!(
                &clipped.shape,
                egui::epaint::Shape::Text(text) if text.galley.text() == expected
            )
        }));
        output.drop_without_applying_deltas();
    }

    #[test]
    fn path_fields_expand_with_the_available_window_width() {
        let measure_width = |viewport_width| {
            let context = egui::Context::default();
            let mut measured_width = 0.0;
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(viewport_width, 160.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    let label = ui.label("Percorso sintetico:");
                    ui.horizontal_wrapped(|ui| {
                        let mut path = "/synthetic/profile/path".to_owned();
                        measured_width = add_responsive_path_edit(ui, label.id, &mut path, 100.0)
                            .rect
                            .width();
                    });
                },
            );
            output.drop_without_applying_deltas();
            measured_width
        };

        let narrow_width = measure_width(400.0);
        let wide_width = measure_width(800.0);
        assert!(wide_width > narrow_width);
        assert!(wide_width - narrow_width > 300.0);
    }

    #[test]
    fn desktop_theme_uses_straw_background_and_navy_text() {
        let context = egui::Context::default();
        configure_desktop_theme(&context);
        let visuals = desktop_visuals();

        assert_eq!(visuals.panel_fill, STRAW_BACKGROUND);
        assert_eq!(visuals.window_fill, STRAW_BACKGROUND);
        assert_eq!(visuals.extreme_bg_color, FIELD_BACKGROUND);
        assert_eq!(visuals.override_text_color, Some(BLUE_TEXT));
        assert_eq!(BLUE_TEXT.r(), 0);
        assert!(BLUE_TEXT.b() > BLUE_TEXT.g() * 3);
        assert_eq!(visuals.disabled_alpha(), 1.0);
        assert_eq!(context.theme(), egui::Theme::Light);
        let configured_visuals = context.style_of(egui::Theme::Light).visuals.clone();
        assert_eq!(configured_visuals.panel_fill, STRAW_BACKGROUND);
        assert_eq!(configured_visuals.override_text_color, Some(BLUE_TEXT));
    }

    #[test]
    fn build_emits_progress_success_and_incremental_resume_results() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let person = profile.accounts().people[0].clone();
        let (sender, receiver) = std::sync::mpsc::channel();

        build_selected_people(vec![person.clone()], CancellationToken::default(), sender);
        let first = collect_events(receiver);
        assert!(first.iter().any(|event| matches!(
            event,
            DesktopEvent::Progress(OperationEvent::Progress { total: None, .. })
        )));
        assert!(first.iter().any(|event| matches!(
            event,
            DesktopEvent::Progress(OperationEvent::Progress {
                completed: 1,
                total: Some(1),
                ..
            })
        )));
        assert!(
            first
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Succeeded)))
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        build_selected_people(vec![person], CancellationToken::default(), sender);
        let resumed = collect_events(receiver);
        assert!(resumed.iter().any(|event| matches!(
            event,
            DesktopEvent::PatientBuilt(result) if result.counts.cached_documents == 1
        )));
        assert!(
            resumed
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Succeeded)))
        );
    }

    #[test]
    fn trace_records_desktop_build_operation_unit_result() {
        use sanikey_core::logging::{Component, Level, Session};
        use serde_json::Value;

        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let person = profile.accounts().people[0].clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0.join("logs"),
            Component::Desktop,
            Level::Trace,
        )
        .expect("sessione sintetica");

        session.scoped(|| {
            build_selected_people_with(
                vec![person],
                CancellationToken::default(),
                sender,
                |person| Ok(synthetic_result(&person.id)),
            );
        });
        assert!(
            collect_events(receiver)
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Succeeded)))
        );
        drop(session);

        let records: Vec<Value> = fs::read_to_string(log_path)
            .expect("log traccia sintetico")
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON valido"))
            .collect();
        let operation_result = records
            .iter()
            .find(|record| {
                record["type"] == "function_exit"
                    && record["function"] == "operation::run_operation"
            })
            .expect("risultato span build.generate");
        assert_eq!(operation_result["result"]["value"]["outcome"], "succeeded");
        assert_eq!(operation_result["result"]["value"]["value"]["kind"], "data");
        assert_eq!(
            operation_result["result"]["value"]["value"]["value"],
            Value::Null
        );
    }

    #[test]
    fn observation_import_action_generates_current_outputs_for_selected_people() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let person = profile.accounts().people[0].clone();
        fs::write(
            person.source_documents.join("observations.csv"),
            "Data,Peso\n2026-01-02,70.5\n",
        )
        .expect("CSV osservazioni sintetico");
        fs::write(
            person.metadata_directory.join("observation_imports.toml"),
            "[[series]]\nid='peso'\nname='Peso'\nvalue_type='numeric'\nunit='kg'\n\n[[source]]\npath='observations.csv'\nseries_id='peso'\n[source.columns]\ndate='Data'\nnumeric_value='Peso'\n",
        )
        .expect("manifest osservazioni sintetico");

        let summary = import_selected_observations(std::slice::from_ref(&person))
            .expect("importazione osservazioni riuscita");

        assert_eq!(summary.people, 1);
        assert_eq!(summary.series, 1);
        assert_eq!(summary.points, 1);
        assert!(summary.warnings.is_empty());
        sanikey_core::observation_imports::ensure_observation_imports_current(
            &person.metadata_directory,
            &person.source_documents,
        )
        .expect("output importato aggiornato per la build");
        let build = build_patient(&person, BuildOptions::default())
            .expect("build incrementale dopo l'importazione");
        assert_eq!(build.patient_id, person.id);
    }

    #[test]
    fn build_reports_failure_and_stops_at_a_pre_requested_cancellation() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut person = profile.accounts().people[0].clone();
        let blocked_output = directory.0.join("output-is-a-file");
        fs::write(&blocked_output, "file sintetico").expect("conflitto output");
        person.local_build = blocked_output;
        let (sender, receiver) = std::sync::mpsc::channel();
        build_selected_people(vec![person.clone()], CancellationToken::default(), sender);
        assert!(
            collect_events(receiver)
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Failed(_))))
        );

        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let (sender, receiver) = std::sync::mpsc::channel();
        build_selected_people(vec![person], cancellation, sender);
        let cancelled = collect_events(receiver);
        assert!(
            !cancelled
                .iter()
                .any(|event| matches!(event, DesktopEvent::PatientBuilt(_)))
        );
        assert!(
            cancelled
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Cancelled)))
        );
    }

    #[test]
    fn terminal_events_set_distinct_success_failure_and_cancel_states() {
        let cases = [
            (OperationEvent::Succeeded, "Build completata."),
            (
                OperationEvent::Failed(sanikey_core::operation::OperationError::new(
                    sanikey_core::operation::ErrorCode::Configuration,
                    "configurazione sintetica invalida",
                )),
                "Build fallita (configuration)",
            ),
            (
                OperationEvent::Cancelled,
                "Build annullata al punto sicuro.",
            ),
        ];

        for (event, expected_status) in cases {
            let (sender, receiver) = std::sync::mpsc::channel();
            let mut app = DesktopApp {
                receiver: Some(receiver),
                cancellation: Some(CancellationToken::default()),
                ..DesktopApp::default()
            };
            sender
                .send(DesktopEvent::Terminal(event))
                .expect("evento terminale sintetico");
            drop(sender);

            app.receive_worker_events();

            assert!(app.status.starts_with(expected_status));
            assert!(app.receiver.is_none());
            assert!(app.cancellation.is_none());
        }
    }

    #[test]
    fn cancellation_requested_during_a_person_stops_at_the_next_person_boundary() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let first = profile.accounts().people[0].clone();
        let mut second = first.clone();
        second.id = "patient-b".to_owned();
        let cancellation = CancellationToken::default();
        let action_cancellation = cancellation.clone();
        let (sender, receiver) = std::sync::mpsc::channel();

        build_selected_people_with(vec![first, second], cancellation, sender, move |person| {
            action_cancellation.cancel();
            Ok(synthetic_result(&person.id))
        });
        let events = collect_events(receiver);

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, DesktopEvent::PatientBuilt(_)))
                .count(),
            1
        );
        assert!(events.iter().any(|event| matches!(
            event,
            DesktopEvent::Progress(OperationEvent::Progress {
                completed: 1,
                total: Some(2),
                ..
            })
        )));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DesktopEvent::Terminal(OperationEvent::Cancelled)))
        );
    }

    #[test]
    fn desktop_ui_renders_with_keyboard_focus_themes_scaling_and_narrow_viewports() {
        let mut app = DesktopApp::default();
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();

        for (width, height, pixels_per_point, theme) in [
            (800.0, 600.0, 1.0, egui::Theme::Light),
            (400.0, 280.0, 2.0, egui::Theme::Dark),
        ] {
            context.set_theme(egui::ThemePreference::System);
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, height),
                )),
                system_theme: Some(theme),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .expect("viewport principale")
                .native_pixels_per_point = Some(pixels_per_point);
            let output = context.run_ui(input, |ui| {
                eframe::App::ui(&mut app, ui, &mut frame);
            });
            if width == 400.0 {
                let button = output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::epaint::Shape::Text(text)
                            if text.galley.text().contains("Apri profilo") =>
                        {
                            Some((clipped.clip_rect, text.visual_bounding_rect()))
                        }
                        _ => None,
                    });
                let (clip_rect, text_rect) =
                    button.expect("etichetta pulsante profilo renderizzata");
                assert!(
                    clip_rect.contains_rect(text_rect),
                    "etichetta pulsante profilo tagliata nel viewport stretto"
                );
            }
            output.drop_without_applying_deltas();

            assert_eq!(context.pixels_per_point(), pixels_per_point);
            assert_eq!(context.theme(), egui::Theme::Light);
        }

        let first_focus = focused_widget_after_key(egui::Key::Tab, egui::Modifiers::NONE)
            .expect("Tab porta il focus a un controllo");
        let last_focus = focused_widget_after_key(egui::Key::Tab, egui::Modifiers::SHIFT)
            .expect("Shift+Tab porta il focus a un controllo");
        assert_ne!(
            first_focus, last_focus,
            "Tab e Shift+Tab raggiungono controlli distinti alle estremita dell'ordine"
        );
    }

    #[test]
    fn accessibility_tree_exposes_form_names_and_invalid_profile_status() {
        let directory = TestDirectory::new();
        let profile_path = directory.0.join("invalid.toml");
        fs::write(&profile_path, "[global]\nconfig_version = 999\n")
            .expect("profilo sintetico non valido");
        let mut app = DesktopApp::default();
        app.import_profile(&profile_path);

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit prodotto dal frame");
        let labels = tree
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let input_name = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::TextInput)
            .and_then(|(_, node)| node.labelled_by().first())
            .and_then(|label_id| tree.nodes.iter().find(|(node_id, _)| node_id == label_id))
            .and_then(|(_, node)| node.value())
            .map(str::to_owned);
        let status_is_accessible = tree
            .nodes
            .iter()
            .any(|(_, node)| node.value() == Some(app.status.as_str()));

        output.drop_without_applying_deltas();
        assert_eq!(input_name.as_deref(), Some("Profilo account"));
        assert!(
            labels.iter().any(|label| label == "Apri profilo… (Ctrl+O)"),
            "{labels:?}"
        );
        assert!(status_is_accessible);
    }

    #[test]
    fn observation_import_action_is_accessible_after_preflight() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut app = DesktopApp::default();
        app.import_profile(profile.source_path());
        app.review = Some(ReviewSummary {
            selected_people: vec!["Persona A".to_owned()],
            ..ReviewSummary::default()
        });

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit prodotto dal frame");
        let labels = tree
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label())
            .map(str::to_owned)
            .collect::<Vec<_>>();

        output.drop_without_applying_deltas();
        assert!(
            labels
                .iter()
                .any(|label| label == "Importa/aggiorna osservazioni"),
            "{labels:?}"
        );
    }

    #[test]
    fn clicking_preflight_with_an_empty_path_shows_accessible_error() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let mut app = DesktopApp::default();
        app.import_profile(profile.source_path());
        app.selections[0].source_documents = PathBuf::new();

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let initial = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let button_position = initial
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text)
                    if text.galley.text() == "Controlla preflight e anomalie" =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("pulsante preflight renderizzato");
        initial.drop_without_applying_deltas();

        let moved = run_app_frame(
            &mut app,
            &context,
            &mut frame,
            vec![egui::Event::PointerMoved(button_position)],
        );
        moved.drop_without_applying_deltas();
        let pressed = run_app_frame(
            &mut app,
            &context,
            &mut frame,
            vec![egui::Event::PointerButton {
                pos: button_position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        pressed.drop_without_applying_deltas();
        let released = run_app_frame(
            &mut app,
            &context,
            &mut frame,
            vec![egui::Event::PointerButton {
                pos: button_position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let error_is_accessible = released
            .platform_output
            .accesskit_update
            .as_ref()
            .is_some_and(|tree| {
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.value() == Some(app.status.as_str()))
            });
        released.drop_without_applying_deltas();

        assert_eq!(
            app.status,
            "Completa tutti i percorsi delle persone selezionate prima del preflight."
        );
        assert!(error_is_accessible);
    }

    #[test]
    fn clicking_preflight_with_a_valid_profile_exposes_the_duplicate_review() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let profile_path = profile.source_path().to_path_buf();
        let mut app = DesktopApp::default();
        app.import_profile(&profile_path);

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let initial = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let button_position = initial
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text)
                    if text.galley.text() == "Controlla preflight e anomalie" =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("pulsante preflight renderizzato");
        initial.drop_without_applying_deltas();

        for event in [
            egui::Event::PointerMoved(button_position),
            egui::Event::PointerButton {
                pos: button_position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: button_position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ] {
            let output = run_app_frame(&mut app, &context, &mut frame, vec![event]);
            output.drop_without_applying_deltas();
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.review.is_none() && std::time::Instant::now() < deadline {
            let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
            output.drop_without_applying_deltas();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.review.is_some(), "preflight completato entro 5 secondi");
        assert_eq!(app.review.as_ref().expect("riepilogo").document_count, 2);
        assert_eq!(
            app.review
                .as_ref()
                .expect("riepilogo")
                .duplicate_groups
                .len(),
            1
        );

        let output = run_app_frame(&mut app, &context, &mut frame, Vec::new());
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit dopo il preflight");
        let review_is_accessible = tree
            .nodes
            .iter()
            .any(|(_, node)| node.value() == Some("Riepilogo preflight"));
        let status_is_accessible = tree
            .nodes
            .iter()
            .any(|(_, node)| node.value() == Some(app.status.as_str()));
        output.drop_without_applying_deltas();

        assert!(review_is_accessible);
        assert!(status_is_accessible);
    }

    #[test]
    fn warning_text_color_matches_light_and_dark_themes() {
        assert_eq!(warning_text_color(true), egui::Color32::LIGHT_RED);
        assert_eq!(warning_text_color(false), egui::Color32::DARK_RED);
    }

    #[test]
    fn aifa_review_loads_synthetic_medications_from_the_selected_person() {
        let directory = TestDirectory::new();
        let profile = profile_with_document(&directory);
        let person = profile
            .accounts()
            .people
            .first()
            .expect("persona sintetica");
        fs::write(
            person.metadata_directory.join("medications.toml"),
            "[[medication]]\nid='drug-a'\nname='Farmaco sintetico'\nactive_ingredient='principio'\nform='compresse'\nstrength_per_unit='10 mg'\n",
        )
        .expect("farmaco sintetico");

        let loaded = load_aifa_people(std::slice::from_ref(person)).expect("metadati caricati");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "patient-a");
        assert_eq!(loaded[0].medications.len(), 1);
        assert_eq!(
            loaded[0].medications[0].medication.name,
            "Farmaco sintetico"
        );
    }

    #[test]
    fn aifa_review_exposes_search_and_confirmation_controls_accessibly() {
        use sanikey_core::logging::{Component, Level, Session};

        let directory = TestDirectory::new();
        let (session, log_path) = Session::create_in_directory_with_level(
            &directory.0,
            Component::Desktop,
            Level::Diagnostic,
        )
        .expect("sessione sintetica");
        let mut app = DesktopApp {
            log_session: Some(session),
            aifa_people: Some(vec![AifaPersonReview {
                id: "patient-a".into(),
                display_name: "Persona A sintetica".into(),
                metadata_directory: PathBuf::from("metadata"),
                metadata: CuratedMetadata::default(),
                medications: vec![AifaMedicationReview {
                    medication: Medication {
                        id: "drug-a".into(),
                        name: "Farmaco sintetico".into(),
                        active_ingredient: Some("principio".into()),
                        form: Some("compresse".into()),
                        strength_per_unit: Some("10 mg".into()),
                    },
                    query: String::new(),
                    search: Some(AifaCandidateSearch {
                        exact: vec![AifaCandidate {
                            title: "Farmaco sintetico 10 mg".into(),
                            codice_sis: "123".into(),
                            aic6: "456".into(),
                            active_ingredients: vec!["principio".into()],
                            form: Some("compresse".into()),
                            strength: Some("10 mg".into()),
                            company: Some("Azienda sintetica".into()),
                            atc: Vec::new(),
                            routes: Vec::new(),
                            packages: Vec::new(),
                            mismatches: Vec::new(),
                        }],
                        ..AifaCandidateSearch::default()
                    }),
                    selected_candidate: Some(0),
                    status: "Ricerca sintetica pronta.".into(),
                }],
            }]),
            ..DesktopApp::default()
        };

        let context = egui::Context::default();
        context.enable_accesskit();
        let mut frame = eframe::Frame::_new_kittest();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 3000.0),
                )),
                ..Default::default()
            },
            |ui| eframe::App::ui(&mut app, ui, &mut frame),
        );
        let log = fs::read_to_string(log_path).expect("log sintetico");
        assert!(log.contains("\"control\":\"aifa_medication_name\""));
        assert!(log.contains("Farmaco sintetico"));
        assert!(log.contains("Ricerca sintetica pronta."));
        assert!(log.contains("Principi attivi: principio"));
        assert!(log.contains("\"control\":\"aifa_identifier\""));
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("albero AccessKit AIFA");
        let labels = tree
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let manual_search_is_labelled = tree.nodes.iter().any(|(_, node)| {
            node.role() == egui::accesskit::Role::TextInput
                && node.labelled_by().iter().any(|label_id| {
                    tree.nodes
                        .iter()
                        .find(|(node_id, _)| node_id == label_id)
                        .and_then(|(_, label)| label.value())
                        == Some("Ricerca manuale")
                })
        });

        let missing = tree
            .nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value()])
            .flatten()
            .filter(|text| !text.is_empty() && !log.contains(text))
            .map(str::to_owned)
            .collect::<Vec<_>>();

        output.drop_without_applying_deltas();
        assert!(
            missing.is_empty(),
            "Testo accessibile assente dal log: {missing:?}"
        );
        assert!(manual_search_is_labelled);
        assert!(
            labels
                .iter()
                .any(|label| label == "Cerca nome e principio attivo")
        );
        assert!(
            labels
                .iter()
                .any(|label| label == "Conferma stato non AIFA")
        );
        assert!(
            labels
                .iter()
                .any(|label| { label == "Conferma candidato e verifica FI/RCP" })
        );
    }

    #[test]
    fn aifa_confirmation_updates_only_the_second_person_with_shared_medication_id() {
        let directory = TestDirectory::new();
        let medication = Medication {
            id: "drug-a".into(),
            name: "Farmaco sintetico".into(),
            active_ingredient: None,
            form: None,
            strength_per_unit: None,
        };
        let mut people = Vec::new();
        for id in ["patient-a", "patient-b"] {
            let path = directory.0.join(id);
            fs::create_dir_all(&path).expect("metadati sintetici");
            fs::write(
                path.join("medications.toml"),
                "[[medication]]\nid='drug-a'\nname='Farmaco sintetico'\n",
            )
            .expect("farmaco sintetico");
            let metadata = load_curated_metadata(&path).expect("metadati caricati");
            people.push(AifaPersonReview {
                id: id.into(),
                display_name: id.into(),
                metadata_directory: path,
                metadata,
                medications: vec![AifaMedicationReview {
                    medication: medication.clone(),
                    query: String::new(),
                    search: Some(AifaCandidateSearch::default()),
                    selected_candidate: Some(0),
                    status: String::new(),
                }],
            });
        }
        let candidate = AifaCandidate {
            title: "Farmaco sintetico".into(),
            codice_sis: "123".into(),
            aic6: "456".into(),
            active_ingredients: Vec::new(),
            form: None,
            strength: None,
            company: None,
            atc: Vec::new(),
            routes: Vec::new(),
            packages: Vec::new(),
            mismatches: Vec::new(),
        };
        let saved = save_aifa_decision_with(
            &people[1].metadata_directory,
            people[1].metadata.clone(),
            &medication,
            AifaDecision::Confirm(Box::new(candidate)),
            |_| Vec::new(),
        )
        .expect("salvataggio sintetico");
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(DesktopEvent::AifaDecisionSaved {
                person_id: "patient-b".into(),
                medication_id: "drug-a".into(),
                result: Ok(saved),
            })
            .expect("evento");
        let mut app = DesktopApp {
            receiver: Some(receiver),
            aifa_people: Some(people),
            ..DesktopApp::default()
        };
        app.receive_worker_events();
        let people = app.aifa_people.as_ref().expect("persone");
        assert!(people[0].metadata.medication_leaflets.is_empty());
        assert!(people[0].medications[0].search.is_some());
        assert_eq!(
            people[1].metadata.medication_leaflets[0].medication_id,
            "drug-a"
        );
        assert!(people[1].medications[0].search.is_none());
        assert!(people[1].medications[0].status.contains("salvato"));
        assert_eq!(
            load_curated_metadata(&people[1].metadata_directory)
                .expect("rilettura")
                .medication_leaflets
                .len(),
            1
        );
    }

    #[test]
    fn aifa_confirmation_persists_only_after_synthetic_fi_rcp_probe() {
        let directory = TestDirectory::new();
        let medication_directory = directory.0.join("metadata");
        fs::create_dir_all(&medication_directory).expect("metadati");
        fs::write(
            medication_directory.join("medications.toml"),
            "[[medication]]\nid='drug-a'\nname='Farmaco sintetico'\n\n[[medication]]\nid='drug-b'\nname='Altro farmaco'\n",
        )
        .expect("farmaci sintetici");
        fs::write(
            medication_directory.join("medication_leaflets.toml"),
            "[[unavailable]]\nmedication_id='drug-a'\nreason='non_aifa'\n",
        )
        .expect("esclusione AIFA iniziale");
        let metadata = CuratedMetadata {
            medications: vec![
                Medication {
                    id: "drug-a".into(),
                    name: "Farmaco sintetico".into(),
                    active_ingredient: None,
                    form: None,
                    strength_per_unit: None,
                },
                Medication {
                    id: "drug-b".into(),
                    name: "Altro farmaco".into(),
                    active_ingredient: None,
                    form: None,
                    strength_per_unit: None,
                },
            ],
            medication_leaflet_exclusions: vec![MedicationLeafletExclusion {
                medication_id: "drug-a".into(),
                reason: "non_aifa".into(),
            }],
            ..CuratedMetadata::default()
        };
        let candidate = AifaCandidate {
            title: "Farmaco sintetico 10 mg".into(),
            codice_sis: "123".into(),
            aic6: "456".into(),
            active_ingredients: vec!["principio".into()],
            form: Some("compresse".into()),
            strength: Some("10 mg".into()),
            company: Some("Azienda sintetica".into()),
            atc: Vec::new(),
            routes: Vec::new(),
            packages: Vec::new(),
            mismatches: Vec::new(),
        };
        let medication = metadata.medications[0].clone();

        let saved = save_aifa_decision_with(
            &medication_directory,
            metadata,
            &medication,
            AifaDecision::Confirm(Box::new(candidate)),
            |reference| {
                assert_eq!(reference.medication_id, "drug-a");
                Vec::new()
            },
        )
        .expect("conferma sintetica salvata");
        let reloaded = load_curated_metadata(&medication_directory).expect("rilettura");

        assert_eq!(reloaded.medication_leaflets.len(), 1);
        assert_eq!(reloaded.medication_leaflets[0].codice_sis, "123");
        assert_eq!(reloaded.medication_leaflets[0].aic6, "456");
        assert!(reloaded.medication_leaflet_exclusions.is_empty());
        assert_eq!(saved.metadata.medications.len(), 2);
        assert!(saved.status.contains("verifica FI/RCP"));
    }

    #[test]
    fn aifa_non_applicable_decision_preserves_other_references() {
        let directory = TestDirectory::new();
        let medication_directory = directory.0.join("metadata");
        fs::create_dir_all(&medication_directory).expect("metadati");
        fs::write(
            medication_directory.join("medications.toml"),
            "[[medication]]\nid='drug-a'\nname='Farmaco sintetico'\n\n[[medication]]\nid='drug-b'\nname='Altro farmaco'\n",
        )
        .expect("farmaci sintetici");
        fs::write(
            medication_directory.join("medication_leaflets.toml"),
            "[[leaflet]]\nmedication_id='drug-a'\ncodice_sis='111'\naic6='222'\n\n[[leaflet]]\nmedication_id='drug-b'\ncodice_sis='333'\naic6='444'\n",
        )
        .expect("riferimenti AIFA iniziali");
        let metadata = CuratedMetadata {
            medications: vec![
                Medication {
                    id: "drug-a".into(),
                    name: "Farmaco sintetico".into(),
                    active_ingredient: None,
                    form: None,
                    strength_per_unit: None,
                },
                Medication {
                    id: "drug-b".into(),
                    name: "Altro farmaco".into(),
                    active_ingredient: None,
                    form: None,
                    strength_per_unit: None,
                },
            ],
            medication_leaflets: vec![
                sanikey_core::metadata::MedicationLeaflet {
                    medication_id: "drug-a".into(),
                    codice_sis: "111".into(),
                    aic6: "222".into(),
                    downloaded_at: None,
                    source_fingerprint: None,
                },
                sanikey_core::metadata::MedicationLeaflet {
                    medication_id: "drug-b".into(),
                    codice_sis: "333".into(),
                    aic6: "444".into(),
                    downloaded_at: None,
                    source_fingerprint: None,
                },
            ],
            ..CuratedMetadata::default()
        };

        let saved = save_aifa_decision_with(
            &medication_directory,
            metadata,
            &Medication {
                id: "drug-a".into(),
                name: "Farmaco sintetico".into(),
                active_ingredient: None,
                form: None,
                strength_per_unit: None,
            },
            AifaDecision::MarkNotApplicable,
            |_| panic!("non AIFA non deve interrogare il servizio"),
        )
        .expect("decisione non AIFA salvata");
        let reloaded = load_curated_metadata(&medication_directory).expect("rilettura");

        assert_eq!(saved.metadata.medication_leaflets.len(), 1);
        assert_eq!(reloaded.medication_leaflets[0].medication_id, "drug-b");
        assert_eq!(reloaded.medication_leaflet_exclusions.len(), 1);
        assert_eq!(
            reloaded.medication_leaflet_exclusions[0].medication_id,
            "drug-a"
        );
        assert_eq!(reloaded.medication_leaflet_exclusions[0].reason, "non_aifa");
    }

    #[test]
    fn aifa_failed_document_probe_does_not_persist_the_candidate() {
        let directory = TestDirectory::new();
        let medication_directory = directory.0.join("metadata");
        fs::create_dir_all(&medication_directory).expect("metadati");
        fs::write(
            medication_directory.join("medications.toml"),
            "[[medication]]\nid='drug-a'\nname='Farmaco sintetico'\n",
        )
        .expect("farmaco sintetico");
        let metadata = CuratedMetadata {
            medications: vec![Medication {
                id: "drug-a".into(),
                name: "Farmaco sintetico".into(),
                active_ingredient: None,
                form: None,
                strength_per_unit: None,
            }],
            ..CuratedMetadata::default()
        };
        let result = save_aifa_decision_with(
            &medication_directory,
            metadata,
            &Medication {
                id: "drug-a".into(),
                name: "Farmaco sintetico".into(),
                active_ingredient: None,
                form: None,
                strength_per_unit: None,
            },
            AifaDecision::Confirm(Box::new(AifaCandidate {
                title: "Farmaco sintetico".into(),
                codice_sis: "123".into(),
                aic6: "456".into(),
                active_ingredients: Vec::new(),
                form: None,
                strength: None,
                company: None,
                atc: Vec::new(),
                routes: Vec::new(),
                packages: Vec::new(),
                mismatches: Vec::new(),
            })),
            |_| {
                vec![LeafletDownloadFailure {
                    medication_id: "drug-a".into(),
                    kind: "FI".into(),
                    url: "https://example.invalid/FI".into(),
                    reason: "fixture sintetica: non disponibile".into(),
                }]
            },
        );

        assert!(result.is_err());
        assert!(
            !medication_directory
                .join("medication_leaflets.toml")
                .exists()
        );
    }
}
