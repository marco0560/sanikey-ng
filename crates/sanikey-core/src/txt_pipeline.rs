//! Pipeline consultabile per il sottoinsieme sintetico dei file `.txt`.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::inventory::scan_documents;
use crate::metadata::{DocumentRecord, load_curated_metadata};
use crate::operation::{ErrorCode, OperationError};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DATABASE: AtomicUsize = AtomicUsize::new(0);

struct DatabaseStage {
    path: PathBuf,
    committed: bool,
}

impl Drop for DatabaseStage {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
            let mut journal = self.path.as_os_str().to_os_string();
            journal.push("-journal");
            let _ = fs::remove_file(PathBuf::from(journal));
        }
    }
}

/// Risultati e percorsi prodotti dalla pipeline TXT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TxtPipelineResult {
    /// Numero di documenti `.txt` distinti elaborati.
    pub documents: usize,
    /// Percorso del database SQLite generato.
    pub database: PathBuf,
    /// Percorso dei dati JSON esportati.
    pub data: PathBuf,
    /// Percorso della pagina statica consultabile.
    pub frontend: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
struct TxtDocument {
    record: DocumentRecord,
    relative_path: String,
    href: String,
    text: String,
    tags: Vec<String>,
}

/// Legge i documenti `.txt`, crea l'indice SQLite e genera export e pagina HTML.
///
/// Il perimetro comprende solo file con estensione `.txt`, in UTF-8 con
/// sostituzione dei byte non validi. Le righe CRLF e CR diventano LF, come
/// nella lettura testuale Python della baseline. Gli altri formati non vengono
/// estratti né mostrati.
///
/// # Errors
///
/// Restituisce un errore di configurazione se la scansione, la lettura, il
/// database o la scrittura di un artefatto non riesce.
pub fn build_txt_pipeline(person: &PersonConfig) -> Result<TxtPipelineResult, OperationError> {
    let inventory = scan_documents(person)?;
    let metadata = load_curated_metadata(&person.metadata_directory)?;
    let mut documents = Vec::new();
    for record in inventory.into_iter().filter(is_plain_text) {
        documents.push(read_document(person, record, &metadata.document_tags)?);
    }

    let database = person
        .local_build
        .join("database")
        .join("medical_archive.db");
    let data = person
        .local_build
        .join("web")
        .join("data")
        .join("documents.json");
    let frontend = person.local_build.join("web").join("index.html");

    build_database(&database, &documents, &person.id)?;
    export_documents(&data, person, &documents)?;
    build_frontend(&frontend, person, &documents)?;

    Ok(TxtPipelineResult {
        documents: documents.len(),
        database,
        data,
        frontend,
    })
}

fn is_plain_text(record: &DocumentRecord) -> bool {
    record.kind == "text"
        && record
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
}

fn read_document(
    person: &PersonConfig,
    record: DocumentRecord,
    configured_tags: &indexmap::IndexMap<String, Vec<String>>,
) -> Result<TxtDocument, OperationError> {
    let bytes = fs::read(&record.path).map_err(|cause| {
        filesystem_error("impossibile leggere il documento TXT", &record.path, cause)
    })?;
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let relative_path = relative_document_path(person, &record.path)?;
    let tags = configured_tags
        .iter()
        .find(|(key, _)| {
            let normalized = key.replace('\\', "/");
            relative_path.ends_with(&normalized)
                || Path::new(&relative_path)
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == normalized)
        })
        .map_or_else(|| record.tags.clone(), |(_, tags)| tags.clone());
    let href = format!("../documents/{}", encode_url_path(&relative_path));

    Ok(TxtDocument {
        record,
        relative_path,
        href,
        text,
        tags,
    })
}

