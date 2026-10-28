//! Fixture temporanee sintetiche condivise dai test metadata.

use crate::metadata::{CuratedMetadata, load_curated_metadata};
use crate::operation::OperationError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) struct TestDirectory(PathBuf);

impl TestDirectory {
    pub(super) fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-metadata-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }

    pub(super) fn write(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().expect("parent del fixture")).expect("directory fixture");
        fs::write(path, contents).expect("file TOML sintetico");
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn metadata(path: &Path) -> CuratedMetadata {
    load_curated_metadata(path).expect("metadati sintetici validi")
}

pub(super) fn error_message(path: &Path) -> String {
    let result: Result<CuratedMetadata, OperationError> = load_curated_metadata(path);
    result
        .expect_err("metadati sintetici non validi")
        .message()
        .to_owned()
}
