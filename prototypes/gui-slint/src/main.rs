use std::error::Error;

slint::slint! {
    import { Button, CheckBox, HorizontalBox, LineEdit, ProgressIndicator,
             StandardTableView, VerticalBox } from "std-widgets.slint";

    export component MainWindow inherits Window {
        title: "SaniKey — prototipo Slint";
        min-width: 800px;
        min-height: 560px;

        in-out property <string> source-path: "Non selezionata";
        in-out property <string> destination-path: "Non selezionata";
        in-out property <int> progress: 0;
        in-out property <bool> indeterminate: false;
        in-out property <bool> running: false;
        in-out property <bool> fail-on-finish: false;
        in-out property <string> status: "Pronto";

        callback choose-source();
        callback choose-destination();
        callback start-operation();
        callback cancel-operation();
        callback tick();

        accessible-role: main;
        accessible-label: "Prototipo di confronto GUI SaniKey";

        VerticalBox {
            padding: 16px;
            spacing: 8px;

            Text {
                text: "SaniKey — prototipo Slint";
                font-size: 22px;
            }
            Text {
                text: "Confronto UI con fixture sintetiche; nessun dato viene letto o scritto.";
                accessible-role: text;
            }

            HorizontalBox {
                Text { text: "Cartella sorgente"; vertical-alignment: center; }
                LineEdit {
                    text <=> root.source-path;
                    accessible-label: "Cartella sorgente";
                }
                Button {
                    text: "Sfoglia sorgente…";
                    clicked => { root.choose-source(); }
                }
            }
            HorizontalBox {
                Text { text: "Cartella destinazione"; vertical-alignment: center; }
                LineEdit {
                    text <=> root.destination-path;
                    accessible-label: "Cartella destinazione";
                }
                Button {
                    text: "Sfoglia destinazione…";
                    clicked => { root.choose-destination(); }
                }
            }

            Text { text: "File rilevati (fixture sintetiche)"; }
            StandardTableView {
                columns: [ { title: "Nome" }, { title: "Stato" } ];
                accessible-role: table;
                accessible-label: "File rilevati";
                rows: [
                    [ { text: "paziente-demo-001" }, { text: "Pronto" } ],
                    [ { text: "paziente-demo-002" }, { text: "Pronto" } ],
                    [ { text: "paziente-demo-003" }, { text: "Pronto" } ]
                ];
            }

            CheckBox {
                text: "Simula errore terminale";
                checked <=> root.fail-on-finish;
            }
            Text { text: "Tema: selezionare SLINT_STYLE fluent-light o fluent-dark in compilazione."; }
            HorizontalBox {
                Button {
                    text: "Avvia demo";
                    enabled: !root.running;
                    clicked => { root.start-operation(); }
                }
                Button {
                    text: "Annulla";
                    enabled: root.running;
                    clicked => { root.cancel-operation(); }
                }
            }
            ProgressIndicator {
                progress: root.progress / 100;
                indeterminate: root.indeterminate;
            }
            Text { text: root.progress + "%"; }
            Text { text: "Stato: " + root.status; }
        }

        Timer {
            interval: 250ms;
            running: root.running;
            triggered => { root.tick(); }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    use sanikey_prototype_gui_protocol::{
        DemoScenario, GuiOperationEvent as OperationEvent, demonstration_events,
    };

    let app = MainWindow::new()?;
    let events = Rc::new(RefCell::new(VecDeque::<OperationEvent>::new()));

    let weak_app = app.as_weak();
    let start_events = Rc::clone(&events);
    app.on_start_operation(move || {
        let Some(app) = weak_app.upgrade() else {
            return;
        };
        let scenario = if app.get_fail_on_finish() {
            DemoScenario::Error
        } else {
            DemoScenario::Success
        };
        *start_events.borrow_mut() = demonstration_events(scenario).into();
        app.set_progress(0);
        app.set_indeterminate(false);
        app.set_status("Operazione in corso".into());
        app.set_running(true);
    });

    let weak_app = app.as_weak();
    app.on_cancel_operation(move || {
        if let Some(app) = weak_app.upgrade() {
            let terminal = demonstration_events(DemoScenario::Cancel)
                .pop()
                .expect("the demo operation emits a terminal event");
            if let OperationEvent::Cancelled = terminal {
                app.set_running(false);
                app.set_indeterminate(false);
                app.set_status("Operazione annullata (cancelled)".into());
            }
        }
    });

    let weak_app = app.as_weak();
    app.on_tick(move || {
        let Some(app) = weak_app.upgrade() else {
            return;
        };
        let Some(event) = events.borrow_mut().pop_front() else {
            app.set_running(false);
            return;
        };
        let terminal = matches!(
            &event,
            OperationEvent::Succeeded | OperationEvent::Failed(_) | OperationEvent::Cancelled
        );
        match event {
            OperationEvent::Started { .. } => app.set_status("Operazione in corso".into()),
            OperationEvent::Progress {
                completed, total, ..
            } => {
                app.set_indeterminate(total.is_none());
                let progress = total
                    .filter(|total| *total > 0)
                    .map_or(0, |total| (completed * 100 / total) as i32);
                app.set_progress(progress);
            }
            OperationEvent::Succeeded => {
                app.set_indeterminate(false);
                app.set_status("Operazione completata".into());
            }
            OperationEvent::Failed(error) => {
                app.set_indeterminate(false);
                app.set_status(format!("{}: {}", error.code().as_str(), error.message()).into())
            }
            OperationEvent::Cancelled => {
                app.set_indeterminate(false);
                app.set_status("Operazione annullata (cancelled)".into());
            }
        }
        if terminal {
            app.set_running(false);
        }
    });

    let weak_app = app.as_weak();
    app.on_choose_source(move || {
        if let Some(path) = rfd::FileDialog::new().pick_folder()
            && let Some(app) = weak_app.upgrade()
        {
            app.set_source_path(path.display().to_string().into());
        }
    });

    let weak_app = app.as_weak();
    app.on_choose_destination(move || {
        if let Some(path) = rfd::FileDialog::new().pick_folder()
            && let Some(app) = weak_app.upgrade()
        {
            app.set_destination_path(path.display().to_string().into());
        }
    });

    app.run()?;
    Ok(())
}