fn build_database(
    path: &Path,
    documents: &[TxtDocument],
    patient_id: &str,
) -> Result<(), OperationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|cause| {
            filesystem_error("impossibile creare la directory SQLite", parent, cause)
        })?;
    }
    let mut stage = DatabaseStage {
        path: temporary_database_path(path, "building"),
        committed: false,
    };
    {
        let mut connection = Connection::open(&stage.path).map_err(|cause| {
            database_error(
                "impossibile creare il database SQLite temporaneo",
                path,
                cause,
            )
        })?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|cause| {
                database_error("impossibile attivare le chiavi esterne", path, cause)
            })?;
        let transaction = connection
            .transaction()
            .map_err(|cause| database_error("impossibile iniziare la transazione", path, cause))?;
        transaction
            .execute_batch(
                "CREATE TABLE documents (
                     id TEXT PRIMARY KEY,
                     patient_id TEXT NOT NULL,
                     path TEXT NOT NULL,
                     title TEXT NOT NULL,
                     category TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     sha256 TEXT NOT NULL,
                     document_date TEXT,
                     series TEXT,
                     origin TEXT NOT NULL,
                     container_id TEXT,
                     internal_path TEXT
                 );
                 CREATE TABLE document_text (
                     document_id TEXT PRIMARY KEY,
                     text TEXT NOT NULL,
                     FOREIGN KEY (document_id) REFERENCES documents(id)
                 );
                 CREATE VIRTUAL TABLE document_fts USING fts5(
                     title, category, tags, text
                 );",
            )
            .map_err(|cause| database_error("impossibile creare lo schema TXT", path, cause))?;

        for document in documents {
            transaction
                .execute(
                    "INSERT INTO documents (
                         id, patient_id, path, title, category, kind, sha256,
                         document_date, series, origin, container_id, internal_path
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        document.record.document_id,
                        patient_id,
                        document.relative_path,
                        document.record.title,
                        document.record.category,
                        document.record.kind,
                        document.record.sha256,
                        document.record.date,
                        document.record.series,
                        document.record.origin,
                        document.record.container_id,
                        document.record.internal_path,
                    ],
                )
                .map_err(|cause| {
                    database_error("impossibile inserire il documento TXT", path, cause)
                })?;
            let row_id = transaction.last_insert_rowid();
            if !document.text.is_empty() {
                transaction
                    .execute(
                        "INSERT INTO document_text(document_id, text) VALUES (?1, ?2)",
                        params![document.record.document_id, document.text],
                    )
                    .map_err(|cause| {
                        database_error("impossibile salvare il testo TXT", path, cause)
                    })?;
            }
            transaction
                .execute(
                    "INSERT INTO document_fts(rowid, title, category, tags, text)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        row_id,
                        document.record.title,
                        document.record.category,
                        document.tags.join(" "),
                        document.text,
                    ],
                )
                .map_err(|cause| {
                    database_error("impossibile indicizzare il testo TXT", path, cause)
                })?;
        }
        transaction.commit().map_err(|cause| {
            database_error("impossibile confermare il database SQLite", path, cause)
        })?;
    }
    replace_generated_database(&stage.path, path)?;
    stage.committed = true;
    Ok(())
}

fn temporary_database_path(target: &Path, purpose: &str) -> PathBuf {
    let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database.db");
    target.with_file_name(format!(
        ".{file_name}.{purpose}-{}-{sequence}",
        std::process::id()
    ))
}

fn replace_generated_database(source: &Path, target: &Path) -> Result<(), OperationError> {
    match fs::symlink_metadata(target) {
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => fs::rename(source, target)
            .map_err(|cause| {
                filesystem_error("impossibile installare il database generato", target, cause)
            }),
        Err(cause) => Err(filesystem_error(
            "impossibile verificare il database precedente",
            target,
            cause,
        )),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let backup = temporary_database_path(target, "previous");
            fs::rename(target, &backup).map_err(|cause| {
                filesystem_error(
                    "impossibile conservare il database precedente",
                    target,
                    cause,
                )
            })?;
            if let Err(cause) = fs::rename(source, target) {
                let detail = match fs::rename(&backup, target) {
                    Ok(()) => "database precedente ripristinato".to_owned(),
                    Err(error) => format!("ripristino del database precedente fallito: {error}"),
                };
                return Err(filesystem_error(
                    &format!("impossibile installare il database generato; {detail}"),
                    target,
                    cause,
                ));
            }
            let _ = fs::remove_file(backup);
            Ok(())
        }
        Ok(_) => Err(OperationError::new(
            ErrorCode::Configuration,
            format!(
                "il percorso del database non è un file regolare: {}",
                target.display()
            ),
        )),
    }
}

