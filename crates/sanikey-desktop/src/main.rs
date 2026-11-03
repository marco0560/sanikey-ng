//! Interfaccia desktop SaniKey per importare un profilo e scegliere persone e cartelle.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "linux")]
static NEXT_USB_IMAGE: AtomicUsize = AtomicUsize::new(0);

use eframe::egui::{self, Key, Modifiers};
use sanikey_core::build::{BuildOptions, PatientBuildResult, build_patient};
use sanikey_core::config::{AccountsConfig, AccountsProfile, PersonConfig};
use sanikey_core::inventory::{find_duplicate_documents, scan_document_inventory};
#[cfg(target_os = "linux")]
use sanikey_core::linux_volume::{LinuxVolumeInfo, inspect_linux_volume};
use sanikey_core::metadata::prepare_people_metadata;
#[cfg(target_os = "linux")]
use sanikey_core::operation::OperationOutcome;
use sanikey_core::operation::{CancellationToken, OperationEvent, OperationPhase, run_operation};
#[cfg(target_os = "linux")]
use sanikey_core::usb_copy::{UsbCopyResult, mirror_validated_usb_image_on_linux_volume};
use sanikey_core::usb_image::UsbImageResult;
#[cfg(target_os = "linux")]
use sanikey_core::usb_image::assemble_usb_image;

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

enum DesktopEvent {
    Preflight(Result<ReviewSummary, String>),
    PatientBuilt(PatientBuildResult),
    ImagePrepared(Result<UsbImageResult, String>),
    #[cfg(target_os = "linux")]
    TargetInspected(Result<LinuxVolumeInfo, String>),
    #[cfg(target_os = "linux")]
    UsbCopied(OperationOutcome<UsbCopyResult>),
    Progress(OperationEvent),
    Terminal(OperationEvent),
}

struct DesktopApp {
    profile_path: String,
    profile: Option<AccountsProfile>,
    selections: Vec<PersonSelection>,
    status: String,
    receiver: Option<Receiver<DesktopEvent>>,
    cancellation: Option<CancellationToken>,
    review: Option<ReviewSummary>,
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
        self.review = None;
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
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label_text);
        let mut text = path.display().to_string();
        let response = ui.text_edit_singleline(&mut text).labelled_by(label.id);
        if response.changed() {
            *path = PathBuf::from(text);
        }
        if ui.button("Scegli…").clicked()
            && let Some(chosen) = rfd::FileDialog::new().pick_folder()
        {
            *path = chosen;
        }
    });
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
        egui::ScrollArea::vertical().show(ui, |ui| self.show_contents(ui));
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
            ui.horizontal_wrapped(|ui| {
                let label = ui.label("Profilo account");
                ui.text_edit_singleline(&mut self.profile_path)
                    .labelled_by(label.id);
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
            ui.label(format!(
                "Base percorsi: {}",
                profile.base_directory().display()
            ));
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
                }
                if ui.button("Rifai preflight").clicked() {
                    self.review = None;
                    self.start_preflight();
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
                ui.monospace(image.root.display().to_string());
            } else if ui.button("Prepara e valida immagine USB").clicked() {
                self.prepare_usb_image();
            }
            if self.usb_image.is_some() {
                if ui.button("Scegli volume Linux…").clicked() {
                    self.choose_usb_target();
                }
                if let Some(target) = &self.usb_target {
                    ui.label(format!(
                        "Volume selezionato: {} · cartella di export: {}",
                        target.display(),
                        target.join("SaniKey").display()
                    ));
                }
                if let Some(volume) = &self.usb_volume {
                    ui.label(format!(
                        "Mount: {} · filesystem: {} · UUID: {} · liberi: {} byte",
                        volume
                            .mount_target
                            .as_deref()
                            .unwrap_or(Path::new("sconosciuto"))
                            .display(),
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
                            ui.monospace(path.display().to_string());
                        }
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
                ui.add(
                    egui::ProgressBar::new(0.3)
                        .animate(true)
                        .text("Elaborazione in corso"),
                );
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        ui.separator();
        ui.label(&self.status);
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "SaniKey",
        eframe::NativeOptions::default(),
        Box::new(|_creation_context| Ok(Box::<DesktopApp>::default())),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopApp, DesktopEvent, build_selected_people, build_selected_people_with,
        review_profile, warning_text_color,
    };
    use eframe::egui;
    use sanikey_core::build::{BuildCounts, PatientBuildResult};
    use sanikey_core::config::AccountsProfile;
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
            assert_eq!(context.theme(), theme);
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
}
