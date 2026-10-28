//! Fixture sintetiche portate dai test di contratto Python della configurazione.

use super::{default_accounts_path, load_accounts, parse_accounts_data};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use toml::Value;

pub(in crate::config) static CURRENT_DIRECTORY_LOCK: Mutex<()> = Mutex::new(());

/// Conserva fixture sintetiche isolate per i test di configurazione.
pub(in crate::config) struct TestDirectory(PathBuf);

/// Crea e accede alla directory temporanea del test.
impl TestDirectory {
    pub(in crate::config) fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-config-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    pub(in crate::config) fn path(&self) -> &Path {
        &self.0
    }

    fn accounts_file(&self, source: &str) -> PathBuf {
        let path = self.path().join("accounts.toml");
        fs::write(&path, source).expect("file account sintetico");
        path
    }
}

/// Rimuove le fixture sintetiche quando il test termina.
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Ripristina la directory corrente dopo i test che verificano i percorsi relativi.
pub(in crate::config) struct WorkingDirectory(PathBuf);

/// Imposta la directory di lavoro isolata per il test.
impl WorkingDirectory {
    pub(in crate::config) fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().expect("directory corrente");
        std::env::set_current_dir(path).expect("impostazione directory temporanea");
        Self(previous)
    }
}

/// Ripristina la directory di lavoro iniziale.
impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("ripristino directory corrente");
    }
}

fn accounts_text(root: &Path) -> String {
    format!(
        r##"
        [global]
        config_version = 1

        [[person]]
        id = "patient-a"
        display_name = "Patient A"
        source_documents = "{}"
        metadata_directory = "{}"
        local_build = "{}"
        usb_uuid = "1A2B-3C4D"
        "##,
        root.join("source").display(),
        root.join("metadata").display(),
        root.join("generated").display(),
    )
}

fn parse_value(source: &str) -> Value {
    toml::from_str(source).expect("TOML sintetico valido")
}

fn message<T: std::fmt::Debug>(result: Result<T, crate::operation::OperationError>) -> String {
    result
        .expect_err("la fixture deve essere rifiutata")
        .message()
        .to_owned()
}

mod loading;
mod nested;
mod paths;
mod person;
mod validation;
