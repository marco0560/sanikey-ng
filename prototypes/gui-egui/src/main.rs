//! Prototipo eframe per confrontare l'interazione GUI con eventi sintetici.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use eframe::egui::{self, Key, Modifiers};
use egui_extras::{Column, TableBuilder};
use sanikey_prototype_gui_protocol::{
    DemoScenario, GuiOperationEvent as OperationEvent, demonstration_events,
};

struct Prototype {
    source: String,
    destination: String,
    progress: u8,
    indeterminate: bool,
    running: bool,
    fail_on_finish: bool,
    dark_theme: bool,
    next_tick: Instant,
    status: String,
    events: VecDeque<OperationEvent>,
}

/// Inizializza lo stato della dimostrazione GUI.
impl Default for Prototype {
    fn default() -> Self {
        Self {
            source: String::new(),
            destination: String::new(),
            progress: 0,
            indeterminate: false,
            running: false,
            fail_on_finish: false,
            dark_theme: false,
            next_tick: Instant::now(),
            status: "Pronto".to_owned(),
            events: VecDeque::new(),
        }
    }
}

/// Gestisce stato, eventi e tempi della dimostrazione.
impl Prototype {
    fn start(&mut self) {
        self.progress = 0;
        self.indeterminate = false;
        self.running = true;
        self.status = "Operazione in corso".to_owned();
        let scenario = if self.fail_on_finish {
            DemoScenario::Error
        } else {
            DemoScenario::Success
        };
        self.events = demonstration_events(scenario).into();
        self.next_tick = Instant::now() + Duration::from_millis(250);
    }

    fn cancel(&mut self) {
        if self.running {
            self.running = false;
            let terminal = demonstration_events(DemoScenario::Cancel)
                .pop()
                .expect("the demo operation emits a terminal event");
            self.apply_event(terminal);
        }
    }

    fn advance(&mut self) {
        if !self.running || Instant::now() < self.next_tick {
            return;
        }
        let Some(event) = self.events.pop_front() else {
            self.running = false;
            return;
        };
        let terminal = matches!(
            &event,
            OperationEvent::Succeeded | OperationEvent::Failed(_) | OperationEvent::Cancelled
        );
        self.apply_event(event);
        self.next_tick = Instant::now() + Duration::from_millis(250);
        if terminal {
            self.running = false;
        }
    }

    fn apply_event(&mut self, event: OperationEvent) {
        match event {
            OperationEvent::Started { .. } => self.status = "Operazione in corso".to_owned(),
            OperationEvent::Progress {
                completed, total, ..
            } => {
                self.indeterminate = total.is_none();
                self.progress = total
                    .filter(|total| *total > 0)
                    .map_or(0, |total| (completed * 100 / total) as u8);
            }
            OperationEvent::Succeeded => {
                self.indeterminate = false;
                self.status = "Operazione completata".to_owned();
            }
            OperationEvent::Failed(error) => {
                self.indeterminate = false;
                self.status = format!("{}: {}", error.code().as_str(), error.message());
            }
            OperationEvent::Cancelled => {
                self.indeterminate = false;
                self.status = "Operazione annullata (cancelled)".to_owned();
            }
        }
    }
}

/// Collega lo stato della demo al rendering e agli input egui.
impl eframe::App for Prototype {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        context.set_visuals(if self.dark_theme {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });

        if context.input_mut(|input| input.consume_key(Modifiers::CTRL, Key::Enter)) {
            self.start();
        }
        if context.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.cancel();
        }

        self.advance();
        ui.heading("SaniKey — prototipo egui");
        ui.label("Confronto UI con fixture sintetiche; nessun dato viene letto o scritto.");

        ui.horizontal(|ui| {
            let label = ui.label("Cartella sorgente");
            ui.text_edit_singleline(&mut self.source)
                .labelled_by(label.id);
            if ui.button("Sfoglia sorgente…").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.source = path.display().to_string();
            }
        });
        ui.horizontal(|ui| {
            let label = ui.label("Cartella destinazione");
            ui.text_edit_singleline(&mut self.destination)
                .labelled_by(label.id);
            if ui.button("Sfoglia destinazione…").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                self.destination = path.display().to_string();
            }
        });

        ui.separator();
        ui.label("File rilevati (fixture sintetiche)");
        TableBuilder::new(ui)
            .striped(true)
            .column(Column::auto())
            .column(Column::remainder())
            .header(24.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Nome");
                });
                header.col(|ui| {
                    ui.strong("Stato");
                });
            })
            .body(|mut body| {
                for (name, state) in [
                    ("paziente-demo-001", "Pronto"),
                    ("paziente-demo-002", "Pronto"),
                    ("paziente-demo-003", "Pronto"),
                ] {
                    body.row(22.0, |mut row| {
                        row.col(|ui| {
                            ui.label(name);
                        });
                        row.col(|ui| {
                            ui.label(state);
                        });
                    });
                }
            });

        ui.separator();
        ui.checkbox(&mut self.fail_on_finish, "Simula errore terminale");
        ui.checkbox(&mut self.dark_theme, "Tema scuro");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.running, egui::Button::new("Avvia demo (Ctrl+Invio)"))
                .clicked()
            {
                self.start();
            }
            if ui
                .add_enabled(self.running, egui::Button::new("Annulla (Esc)"))
                .clicked()
            {
                self.cancel();
            }
        });
        let progress_bar = if self.indeterminate {
            egui::ProgressBar::new(0.3)
                .animate(true)
                .text("Avanzamento non determinabile")
        } else {
            egui::ProgressBar::new(f32::from(self.progress) / 100.0)
                .text(format!("{}%", self.progress))
        };
        ui.add(progress_bar);
        ui.label(format!("Stato: {}", self.status));

        if self.running {
            context.request_repaint_after(Duration::from_millis(30));
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "SaniKey — prototipo egui",
        options,
        Box::new(|_creation_context| Ok(Box::<Prototype>::default())),
    )
}
