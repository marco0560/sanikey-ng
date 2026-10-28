//! Configurazione globale e per persona, con validazione TOML.

mod parse;
mod profile;
mod sections;
mod validation;

#[cfg(test)]
mod tests;

use indexmap::IndexMap;
pub use parse::{default_accounts_path, load_accounts, parse_accounts_data};
pub use profile::AccountsProfile;
use std::path::PathBuf;

/// Configurazione dell'interfaccia statica associata a una persona.
#[derive(Clone, Debug, PartialEq)]
pub struct UiConfig {
    /// Colore esadecimale `#rrggbb` normalizzato in minuscolo.
    pub accent_color: String,
    /// Densità del layout: `compact` o `comfortable`.
    pub density: String,
    /// Scheda iniziale della consultazione.
    pub default_tab: String,
    /// Ordinamento della cronologia: `asc` o `desc`.
    pub timeline_order: String,
    /// Modalità dei collegamenti ai documenti.
    pub document_link_mode: String,
    /// Sottotitolo mostrato sotto il nome.
    pub subtitle: String,
    /// Immagine di sfondo facoltativa, risolta rispetto alla configurazione.
    pub background_image: Option<PathBuf>,
    /// Opacità dello sfondo compresa tra zero e uno.
    pub background_opacity: f64,
}

/// Fornisce i default storici dell'interfaccia di consultazione.
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            accent_color: "#2563eb".to_owned(),
            density: "comfortable".to_owned(),
            default_tab: "documents".to_owned(),
            timeline_order: "desc".to_owned(),
            document_link_mode: "usb-relative".to_owned(),
            subtitle: "Archivio sanitario personale".to_owned(),
            background_image: None,
            background_opacity: 0.1,
        }
    }
}

/// Espansioni configurabili per i termini e i mesi della ricerca.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchDictionary {
    /// Espansioni indicizzate per termine canonico.
    pub terms: IndexMap<String, Vec<String>>,
    /// Espansioni indicizzate per nome o numero del mese.
    pub months: IndexMap<String, Vec<String>>,
}

/// Configurazione della ricerca avanzata.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchConfig {
    /// Percorso facoltativo del dizionario TOML.
    pub dictionary: Option<PathBuf>,
    /// Contenuto validato del dizionario.
    pub dictionary_data: SearchDictionary,
    /// Soglia di avviso dell'indice avanzato in megabyte.
    pub advanced_index_warning_mb: i64,
}

/// Fornisce i default storici della ricerca avanzata.
impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            dictionary: None,
            dictionary_data: SearchDictionary::default(),
            advanced_index_warning_mb: 25,
        }
    }
}

/// Filtri di ingestione ereditati e poi estesi dalla configurazione persona.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestionConfig {
    /// Pattern esclusi prima di hash, staging ed estrazione.
    pub exclude_patterns: Vec<String>,
    /// Pattern che recuperano percorsi esclusi.
    pub include_patterns: Vec<String>,
}

/// Impostazioni globali di distribuzione USB.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsbConfig {
    /// UUID filesystem atteso, se configurato globalmente.
    pub usb_uuid: Option<String>,
    /// Richiede un volume exFAT.
    pub require_exfat: bool,
    /// Margine minimo di spazio libero in megabyte.
    pub min_free_space_mb: i64,
    /// Strategia di copia: `python` o `rsync-preferred`.
    pub copy_strategy: String,
}

/// Fornisce i default storici della distribuzione USB.
impl Default for UsbConfig {
    fn default() -> Self {
        Self {
            usb_uuid: None,
            require_exfat: false,
            min_free_space_mb: 256,
            copy_strategy: "rsync-preferred".to_owned(),
        }
    }
}

/// Configurazione validata di una persona.
#[derive(Clone, Debug, PartialEq)]
pub struct PersonConfig {
    /// Identificatore tecnico stabile.
    pub id: String,
    /// Nome mostrato negli artefatti generati.
    pub display_name: String,
    /// Directory dei documenti originali.
    pub source_documents: PathBuf,
    /// Directory dei metadati curati.
    pub metadata_directory: PathBuf,
    /// Directory degli artefatti generati.
    pub local_build: PathBuf,
    /// UUID filesystem atteso per la distribuzione.
    pub usb_uuid: String,
    /// Indica se la persona entra nelle build.
    pub enabled: bool,
    /// Configurazione UI globale con override persona.
    pub ui: UiConfig,
    /// Configurazione ricerca globale con override persona.
    pub search: SearchConfig,
    /// Configurazione filtri globale estesa dai filtri persona.
    pub ingestion: IngestionConfig,
}

/// Configurazione account caricata e validata.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountsConfig {
    /// Versione dello schema degli account.
    pub config_version: i64,
    /// Persone nell'ordine del file.
    pub people: Vec<PersonConfig>,
    /// Percorso passato al parser o risolto dal caricatore.
    pub path: PathBuf,
    /// Default UI globali.
    pub ui: UiConfig,
    /// Default ricerca globali.
    pub search: SearchConfig,
    /// Filtri di ingestione globali.
    pub ingestion: IngestionConfig,
    /// Impostazioni USB globali.
    pub usb: UsbConfig,
}

/// Espone le viste derivate dalla configurazione account validata.
impl AccountsConfig {
    /// Restituisce le persone abilitate conservandone l'ordine.
    pub fn enabled_people(&self) -> impl Iterator<Item = &PersonConfig> {
        self.people.iter().filter(|person| person.enabled)
    }
}