fn export_documents(
    path: &Path,
    person: &PersonConfig,
    documents: &[TxtDocument],
) -> Result<(), OperationError> {
    let rows: Vec<Value> = documents
        .iter()
        .map(|document| {
            json!({
                "id": document.record.document_id,
                "patient_id": person.id,
                "title": document.record.title,
                "date": document.record.date,
                "category": document.record.category,
                "kind": document.record.kind,
                "path": document.relative_path,
                "href": document.href,
                "sha256": document.record.sha256,
                "tags": document.tags,
                "text": document.text,
            })
        })
        .collect();
    write_file(
        path,
        serde_json::to_string_pretty(&json!({
            "patient_id": person.id,
            "display_name": person.display_name,
            "documents": rows,
        }))
        .map_err(|cause| {
            OperationError::new(
                ErrorCode::Configuration,
                format!("impossibile serializzare i dati TXT: {cause}"),
            )
        })? + "\n",
    )
}

fn build_frontend(
    path: &Path,
    person: &PersonConfig,
    documents: &[TxtDocument],
) -> Result<(), OperationError> {
    let document_root = path
        .parent()
        .expect("la pagina frontend ha una directory")
        .parent()
        .expect("la directory frontend ha un genitore")
        .join("documents");
    fs::create_dir_all(&document_root).map_err(|cause| {
        filesystem_error(
            "impossibile creare la directory dei documenti",
            &document_root,
            cause,
        )
    })?;
    let mut html = format!(
        "<!doctype html>\n<html lang=\"it\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>SaniKey - {}</title>\n</head>\n<body>\n<main>\n<h1>Archivio TXT — {}</h1>\n<ul>\n",
        escape_html(&person.display_name),
        escape_html(&person.display_name)
    );
    for document in documents {
        let copy_path = path
            .parent()
            .expect("la pagina frontend ha una directory")
            .parent()
            .expect("la directory frontend ha un genitore")
            .join("documents")
            .join(&document.relative_path);
        if let Some(parent) = copy_path.parent() {
            fs::create_dir_all(parent).map_err(|cause| {
                filesystem_error(
                    "impossibile creare la directory dei documenti",
                    parent,
                    cause,
                )
            })?;
        }
        fs::copy(&document.record.path, &copy_path).map_err(|cause| {
            filesystem_error("impossibile copiare il documento TXT", &copy_path, cause)
        })?;
        let date = document
            .record
            .date
            .as_deref()
            .unwrap_or("Data non indicata");
        html.push_str(&format!(
            "<li><article><h2><a href=\"{}\">{}</a></h2><p>{} · {}</p><p>Categoria: {}</p><pre>{}</pre></article></li>\n",
            escape_html(&document.href),
            escape_html(&document.record.title),
            escape_html(date),
            escape_html(&document.record.kind),
            escape_html(&document.record.category),
            escape_html(&document.text),
        ));
    }
    html.push_str("</ul>\n</main>\n</body>\n</html>\n");
    write_file(path, html)
}

fn relative_document_path(person: &PersonConfig, path: &Path) -> Result<String, OperationError> {
    let relative = path
        .strip_prefix(&person.source_documents)
        .map_err(|cause| {
            OperationError::new(
                ErrorCode::Configuration,
                format!(
                    "documento fuori dalla directory sorgente {}: {cause}",
                    path.display()
                ),
            )
        })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => components.push(part.to_string_lossy().into_owned()),
            _ => {
                return Err(OperationError::new(
                    ErrorCode::Configuration,
                    format!("percorso documento relativo non valido: {}", path.display()),
                ));
            }
        }
    }
    Ok(components.join("/"))
}

fn encode_url_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn write_file(path: &Path, contents: String) -> Result<(), OperationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|cause| {
            filesystem_error("impossibile creare la directory di output", parent, cause)
        })?;
    }
    fs::write(path, contents)
        .map_err(|cause| filesystem_error("impossibile scrivere l'artefatto", path, cause))
}

fn filesystem_error(context: &str, path: &Path, cause: std::io::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{context} {}: {cause}", path.display()),
    )
}

fn database_error(context: &str, path: &Path, cause: rusqlite::Error) -> OperationError {
    OperationError::new(
        ErrorCode::Configuration,
        format!("{context} {}: {cause}", path.display()),
    )
}
