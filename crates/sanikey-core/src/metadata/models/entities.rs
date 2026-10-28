//! Problemi, farmaci, terapie e riferimenti clinici.

/// Problema clinico curato con provenienza facoltativa.
#[derive(Clone, Debug, PartialEq)]
pub struct ClinicalProblem {
    /// Identificatore stabile.
    pub id: String,
    /// Titolo del problema.
    pub title: String,
    /// Stato del problema.
    pub status: String,
    /// Riferimenti alle evidenze.
    pub provenance: Vec<super::Provenance>,
}

/// Identità di un farmaco curato.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Medication {
    /// Identificatore stabile.
    pub id: String,
    /// Nome del farmaco.
    pub name: String,
    /// Principio attivo, se noto.
    pub active_ingredient: Option<String>,
    /// Forma farmaceutica.
    pub form: Option<String>,
    /// Quantità di principio attivo per unità.
    pub strength_per_unit: Option<String>,
}

/// Riferimento confermato a un documento farmaceutico AIFA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MedicationLeaflet {
    /// Identificatore del farmaco curato.
    pub medication_id: String,
    /// Identificatore dell'organizzazione AIFA.
    pub codice_sis: String,
    /// Identificatore AIC a sei cifre.
    pub aic6: String,
    /// Data ISO di scaricamento.
    pub downloaded_at: Option<String>,
    /// Impronta dei campi usati nella ricerca.
    pub source_fingerprint: Option<String>,
}

/// Esclusione curata di un farmaco dalla consultazione AIFA.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MedicationLeafletExclusion {
    /// Identificatore del farmaco curato.
    pub medication_id: String,
    /// Motivo dell'esclusione, normalmente `non_aifa`.
    pub reason: String,
}

/// Intervallo terapeutico associato a un farmaco.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TherapyEpisode {
    /// Identificatore stabile, anche generato dal loader.
    pub id: String,
    /// Identificatore del farmaco collegato.
    pub medication_id: String,
    /// Data ISO di inizio.
    pub start_date: Option<String>,
    /// Data ISO di fine.
    pub end_date: Option<String>,
    /// Posologia descrittiva.
    pub dosage: Option<String>,
    /// Ruolo o indicazione terapeutica.
    pub role: Option<String>,
    /// Orari o fasce di assunzione.
    pub schedule: Vec<String>,
    /// Istruzioni testuali.
    pub instructions: Option<String>,
}

/// Procedura clinica curata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Procedure {
    /// Identificatore stabile.
    pub id: String,
    /// Titolo della procedura.
    pub title: String,
    /// Data ISO.
    pub date: Option<String>,
    /// Stato della procedura.
    pub status: String,
}

/// Valore osservato in un contesto clinico.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    /// Identificatore stabile.
    pub id: String,
    /// Tipo di osservazione.
    pub kind: String,
    /// Valore leggibile con la relativa unità.
    pub value: String,
    /// Data ISO dell'osservazione.
    pub date: Option<String>,
}

/// Evento o intervallo manuale della cronologia clinica.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimelineEvent {
    /// Identificatore stabile.
    pub id: String,
    /// Titolo dell'evento.
    pub title: String,
    /// Data ISO iniziale.
    pub start_date: Option<String>,
    /// Data ISO finale.
    pub end_date: Option<String>,
    /// Origine dell'evento.
    pub source: String,
    /// Identificatori di documenti o entità collegate.
    pub links: Vec<String>,
}
