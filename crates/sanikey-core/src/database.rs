//! Archivio SQLite per documenti e metadati clinici curati.

#[cfg(test)]
mod tests;

use crate::config::PersonConfig;
use crate::dicom::DicomStudy;
use crate::metadata::{CuratedMetadata, DocumentRecord};
use crate::operation::{ErrorCode, OperationError};
use crate::pdf::ExtractedText;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static BUILD_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// Riepilogo della generazione dell'archivio SQLite di una persona.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseBuildResult {
    /// Percorso del database creato.
    pub path: PathBuf,
    /// Numero di documenti inseriti.
    pub documents: usize,
    /// Numero di eventi della cronologia inseriti.
    pub timeline_events: usize,
    /// Numero di studi DICOM inseriti.
    pub dicom_studies: usize,
}

/// Restituisce il percorso dell'archivio generato per una persona.
#[must_use]
pub fn database_path(person: &PersonConfig) -> PathBuf {
    person
        .local_build
        .join("database")
        .join("medical_archive.db")
}

/// Costruisce un database nuovo e completo, sostituendo l'artefatto solo a commit riuscito.
pub fn build_database(
    person: &PersonConfig,
    documents: &[DocumentRecord],
    metadata: &CuratedMetadata,
    dicom_studies: &[DicomStudy],
    extracted_text: &[ExtractedText],
) -> Result<DatabaseBuildResult, OperationError> {
    let path = database_path(person);
    let parent = path
        .parent()
        .expect("il database ha una directory genitore");
    fs::create_dir_all(parent).map_err(|error| {
        database_error(format!(
            "creazione directory SQLite {}: {error}",
            parent.display()
        ))
    })?;
    let mut stage = DatabaseStage::new(&path);
    {
        let mut connection = Connection::open(&stage.path).map_err(|error| {
            database_error(format!("apertura database SQLite temporaneo: {error}"))
        })?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| database_error(format!("attivazione foreign key SQLite: {error}")))?;
        let transaction = connection
            .transaction()
            .map_err(|error| database_error(format!("inizio transazione SQLite: {error}")))?;
        transaction
            .execute_batch(SCHEMA)
            .map_err(|error| database_error(format!("creazione schema SQLite: {error}")))?;
        insert_documents(&transaction, documents, extracted_text)?;
        insert_metadata(&transaction, metadata)?;
        insert_dicom(&transaction, dicom_studies)?;
        transaction
            .commit()
            .map_err(|error| database_error(format!("commit transazione SQLite: {error}")))?;
    }
    fs::rename(&stage.path, &path).map_err(|error| {
        database_error(format!(
            "finalizzazione database {}: {error}",
            path.display()
        ))
    })?;
    stage.committed = true;
    Ok(DatabaseBuildResult {
        path,
        documents: documents.len(),
        timeline_events: metadata.timeline_events.len(),
        dicom_studies: dicom_studies.len(),
    })
}

struct DatabaseStage {
    path: PathBuf,
    committed: bool,
}

impl DatabaseStage {
    fn new(database_path: &Path) -> Self {
        let sequence = BUILD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let file_name = database_path
            .file_name()
            .expect("il database ha un nome file")
            .to_string_lossy();
        let path = database_path.with_file_name(format!(
            ".{file_name}.building-{}-{sequence}",
            std::process::id()
        ));
        Self {
            path,
            committed: false,
        }
    }
}

impl Drop for DatabaseStage {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn insert_documents(
    connection: &Connection,
    documents: &[DocumentRecord],
    extracted_text: &[ExtractedText],
) -> Result<(), OperationError> {
    let text_by_document_id: HashMap<_, _> = extracted_text
        .iter()
        .filter(|item| !item.text.is_empty())
        .map(|item| (item.document_id.as_str(), item.text.as_str()))
        .collect();
    for document in documents {
        connection
            .execute(
                "INSERT INTO documents (
                    id, patient_id, path, title, category, kind, sha256,
                    document_date, series, origin, container_id, internal_path
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    document.document_id,
                    document.patient_id,
                    document.path.to_string_lossy().as_ref(),
                    document.title,
                    document.category,
                    document.kind,
                    document.sha256,
                    document.date,
                    document.series,
                    document.origin,
                    document.container_id,
                    document.internal_path,
                ],
            )
            .map_err(|error| database_error(format!("inserimento documento SQLite: {error}")))?;
        let row_id = connection.last_insert_rowid();
        let text = text_by_document_id
            .get(document.document_id.as_str())
            .copied()
            .unwrap_or_default();
        if !text.is_empty() {
            connection
                .execute(
                    "INSERT INTO document_text(document_id, text) VALUES (?1, ?2)",
                    params![document.document_id, text],
                )
                .map_err(|error| {
                    database_error(format!("inserimento testo estratto SQLite: {error}"))
                })?;
        }
        connection
            .execute(
                "INSERT INTO document_fts(rowid, title, category, tags, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    row_id,
                    document.title,
                    document.category,
                    document.tags.join(" "),
                    text,
                ],
            )
            .map_err(|error| database_error(format!("indicizzazione documento FTS5: {error}")))?;
    }
    Ok(())
}

