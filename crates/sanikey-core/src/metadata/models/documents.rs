//! Provenienza e identità dei documenti originali.

use std::path::PathBuf;

/// Provenienza esplicita di un dato derivato.
#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    /// Sottosistema o file di origine.
    pub source: String,
    /// Riferimento stabile alla fonte.
    pub reference: String,
    /// Confidenza facoltativa del dato derivato.
    pub confidence: Option<f64>,
}

/// Rappresentazione di un documento originale senza modificarne i byte.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentRecord {
    /// Identificatore del documento basato sul digest SHA-256.
    pub document_id: String,
    /// Identificatore della persona proprietaria.
    pub patient_id: String,
    /// Percorso dell'originale.
    pub path: PathBuf,
    /// Titolo leggibile.
    pub title: String,
    /// Categoria derivata dalla struttura delle directory.
    pub category: String,
    /// Tipo del documento, per esempio `pdf`, `image` o `dicom_iso`.
    pub kind: String,
    /// Digest SHA-256 dei byte originali.
    pub sha256: String,
    /// Data ISO facoltativa.
    pub date: Option<String>,
    /// Nome facoltativo della serie.
    pub series: Option<String>,
    /// Tag curati o derivati.
    pub tags: Vec<String>,
    /// Origine `source` oppure `container`.
    pub origin: String,
    /// Identificatore del contenitore padre.
    pub container_id: Option<String>,
    /// Percorso del membro dentro il contenitore.
    pub internal_path: Option<String>,
}
