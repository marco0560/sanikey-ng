//! Interfaccia desktop SaniKey per importare un profilo e scegliere persone e cartelle.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, Modifiers};
use sanikey_core::config::{AccountsProfile, PersonConfig};

#[derive(Clone, Debug)]
struct PersonSelection {
    selected: bool,
    source_documents: PathBuf,
    metadata_directory: PathBuf,
    local_build: PathBuf,
}

struct DesktopApp {
    profile_path: String,
    profile: Option<AccountsProfile>,
    selections: Vec<PersonSelection>,
    status: String,
}

impl Default for DesktopApp {
    fn default() -> Self {
        Self {
            profile_path: String::new(),
            profile: None,
            selections: Vec::new(),
            status: "Aprire un profilo account per iniziare".to_owned(),
        }
    }
}

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
            }
            Err(error) => {
                self.profile = None;
                self.selections.clear();
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
    ui.horizontal(|ui| {
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

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui
            .ctx()
            .input_mut(|input| input.consume_key(Modifiers::CTRL, Key::O))
        {
            self.open_profile_dialog();
        }

        ui.heading("SaniKey");
        ui.label("Importa un profilo e scegli persone e cartelle per questa sessione.");
        ui.horizontal(|ui| {
            let label = ui.label("Profilo account");
            ui.text_edit_singleline(&mut self.profile_path)
                .labelled_by(label.id);
            if ui.button("Apri profilo… (Ctrl+O)").clicked() {
                self.open_profile_dialog();
            }
        });
        ui.label("Le scelte di questa schermata non modificano il file del profilo.");

        if let Some(profile) = &self.profile {
            ui.separator();
            ui.label(format!(
                "Base percorsi: {}",
                profile.base_directory().display()
            ));
            if self.selections.iter().all(|person| !person.selected) {
                ui.colored_label(
                    egui::Color32::DARK_RED,
                    "Nessuna persona selezionata: selezionane almeno una.",
                );
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (person, selection) in
                    profile.accounts().people.iter().zip(&mut self.selections)
                {
                    egui::CollapsingHeader::new(format!("{} ({})", person.display_name, person.id))
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.checkbox(&mut selection.selected, "Includi questa persona");
                            choose_folder(ui, "Documenti", &mut selection.source_documents);
                            choose_folder(ui, "Metadati", &mut selection.metadata_directory);
                            choose_folder(ui, "Destinazione build", &mut selection.local_build);
                        });
                }
            });
        } else {
            ui.colored_label(egui::Color32::DARK_RED, "Profilo non caricato.");
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
    use super::DesktopApp;
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
}