fn insert_metadata(
    connection: &Connection,
    metadata: &CuratedMetadata,
) -> Result<(), OperationError> {
    for item in &metadata.problems {
        connection
            .execute(
                "INSERT INTO problems(id, title, status) VALUES (?1, ?2, ?3)",
                params![item.id, item.title, item.status],
            )
            .map_err(|error| database_error(format!("inserimento problema: {error}")))?;
    }
    for item in &metadata.medications {
        connection
            .execute(
                "INSERT INTO medications(id, name, active_ingredient, form, strength_per_unit)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    item.id,
                    item.name,
                    item.active_ingredient,
                    item.form,
                    item.strength_per_unit
                ],
            )
            .map_err(|error| database_error(format!("inserimento farmaco: {error}")))?;
    }
    for item in &metadata.therapies {
        connection.execute(
            "INSERT INTO therapies(id, medication_id, start_date, end_date, dosage, role, schedule, instructions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![item.id, item.medication_id, item.start_date, item.end_date, item.dosage,
                item.role, item.schedule.join(","), item.instructions],
        ).map_err(|error| database_error(format!("inserimento terapia: {error}")))?;
    }
    for item in &metadata.procedures {
        connection
            .execute(
                "INSERT INTO procedures(id, title, procedure_date, status) VALUES (?1, ?2, ?3, ?4)",
                params![item.id, item.title, item.date, item.status],
            )
            .map_err(|error| database_error(format!("inserimento procedura: {error}")))?;
    }
    for item in &metadata.observations {
        connection.execute(
            "INSERT INTO observations(id, kind, value, observation_date) VALUES (?1, ?2, ?3, ?4)",
            params![item.id, item.kind, item.value, item.date],
        ).map_err(|error| database_error(format!("inserimento osservazione: {error}")))?;
    }
    for item in &metadata.observation_series {
        connection.execute(
            "INSERT INTO observation_series(id, name, value_type, unit, description,
             warn_duplicate_same_day, synonyms, parameter_rule_id, parameter_rule_version,
             parameter_rule_digest, unit_variant) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![item.id, item.name, item.value_type, item.unit, item.description,
                if item.warn_duplicate_same_day { 1_i64 } else { 0_i64 }, item.synonyms.join(","),
                item.parameter_rule_id, item.parameter_rule_version,
                item.parameter_rule_digest, item.unit_variant],
        ).map_err(|error| database_error(format!("inserimento serie osservazioni: {error}")))?;
    }
    for item in &metadata.observation_points {
        connection
            .execute(
                "INSERT INTO observation_points(
                id, series_id, observation_date, source_type, source_reference,
                numeric_value, text_value, systolic, diastolic, pulse, note,
                source_kind, document_id, document_href, document_title,
                document_category, source_text_digest, original_line, line_number,
                page_number, character_start, character_end, matched_label, raw_value,
                parsed_value, raw_unit, normalized_unit, qualifier, rule_id,
                rule_version, rule_digest, reason_code
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32
             )",
                params![
                    item.id,
                    item.series_id,
                    item.observation_date,
                    item.source_type,
                    item.source_reference,
                    item.numeric_value,
                    item.text_value,
                    item.systolic,
                    item.diastolic,
                    item.pulse,
                    item.note,
                    item.source_kind,
                    item.document_id,
                    item.document_href,
                    item.document_title,
                    item.document_category,
                    item.source_text_digest,
                    item.original_line,
                    item.line_number,
                    item.page_number,
                    item.character_start,
                    item.character_end,
                    item.matched_label,
                    item.raw_value,
                    item.parsed_value,
                    item.raw_unit,
                    item.normalized_unit,
                    item.qualifier,
                    item.rule_id,
                    item.rule_version,
                    item.rule_digest,
                    item.reason_code
                ],
            )
            .map_err(|error| database_error(format!("inserimento punto osservazioni: {error}")))?;
    }
    for item in &metadata.timeline_events {
        connection
            .execute(
                "INSERT INTO timeline_events(id, title, start_date, end_date, source, links)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item.id,
                    item.title,
                    item.start_date,
                    item.end_date,
                    item.source,
                    item.links.join(",")
                ],
            )
            .map_err(|error| database_error(format!("inserimento evento cronologia: {error}")))?;
    }
    Ok(())
}

