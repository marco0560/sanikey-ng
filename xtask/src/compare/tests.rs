//! Test sintetici dei comparatori.

use super::files::{compare_file_hash, compare_originals, sha256_file};
use super::formats::{
    compare_frontend_directories, compare_json_values, compare_text, compare_text_files,
};
use super::sqlite::compare_sqlite_connections;
use super::sqlite_rows::fts_search_results_equal;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sanikey-ng-xtask-compare-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("directory temporanea");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn originals_match_by_sha256_and_detect_changed_bytes() {
    let directory = TestDirectory::new();
    let reference = directory.path().join("reference.txt");
    let candidate = directory.path().join("candidate.txt");
    fs::write(&reference, b"synthetic source\n").expect("fixture di riferimento");
    fs::write(&candidate, b"synthetic source\n").expect("fixture candidata");
    assert!(compare_file_hash(&reference, &candidate).is_ok());
    fs::write(&candidate, b"hello world").expect("fixture SHA nota");
    assert_eq!(
        sha256_file(&candidate).expect("SHA-256"),
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    fs::write(&candidate, b"mutated synthetic source\n").expect("mutazione");
    assert!(compare_file_hash(&reference, &candidate).is_err());
}

#[test]
fn original_directories_detect_changed_files_and_paths() {
    let directory = TestDirectory::new();
    let reference = directory.path().join("reference");
    let candidate = directory.path().join("candidate");
    fs::create_dir_all(reference.join("nested")).expect("directory di riferimento");
    fs::create_dir_all(candidate.join("nested")).expect("directory candidata");
    fs::write(reference.join("nested/source.txt"), b"same").expect("originale");
    fs::write(candidate.join("nested/source.txt"), b"same").expect("copia");
    assert!(compare_originals(&reference, &candidate).is_ok());
    fs::write(candidate.join("nested/source.txt"), b"changed").expect("mutazione");
    assert!(compare_originals(&reference, &candidate).is_err());
    fs::remove_file(candidate.join("nested/source.txt")).expect("rimozione");
    fs::write(candidate.join("nested/renamed.txt"), b"same").expect("rinomina");
    assert!(compare_originals(&reference, &candidate).is_err());
}

#[test]
fn json_object_order_and_format_are_ignored_but_values_are_not() {
    let reference: Value = serde_json::from_str(
        r#"{"date":"2026-01-02","value":70.0,"unit":"kg","source":"laboratory/peso-sintetico.csv"}"#,
    )
    .expect("JSON di riferimento");
    let reordered: Value = serde_json::from_str(
        r#"{ "source": "laboratory/peso-sintetico.csv", "unit": "kg", "value": 70.0, "date": "2026-01-02" }"#,
    )
    .expect("JSON riordinato");
    assert!(compare_json_values(&reference, &reordered).is_ok());
    for (field, changed) in [
        ("date", json!("2026-01-03")),
        ("value", json!(71.0)),
        ("unit", json!("g")),
        ("source", json!("other/source.csv")),
    ] {
        let mut mutated = reference.clone();
        mutated[field] = changed;
        assert!(
            compare_json_values(&reference, &mutated).is_err(),
            "{field}"
        );
    }
}

#[test]
fn text_allows_crlf_but_not_content_changes() {
    assert!(compare_text("data\n70.0 kg\n", "data\r\n70.0 kg\r\n").is_ok());
    assert!(compare_text("data\n70.0 kg\n", "data\n71.0 kg\n").is_err());
}

#[test]
fn frontend_compares_json_text_and_file_paths() {
    let directory = TestDirectory::new();
    let reference = directory.path().join("reference");
    let candidate = directory.path().join("candidate");
    for root in [&reference, &candidate] {
        fs::create_dir_all(root.join("data")).expect("asset directory");
        fs::write(root.join("index.html"), "<main>synthetic</main>\n").expect("documento frontend");
        fs::write(root.join("data/summary.json"), r#"{"count":1,"unit":"kg"}"#)
            .expect("dati frontend");
    }
    assert!(compare_frontend_directories(&reference, &candidate).is_ok());
    fs::write(
        candidate.join("data/summary.json"),
        r#"{"count":2,"unit":"kg"}"#,
    )
    .expect("mutazione dati");
    assert!(compare_frontend_directories(&reference, &candidate).is_err());
    fs::write(
        candidate.join("data/summary.json"),
        r#"{"count":1,"unit":"kg"}"#,
    )
    .expect("ripristino dati");
    fs::write(candidate.join("extra.js"), "// unexpected\n").expect("file inatteso");
    assert!(compare_frontend_directories(&reference, &candidate).is_err());
}

#[test]
fn sqlite_comparison_checks_schema_constraints_rows_and_searches() {
    let reference = Connection::open_in_memory().expect("database di riferimento");
    let candidate = Connection::open_in_memory().expect("database candidato");
    for connection in [&reference, &candidate] {
        connection
            .execute_batch(
                "CREATE TABLE observations (id INTEGER PRIMARY KEY, value REAL NOT NULL CHECK(value > 0), unit TEXT NOT NULL);\
                 CREATE VIRTUAL TABLE searchable USING fts5(content);\
                 INSERT INTO observations(id, value, unit) VALUES (1, 70.0, 'kg');\
                 INSERT INTO searchable(rowid, content) VALUES (1, 'peso peso dato clinico'), (2, 'peso dato clinico parametro');",
            )
            .expect("schema e fixture SQLite");
    }
    assert!(compare_sqlite_connections(&reference, &candidate, &[]).is_err());
    let queries = vec!["peso".to_owned()];
    assert!(compare_sqlite_connections(&reference, &candidate, &queries).is_ok());
    let without_fts = Connection::open_in_memory().expect("database senza FTS");
    without_fts
        .execute("CREATE TABLE records (id INTEGER PRIMARY KEY)", [])
        .expect("tabella senza FTS");
    assert!(compare_sqlite_connections(&without_fts, &without_fts, &queries).is_err());

    candidate
        .execute("UPDATE observations SET value = 71.0 WHERE id = 1", [])
        .expect("mutazione del valore");
    assert!(compare_sqlite_connections(&reference, &candidate, &queries).is_err());
    candidate
        .execute("UPDATE observations SET value = 70.0 WHERE id = 1", [])
        .expect("ripristino del valore");

    let constrained_differently = Connection::open_in_memory().expect("database alternativo");
    constrained_differently
        .execute_batch(
            "CREATE TABLE observations (id INTEGER PRIMARY KEY, value REAL NOT NULL, unit TEXT NOT NULL);\
             CREATE VIRTUAL TABLE searchable USING fts5(content);\
             INSERT INTO observations(id, value, unit) VALUES (1, 70.0, 'kg');\
             INSERT INTO searchable(rowid, content) VALUES (1, 'peso peso dato clinico'), (2, 'peso dato clinico parametro');",
        )
        .expect("schema con vincolo mutato");
    assert!(
        compare_sqlite_connections(&reference, &constrained_differently, &queries).is_err(),
        "la rimozione del CHECK deve essere rilevata"
    );

    candidate
        .execute(
            "UPDATE searchable SET content = CASE rowid WHEN 1 THEN 'peso dato clinico parametro' ELSE 'peso peso dato clinico' END",
            [],
        )
        .expect("mutazione ricerca");
    assert!(
        !fts_search_results_equal(&reference, &candidate, "searchable", "peso")
            .expect("confronto risultati FTS"),
        "la modifica che inverte la rilevanza deve cambiare l'ordine dei risultati"
    );
    assert!(compare_sqlite_connections(&reference, &candidate, &queries).is_err());
}

#[test]
fn text_file_comparator_rejects_invalid_utf8() {
    let directory = TestDirectory::new();
    let reference = directory.path().join("reference.txt");
    let candidate = directory.path().join("candidate.txt");
    fs::write(&reference, b"synthetic\n").expect("testo valido");
    fs::write(&candidate, [0xff, 0xfe]).expect("testo non valido");
    assert!(compare_text_files(&reference, &candidate).is_err());
}
