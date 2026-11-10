//! Interfaccia desktop SaniKey per importare un profilo e scegliere persone e cartelle.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
static NEXT_USB_IMAGE: AtomicUsize = AtomicUsize::new(0);

use eframe::egui::{self, Key, Modifiers};
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
use sanikey_core::metadata::{
    CuratedMetadata, Medication, MedicationLeaflet, MedicationLeafletExclusion,
    load_curated_metadata, prepare_people_metadata,
};
use sanikey_core::observation_imports::import_observations;
#[cfg(target_os = "linux")]
use sanikey_core::operation::OperationOutcome;
use sanikey_core::operation::{CancellationToken, OperationEvent, OperationPhase, run_operation};
#[cfg(target_os = "linux")]
use sanikey_core::usb_copy::{UsbCopyResult, mirror_validated_usb_image_on_linux_volume};
use sanikey_core::usb_image::UsbImageResult;
#[cfg(target_os = "linux")]
use sanikey_core::usb_image::assemble_usb_image;

const STRAW_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(248, 241, 198);
const FIELD_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(255, 252, 229);
const BLUE_TEXT: egui::Color32 = egui::Color32::from_rgb(0, 51, 204);
const MIN_PATH_FIELD_WIDTH: f32 = 64.0;

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
        Self {
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
    }
}

/// Gestisce il profilo di sessione e gli eventi di preflight e build.
impl DesktopApp {
    fn import_profile(&mut self, path: &Path) {
        self.profile_path = path.display().to_string();
        match AccountsProfile::import(path) {
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
    }

    fn open_profile_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Profili account TOML", &["toml"])
            .pick_file()
        {
            self.import_profile(&path);
        }
    }

    fn effective_config(&self) -> Option<AccountsConfig> {
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
    }

    fn start_preflight(&mut self) {
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
        thread::spawn(move || {
            let result = review_profile(&config, &root);
            let _ = sender.send(DesktopEvent::Preflight(result));
        });
    }

    fn start_build(&mut self) {
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
        thread::spawn(move || build_selected_people(selected, cancellation, sender));
    }

    fn start_observation_import(&mut self) {
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
        thread::spawn(move || {
            let result = import_selected_observations(&selected);
            let _ = sender.send(DesktopEvent::ObservationsImported(result));
        });
    }

    fn start_aifa_review(&mut self) {
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
        thread::spawn(move || {
            let result = load_aifa_people(&people);
            let _ = sender.send(DesktopEvent::AifaMetadataLoaded(result));
        });
    }

    fn start_aifa_search(&mut self, person_index: usize, medication_index: usize, manual: bool) {
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
        thread::spawn(move || {
            let result = lookup_aifa_candidate_search(&medication, query.as_deref());
            let _ = sender.send(DesktopEvent::AifaSearchFinished {
                person_id,
                medication_id,
                result,
            });
        });
    }