fn insert_dicom(connection: &Connection, studies: &[DicomStudy]) -> Result<(), OperationError> {
    for study in studies {
        let viewer_count = i64::try_from(study.viewer_paths.len()).map_err(|error| {
            database_error(format!("conteggio viewer DICOM fuori intervallo: {error}"))
        })?;
        let warning_count = i64::try_from(study.warnings.len()).map_err(|error| {
            database_error(format!("conteggio avvisi DICOM fuori intervallo: {error}"))
        })?;
        let instance_count = i64::try_from(study.instance_count).map_err(|error| {
            database_error(format!("conteggio istanze DICOM fuori intervallo: {error}"))
        })?;
        connection.execute(
            "INSERT INTO dicom_studies(id, patient_id, support_path, support_kind, extracted_path,
             viewer_count, warning_count, study_instance_uid, study_date, study_description, instance_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![study.study_id, study.patient_id, study.support_path.to_string_lossy().as_ref(),
                study.support_kind, study.extracted_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
                viewer_count, warning_count, study.study_instance_uid, study.study_date,
                study.study_description, instance_count],
        ).map_err(|error| database_error(format!("inserimento studio DICOM: {error}")))?;
    }
    Ok(())
}

fn database_error(message: String) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

const SCHEMA: &str = r#"
        CREATE TABLE documents (
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

        CREATE VIRTUAL TABLE document_fts USING fts5(
            title,
            category,
            tags,
            text
        );

        CREATE TABLE document_text (
            document_id TEXT PRIMARY KEY,
            text TEXT NOT NULL,
            FOREIGN KEY (document_id) REFERENCES documents(id)
        );

        CREATE TABLE problems (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            status TEXT NOT NULL
        );

        CREATE TABLE medications (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            active_ingredient TEXT,
            form TEXT,
            strength_per_unit TEXT
        );

        CREATE TABLE therapies (
            id TEXT PRIMARY KEY,
            medication_id TEXT NOT NULL,
            start_date TEXT,
            end_date TEXT,
            dosage TEXT,
            role TEXT,
            schedule TEXT NOT NULL,
            instructions TEXT,
            FOREIGN KEY (medication_id) REFERENCES medications(id)
        );

        CREATE TABLE procedures (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            procedure_date TEXT,
            status TEXT NOT NULL
        );

        CREATE TABLE observations (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            value TEXT NOT NULL,
            observation_date TEXT
        );

        CREATE TABLE observation_series (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            value_type TEXT NOT NULL,
            unit TEXT,
            description TEXT,
            warn_duplicate_same_day INTEGER NOT NULL,
            synonyms TEXT NOT NULL,
            parameter_rule_id TEXT,
            parameter_rule_version INTEGER,
            parameter_rule_digest TEXT,
            unit_variant TEXT
        );

        CREATE TABLE observation_points (
            id TEXT PRIMARY KEY,
            series_id TEXT NOT NULL,
            observation_date TEXT NOT NULL,
            source_type TEXT NOT NULL,
            source_reference TEXT NOT NULL,
            numeric_value REAL,
            text_value TEXT,
            systolic REAL,
            diastolic REAL,
            pulse REAL,
            note TEXT,
            source_kind TEXT NOT NULL,
            document_id TEXT,
            document_href TEXT,
            document_title TEXT,
            document_category TEXT,
            source_text_digest TEXT,
            original_line TEXT,
            line_number INTEGER,
            page_number INTEGER,
            character_start INTEGER,
            character_end INTEGER,
            matched_label TEXT,
            raw_value TEXT,
            parsed_value REAL,
            raw_unit TEXT,
            normalized_unit TEXT,
            qualifier TEXT,
            rule_id TEXT,
            rule_version INTEGER,
            rule_digest TEXT,
            reason_code TEXT,
            FOREIGN KEY (series_id) REFERENCES observation_series(id)
        );

        CREATE TABLE timeline_events (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            start_date TEXT,
            end_date TEXT,
            source TEXT NOT NULL,
            links TEXT NOT NULL
        );

        CREATE TABLE dicom_studies (
            id TEXT PRIMARY KEY,
            patient_id TEXT NOT NULL,
            support_path TEXT NOT NULL,
            support_kind TEXT NOT NULL,
            extracted_path TEXT,
            viewer_count INTEGER NOT NULL,
            warning_count INTEGER NOT NULL,
            study_instance_uid TEXT,
            study_date TEXT,
            study_description TEXT,
            instance_count INTEGER NOT NULL
        );
"#;
