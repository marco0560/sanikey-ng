//! Serie e punti normalizzati delle osservazioni longitudinali.

/// Serie longitudinale di osservazioni.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationSeries {
    /// Identificatore stabile della serie.
    pub id: String,
    /// Nome leggibile.
    pub name: String,
    /// Tipo di valore, per esempio `numeric` o `blood_pressure`.
    pub value_type: String,
    /// Unità di misura.
    pub unit: Option<String>,
    /// Descrizione testuale.
    pub description: Option<String>,
    /// Segnala duplicati nella stessa giornata durante l'importazione.
    pub warn_duplicate_same_day: bool,
    /// Alias di ricerca deterministici.
    pub synonyms: Vec<String>,
    /// Identificatore della regola che ha generato la serie.
    pub parameter_rule_id: Option<String>,
    /// Versione dichiarata della regola.
    pub parameter_rule_version: Option<i64>,
    /// Digest della configurazione risolta della regola.
    pub parameter_rule_digest: Option<String>,
    /// Variante di unità senza conversione esplicita.
    pub unit_variant: Option<String>,
}

/// Punto normalizzato in una serie longitudinale.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservationPoint {
    /// Identificatore stabile del punto.
    pub id: String,
    /// Serie di appartenenza.
    pub series_id: String,
    /// Data ISO dell'osservazione.
    pub observation_date: String,
    /// Tipo della fonte.
    pub source_type: String,
    /// Riferimento leggibile alla fonte.
    pub source_reference: String,
    /// Valore numerico.
    pub numeric_value: Option<f64>,
    /// Valore testuale o categoriale.
    pub text_value: Option<String>,
    /// Pressione sistolica.
    pub systolic: Option<f64>,
    /// Pressione diastolica.
    pub diastolic: Option<f64>,
    /// Frequenza cardiaca.
    pub pulse: Option<f64>,
    /// Nota facoltativa.
    pub note: Option<String>,
    /// Origine di alto livello del punto.
    pub source_kind: String,
    /// Identità del documento sorgente.
    pub document_id: Option<String>,
    /// Link relativo al documento esportato.
    pub document_href: Option<String>,
    /// Titolo del documento sorgente.
    pub document_title: Option<String>,
    /// Categoria del documento sorgente.
    pub document_category: Option<String>,
    /// Digest del testo sorgente estratto.
    pub source_text_digest: Option<String>,
    /// Riga esatta della fonte.
    pub original_line: Option<String>,
    /// Numero di riga a partire da uno.
    pub line_number: Option<i64>,
    /// Numero di pagina se affidabile.
    pub page_number: Option<i64>,
    /// Offset iniziale inclusivo nel testo.
    pub character_start: Option<i64>,
    /// Offset finale esclusivo nel testo.
    pub character_end: Option<i64>,
    /// Etichetta riconosciuta dalla regola.
    pub matched_label: Option<String>,
    /// Token numerico grezzo.
    pub raw_value: Option<String>,
    /// Numero prima della conversione.
    pub parsed_value: Option<f64>,
    /// Unità grezza.
    pub raw_unit: Option<String>,
    /// Unità normalizzata esplicita.
    pub normalized_unit: Option<String>,
    /// Qualificatore di confronto.
    pub qualifier: Option<String>,
    /// Identificatore della regola applicata.
    pub rule_id: Option<String>,
    /// Versione della regola applicata.
    pub rule_version: Option<i64>,
    /// Digest della regola applicata.
    pub rule_digest: Option<String>,
    /// Codice deterministico di accettazione o conflitto.
    pub reason_code: Option<String>,
}