    fn start_aifa_decision(
        &mut self,
        person_index: usize,
        medication_index: usize,
        decision: AifaDecision,
    ) {
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
        thread::spawn(move || {
            let result = save_aifa_decision(
                &metadata_directory,
                metadata,
                &medication,
                decision,
                probe_leaflet_documents,
            );
            let _ = sender.send(DesktopEvent::AifaDecisionSaved {
                person_id,
                medication_id,
                result,
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn prepare_usb_image(&mut self) {
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
        thread::spawn(move || {
            let result = assemble_usb_image(&config, &builds, destination)
                .map_err(|error| format!("{}: {}", error.code().as_str(), error.message()));
            let _ = sender.send(DesktopEvent::ImagePrepared(result));
        });
    }

    #[cfg(target_os = "linux")]
    fn choose_usb_target(&mut self) {
        let Some(target) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        let worker_target = target.clone();
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        self.progress = None;
        self.status = "Lettura del filesystem e dello spazio libero del volume…".to_owned();
        thread::spawn(move || {
            let result = inspect_linux_volume(&worker_target)
                .map_err(|error| format!("{}: {}", error.code().as_str(), error.message()));
            let _ = sender.send(DesktopEvent::TargetInspected(result));
        });
        self.usb_target = Some(target);
        self.usb_volume = None;
        self.confirm_usb_target = false;
        self.usb_copy = None;
    }

    #[cfg(target_os = "linux")]
    fn copy_usb_image(&mut self) {
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
        thread::spawn(move || {
            let run = mirror_validated_usb_image_on_linux_volume(
                &config,
                &image.root,
                &target,
                &CancellationToken::default(),
            );
            let _ = sender.send(DesktopEvent::UsbCopied(run.outcome));
        });
    }

    fn receive_worker_events(&mut self) {
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
    }
}

fn resolve_session_path(base_directory: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_directory.join(path)
    }
}

fn review_profile(config: &AccountsConfig, root: &Path) -> Result<ReviewSummary, String> {
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
}

fn import_selected_observations(
    people: &[PersonConfig],
) -> Result<ObservationImportSummary, String> {
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
}

fn load_aifa_people(people: &[PersonConfig]) -> Result<Vec<AifaPersonReview>, String> {
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
}

fn candidates_for_review(search: &AifaCandidateSearch) -> &[AifaCandidate] {
    if search.exact.is_empty() {
        &search.review
    } else {
        &search.exact
    }
}

fn show_aifa_candidate(ui: &mut egui::Ui, candidate: &AifaCandidate) {
    ui.label(format!(
        "Principi attivi: {}",
        candidate.active_ingredients.join(", ")
    ));
    ui.label(format!(
        "Forma: {}",
        candidate.form.as_deref().unwrap_or("non indicata")
    ));
    ui.label(format!(
        "Dosaggio: {}",
        candidate.strength.as_deref().unwrap_or("non indicato")
    ));
    ui.label(format!(
        "Titolare: {}",
        candidate.company.as_deref().unwrap_or("non indicato")
    ));
    ui.label(format!("ATC: {}", candidate.atc.join(", ")));
    ui.label(format!("Vie: {}", candidate.routes.join(", ")));
    ui.label(format!("Confezioni: {}", candidate.packages.join("; ")));
    ui.label(format!(
        "Identificativo AIFA: {}/{}",
        candidate.codice_sis, candidate.aic6
    ));
    if !candidate.mismatches.is_empty() {
        ui.colored_label(
            warning_text_color(ui.visuals().dark_mode),
            format!("Campi da verificare: {}", candidate.mismatches.join(", ")),
        );
    }
}

fn save_aifa_decision(
    metadata_directory: &Path,
    metadata: CuratedMetadata,
    medication: &Medication,
    decision: AifaDecision,
    probe: impl FnOnce(&MedicationLeaflet) -> Vec<LeafletDownloadFailure>,
) -> Result<AifaDecisionSaved, String> {
    save_aifa_decision_with(metadata_directory, metadata, medication, decision, probe)
}

fn save_aifa_decision_with(
    metadata_directory: &Path,
    metadata: CuratedMetadata,
    medication: &Medication,
    decision: AifaDecision,
    probe: impl FnOnce(&MedicationLeaflet) -> Vec<LeafletDownloadFailure>,
) -> Result<AifaDecisionSaved, String> {
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
}

fn build_selected_people(
    people: Vec<PersonConfig>,
    cancellation: CancellationToken,
    sender: Sender<DesktopEvent>,
) {
    build_selected_people_with(people, cancellation, sender, |person| {
        build_patient(person, BuildOptions::default())
    });
}

fn build_selected_people_with(
    people: Vec<PersonConfig>,
    cancellation: CancellationToken,
    sender: Sender<DesktopEvent>,
    mut build: impl FnMut(
        &PersonConfig,
    ) -> Result<PatientBuildResult, sanikey_core::operation::OperationError>,
) {
    let total = people.len() as u64;
    let _ = sender.send(DesktopEvent::Progress(OperationEvent::Started {
        operation: "build.generate",
    }));
    let run = run_operation("build.generate", &cancellation, |context| {
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
    });
    if let Some(event) = run.events.last() {
        let _ = sender.send(DesktopEvent::Terminal(event.clone()));
    }
    let _outcome = run.outcome;
}

fn selection_from_person(person: &PersonConfig) -> PersonSelection {
    PersonSelection {
        selected: person.enabled,
        source_documents: person.source_documents.clone(),
        metadata_directory: person.metadata_directory.clone(),
        local_build: person.local_build.clone(),
    }
}

fn choose_folder(ui: &mut egui::Ui, label_text: &str, path: &mut PathBuf) {
    let label = ui.label(label_text);
    let mut text = path.display().to_string();
    ui.horizontal_wrapped(|ui| {
        let response = add_responsive_path_edit(ui, label.id, &mut text, 100.0);
        if response.changed() {
            *path = PathBuf::from(&text);
        }
        if ui.button("Scegli…").clicked()
            && let Some(chosen) = rfd::FileDialog::new().pick_folder()
        {
            *path = chosen;
        }
    });
}

fn add_responsive_path_edit(
    ui: &mut egui::Ui,
    label_id: egui::Id,
    text: &mut String,
    trailing_controls_width: f32,
) -> egui::Response {
    let width = (ui.available_width() - trailing_controls_width).max(MIN_PATH_FIELD_WIDTH);
    ui.add_sized(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::TextEdit::singleline(text),
    )
    .labelled_by(label_id)
}

fn desktop_visuals() -> egui::Visuals {
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
}

fn configure_desktop_theme(context: &egui::Context) {
    context.set_theme(egui::Theme::Light);
    context.set_visuals(desktop_visuals());
}

fn show_path(ui: &mut egui::Ui, label_text: &str, path: &Path) {
    ui.label(label_text);
    ui.add(
        egui::Label::new(egui::RichText::new(path.display().to_string()).monospace())
            .wrap_mode(egui::TextWrapMode::Wrap),
    );
}

/// Sceglie un rosso leggibile sia sul fondo chiaro sia su quello scuro.
fn warning_text_color(dark_mode: bool) -> egui::Color32 {
    if dark_mode {
        egui::Color32::LIGHT_RED
    } else {
        egui::Color32::DARK_RED
    }
}

/// Collega gli eventi del core ai controlli e allo stato visuale.
impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
    }
}

/// Rende l'intera vista desktop scorrevole sui viewport ridotti.
impl DesktopApp {
    fn show_contents(&mut self, ui: &mut egui::Ui) {
        self.receive_worker_events();
        let busy = self.receiver.is_some();
        if !busy
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(Modifiers::CTRL, Key::O))
        {
            self.open_profile_dialog();
        }

        ui.heading("SaniKey");
        ui.label("Importa un profilo e scegli persone e cartelle per questa sessione.");
        let can_edit = !busy && self.review.is_none();
        let mut open_profile = false;
        ui.add_enabled_ui(can_edit, |ui| {
            let label = ui.label("Profilo account");
            ui.horizontal_wrapped(|ui| {
                add_responsive_path_edit(ui, label.id, &mut self.profile_path, 180.0);
                open_profile = ui.button("Apri profilo… (Ctrl+O)").clicked();
            });
        });
        if open_profile {
            self.open_profile_dialog();
        }
        ui.label("Le scelte di questa schermata non modificano il file del profilo.");
        ui.label("I percorsi relativi digitati sono risolti rispetto alla base del profilo.");

        if let Some(profile) = &self.profile {
            ui.separator();
            show_path(ui, "Base percorsi:", profile.base_directory());
            if !self.selections.iter().any(|person| person.selected) {
                ui.colored_label(
                    warning_text_color(ui.visuals().dark_mode),
                    "Nessuna persona selezionata: selezionane almeno una.",
                );
            }
            ui.add_enabled_ui(can_edit, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (person, selection) in
                        profile.accounts().people.iter().zip(&mut self.selections)
                    {
                        egui::CollapsingHeader::new(format!(
                            "{} ({})",
                            person.display_name, person.id
                        ))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.checkbox(&mut selection.selected, "Includi questa persona");
                            choose_folder(ui, "Documenti", &mut selection.source_documents);
                            choose_folder(ui, "Metadati", &mut selection.metadata_directory);
                            choose_folder(ui, "Destinazione build", &mut selection.local_build);
                        });
                    }
                });
            });
        } else {
            ui.colored_label(
                warning_text_color(ui.visuals().dark_mode),
                "Profilo non caricato.",
            );
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let selected = self.selections.iter().any(|person| person.selected);
            if ui
                .add_enabled(
                    !busy && self.profile.is_some() && selected && self.review.is_none(),
                    egui::Button::new("Controlla preflight e anomalie"),
                )
                .clicked()
            {
                self.start_preflight();
            }
            if self.review.is_some() && !busy {
                if ui.button("Modifica selezione").clicked() {
                    self.review = None;
                    self.aifa_people = None;
                }
                if ui.button("Rifai preflight").clicked() {
                    self.review = None;
                    self.aifa_people = None;
                    self.start_preflight();
                }
                if ui
                    .add_enabled(selected, egui::Button::new("Importa/aggiorna osservazioni"))
                    .clicked()
                {
                    self.start_observation_import();
                }
                if ui
                    .add_enabled(selected, egui::Button::new("Rivedi riferimenti AIFA"))
                    .clicked()
                {
                    self.start_aifa_review();
                }
                if ui
                    .add_enabled(selected, egui::Button::new("Avvia build incrementale"))
                    .clicked()
                {
                    self.start_build();
                }
            }
            if let Some(cancellation) = &self.cancellation
                && ui.button("Annulla dopo la persona corrente").clicked()
            {
                cancellation.cancel();
                self.status = "Annullamento richiesto; attendo il punto sicuro.".to_owned();
            }
        });

        #[cfg(target_os = "linux")]
        if !self.completed_builds.is_empty() && self.receiver.is_none() {
            ui.separator();
            ui.heading("Export USB Linux");
            if let Some(image) = &self.usb_image {
                ui.label(format!(
                    "Immagine verificata: {} persone · {} file · {} byte",
                    image.patients, image.files, image.bytes
                ));
                show_path(ui, "Percorso immagine:", &image.root);
            } else if ui.button("Prepara e valida immagine USB").clicked() {
                self.prepare_usb_image();
            }
            if self.usb_image.is_some() {
                if ui.button("Scegli volume Linux…").clicked() {
                    self.choose_usb_target();
                }
                if let Some(target) = &self.usb_target {
                    show_path(ui, "Volume selezionato:", target);
                    show_path(ui, "Cartella di export:", &target.join("SaniKey"));
                }
                if let Some(volume) = &self.usb_volume {
                    show_path(
                        ui,
                        "Mount del volume:",
                        volume
                            .mount_target
                            .as_deref()
                            .unwrap_or(Path::new("sconosciuto")),
                    );
                    ui.label(format!(
                        "Filesystem: {} · UUID: {} · liberi: {} byte",
                        volume.filesystem.as_deref().unwrap_or("sconosciuto"),
                        volume.uuid.as_deref().unwrap_or("sconosciuto"),
                        volume.available_bytes
                    ));
                    ui.colored_label(warning_text_color(ui.visuals().dark_mode),
                        "La copia aggiorna la cartella SaniKey nel volume e rimuove da quella cartella i file che non appartengono all'immagine. Gli altri file del volume restano intatti.");
                    ui.checkbox(&mut self.confirm_usb_target, "Confermo che il volume mostrato è quello corretto e autorizzo l'aggiornamento della cartella SaniKey.");
                    if ui
                        .add_enabled(
                            self.confirm_usb_target && self.receiver.is_none(),
                            egui::Button::new("Copia, verifica e mostra il risultato"),
                        )
                        .clicked()
                    {
                        self.copy_usb_image();
                    }
                }
                if let Some(copy) = &self.usb_copy {
                    ui.label(format!("Report copia: {} copiati · {} riutilizzati · {} rimossi · {} byte scritti · target validato.", copy.copied_files, copy.reused_files, copy.removed_files, copy.bytes_copied));
                }
            }
        }

        if let Some(review) = &self.review {
            ui.heading("Riepilogo preflight");
            ui.label(format!(
                "Persone: {} · Documenti: {} · Gruppi duplicati: {}",
                review.selected_people.len(),
                review.document_count,
                review.duplicate_groups.len()
            ));
            egui::CollapsingHeader::new("Rivedi i documenti duplicati")
                .default_open(!review.duplicate_groups.is_empty())
                .show(ui, |ui| {
                    if review.duplicate_groups.is_empty() {
                        ui.label("Nessun duplicato rilevato.");
                    }
                    for (person, digest, paths) in &review.duplicate_groups {
                        ui.label(format!("{person} · SHA-256 {digest}"));
                        for path in paths {
                            show_path(ui, "Documento:", path);
                        }
                    }
                });
            ui.label("Importa o aggiorna le osservazioni prima della build; i metadati curati restano invariati.");
        }
        if let Some(people) = &mut self.aifa_people {
            ui.separator();
            ui.heading("Riferimenti farmaceutici AIFA");
            ui.label("Le ricerche online vengono eseguite fuori dalla finestra. Il riferimento si salva solo dopo conferma e verifica di FI e RCP.");
            let mut action = None;
            for (person_index, person) in people.iter_mut().enumerate() {
                egui::CollapsingHeader::new(format!(
                    "{} ({}) · {} farmaci",
                    person.display_name,
                    person.id,
                    person.medications.len()
                ))
                .default_open(true)
                .show(ui, |ui| {
                    for (medication_index, item) in person.medications.iter_mut().enumerate() {
                        ui.group(|ui| {
                            ui.heading(&item.medication.name);
                            ui.label(format!("ID: {}", item.medication.id));
                            for therapy in person
                                .metadata
                                .therapies
                                .iter()
                                .filter(|therapy| therapy.medication_id == item.medication.id)
                            {
                                ui.label(format!(
                                    "Terapia curata: {} · {} · {}",
                                    therapy.role.as_deref().unwrap_or("ruolo non indicato"),
                                    therapy
                                        .dosage
                                        .as_deref()
                                        .unwrap_or("posologia non indicata"),
                                    therapy.schedule.join(", ")
                                ));
                                if let Some(instructions) = &therapy.instructions {
                                    ui.label(format!("Istruzioni: {instructions}"));
                                }
                            }
                            if let Some(reference) = person
                                .metadata
                                .medication_leaflets
                                .iter()
                                .find(|reference| reference.medication_id == item.medication.id)
                            {
                                ui.label(format!(
                                    "Riferimento registrato: AIFA {}/{}",
                                    reference.codice_sis, reference.aic6
                                ));
                            }
                            if person
                                .metadata
                                .medication_leaflet_exclusions
                                .iter()
                                .any(|exclusion| exclusion.medication_id == item.medication.id)
                            {
                                ui.label("Stato registrato: non applicabile ad AIFA.");
                            }
                            ui.horizontal_wrapped(|ui| {
                                let label = ui.label("Ricerca manuale");
                                ui.text_edit_singleline(&mut item.query)
                                    .labelled_by(label.id);
                                if ui
                                    .add_enabled(
                                        !busy,
                                        egui::Button::new("Cerca nome e principio attivo"),
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
                                        egui::Button::new("Cerca testo"),
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
                                    ui.label("Nessun candidato da confermare.");
                                }
                                for (candidate_index, candidate) in candidates.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut item.selected_candidate,
                                        Some(candidate_index),
                                        format!(
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
                                        ),
                                    );
                                }
                                if let Some(candidate) = item
                                    .selected_candidate
                                    .and_then(|index| candidates.get(index))
                                {
                                    show_aifa_candidate(ui, candidate);
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            egui::Button::new(
                                                "Conferma candidato e verifica FI/RCP",
                                            ),
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
                                .add_enabled(!busy, egui::Button::new("Conferma stato non AIFA"))
                                .clicked()
                            {
                                action = Some(AifaUiAction::MarkNotApplicable(
                                    person_index,
                                    medication_index,
                                ));
                            }
                            ui.label(&item.status);
                        });
                    }
                });
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
            ui.collapsing("Risultato importazione osservazioni", |ui| {
                ui.label(format!(
                    "{} persone · {} serie · {} osservazioni · {} avvisi",
                    summary.people,
                    summary.series,
                    summary.points,
                    summary.warnings.len()
                ));
                for warning in &summary.warnings {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(warning)
                                .color(warning_text_color(ui.visuals().dark_mode)),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                }
            });
        }
        if !self.completed_people.is_empty() {
            ui.label(format!(
                "Build completate: {}",
                self.completed_people.join(", ")
            ));
        }
        if self.receiver.is_some() || self.cancellation.is_some() {
            if let Some(progress) = self.progress {
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            } else {
                indeterminate_progress_bar(ui);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        ui.separator();
        ui.add(egui::Label::new(&self.status).wrap_mode(egui::TextWrapMode::Wrap));
    }
}

fn indeterminate_progress_bar(ui: &mut egui::Ui) {
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
}

fn indeterminate_progress_segment(track: egui::Rect, time: f64) -> egui::Rect {
    let segment_width = track.width() * 0.28;
    let phase = (time / 1.2).rem_euclid(2.0);
    let position = (if phase <= 1.0 { phase } else { 2.0 - phase }) as f32;
    let left = track.left() + (track.width() - segment_width) * position;
    egui::Rect::from_min_size(
        egui::pos2(left, track.top()),
        egui::vec2(segment_width, track.height()),
    )
}

fn main() -> eframe::Result {
    eframe::run_native(
        "SaniKey",
        eframe::NativeOptions::default(),
        Box::new(|creation_context| {
            configure_desktop_theme(&creation_context.egui_ctx);
            Ok(Box::<DesktopApp>::default())
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AifaCandidate, AifaCandidateSearch, AifaDecision, AifaMedicationReview, AifaPersonReview,
        BLUE_TEXT, DesktopApp, DesktopEvent, FIELD_BACKGROUND, LeafletDownloadFailure,
        ReviewSummary, STRAW_BACKGROUND, add_responsive_path_edit, build_selected_people,
        build_selected_people_with, configure_desktop_theme, desktop_visuals,
        import_selected_observations, indeterminate_progress_segment, load_aifa_people,
        review_profile, save_aifa_decision_with, show_path, warning_text_color,
    };
    use eframe::egui;
    use sanikey_core::build::{BuildCounts, BuildOptions, PatientBuildResult, build_patient};
    use sanikey_core::config::AccountsProfile;
    use sanikey_core::metadata::{
        CuratedMetadata, Medication, MedicationLeafletExclusion, load_curated_metadata,
    };
    use sanikey_core::operation::{CancellationToken, OperationEvent};
    use std::{fs, path::PathBuf, time::SystemTime};

    struct TestDirectory(PathBuf);

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
        let mut app = DesktopApp {
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
                    egui::vec2(1000.0, 2200.0),
                )),
                ..Default::default()
            },
            |ui| eframe::App::ui(&mut app, ui, &mut frame),
        );
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
        let manual_search_is_labelled = tree
            .nodes
            .iter()
            .find(|(_, node)| node.role() == egui::accesskit::Role::TextInput)
            .and_then(|(_, node)| node.labelled_by().first())
            .and_then(|label_id| tree.nodes.iter().find(|(node_id, _)| node_id == label_id))
            .and_then(|(_, node)| node.value())
            == Some("Ricerca manuale");

        output.drop_without_applying_deltas();
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
