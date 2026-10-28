//! Importazione e copia verificata dei profili account esistenti.

use super::AccountsConfig;
use super::parse::{absolute_non_strict, load_accounts, path_base};
use crate::operation::{ErrorCode, OperationError};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Profilo account importato senza modificare il file sorgente.
#[derive(Clone, Debug)]
pub struct AccountsProfile {
    accounts: AccountsConfig,
    base_directory: PathBuf,
    source_path: PathBuf,
    source_contents: String,
}

/// Importa un profilo account TOML storico e ne conserva i byte originali.
impl AccountsProfile {
    /// Carica e valida un profilo dal percorso indicato.
    ///
    /// I percorsi relativi nel profilo vengono risolti rispetto alla base
    /// storica della configurazione. Il percorso sorgente viene conservato in
    /// forma assoluta, per consentire operazioni successive dopo un cambio di
    /// directory corrente.
    ///
    /// # Errors
    ///
    /// Restituisce `ErrorCode::Configuration` se il profilo non può essere
    /// letto, non è valido, il percorso non è assoluto o la sua base non può
    /// essere determinata.
    pub fn import(path: impl AsRef<Path>) -> Result<Self, OperationError> {
        let requested_path = path.as_ref();
        if !requested_path.is_absolute() {
            return Err(configuration_error(
                "il percorso del profilo da importare deve essere assoluto".to_owned(),
            ));
        }
        let source_path = fs::canonicalize(requested_path).map_err(|cause| {
            configuration_error(format!(
                "impossibile individuare il profilo account {}: {cause}",
                requested_path.display()
            ))
        })?;
        if !source_path.is_file() {
            return Err(configuration_error(format!(
                "il profilo account non è un file: {}",
                source_path.display()
            )));
        }
        let source_contents = fs::read_to_string(&source_path).map_err(|cause| {
            configuration_error(format!(
                "impossibile leggere il profilo account {}: {cause}",
                source_path.display()
            ))
        })?;
        let accounts = load_accounts(&source_path)?;
        let base_directory = path_base(&source_path)?;

        Ok(Self {
            accounts,
            base_directory,
            source_path,
            source_contents,
        })
    }

    /// Restituisce la configurazione validata importata.
    #[must_use]
    pub const fn accounts(&self) -> &AccountsConfig {
        &self.accounts
    }

    /// Restituisce la directory che ancora i percorsi relativi del profilo.
    #[must_use]
    pub fn base_directory(&self) -> &Path {
        &self.base_directory
    }

    /// Restituisce il percorso assoluto del file originale.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Esporta una copia verificata senza sovrascrivere file esistenti.
    ///
    /// La destinazione deve essere assoluta e condividere la stessa base del
    /// file importato. La copia conserva il contenuto TOML originale, inclusi
    /// alias storici e percorsi relativi. Dopo la scrittura il profilo viene
    /// ricaricato e confrontato con quello importato; in caso di errore la
    /// nuova copia viene rimossa.
    ///
    /// # Errors
    ///
    /// Restituisce `ErrorCode::Configuration` se la destinazione non è
    /// assoluta, cambia la base dei percorsi, esiste già o la copia non supera
    /// la riapertura e il confronto.
    pub fn export_copy(&self, destination: impl AsRef<Path>) -> Result<PathBuf, OperationError> {
        let requested_destination = destination.as_ref();
        if !requested_destination.is_absolute() {
            return Err(configuration_error(
                "il percorso di esportazione del profilo deve essere assoluto".to_owned(),
            ));
        }

        let destination = absolute_non_strict(requested_destination)?;
        if destination == self.source_path {
            return Err(configuration_error(
                "il profilo esportato deve avere un percorso distinto dall'originale".to_owned(),
            ));
        }
        let destination_base = path_base(&destination)?;
        if destination_base != self.base_directory {
            return Err(configuration_error(format!(
                "la destinazione deve mantenere la base del profilo: {}",
                self.base_directory.display()
            )));
        }

        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|cause| {
                configuration_error(format!(
                    "impossibile creare il profilo esportato {}: {cause}",
                    destination.display()
                ))
            })?;
        let write_result = output
            .write_all(self.source_contents.as_bytes())
            .and_then(|()| output.sync_all());
        drop(output);

        let result = write_result
            .map_err(|cause| {
                configuration_error(format!(
                    "impossibile scrivere il profilo esportato {}: {cause}",
                    destination.display()
                ))
            })
            .and_then(|()| verify_export(&self.accounts, &self.source_contents, &destination));

        if result.is_err() {
            let _ = fs::remove_file(&destination);
        }
        result.map(|()| destination)
    }
}

fn verify_export(
    expected: &AccountsConfig,
    expected_contents: &str,
    destination: &Path,
) -> Result<(), OperationError> {
    let actual_contents = fs::read_to_string(destination).map_err(|cause| {
        configuration_error(format!(
            "impossibile verificare il profilo esportato {}: {cause}",
            destination.display()
        ))
    })?;
    if actual_contents != expected_contents {
        return Err(configuration_error(format!(
            "il contenuto del profilo esportato differisce dall'originale: {}",
            destination.display()
        )));
    }
    let mut actual = load_accounts(destination)?;
    actual.path.clone_from(&expected.path);
    if actual != *expected {
        return Err(configuration_error(format!(
            "il profilo esportato non corrisponde all'originale: {}",
            destination.display()
        )));
    }
    Ok(())
}

fn configuration_error(message: String) -> OperationError {
    OperationError::new(ErrorCode::Configuration, message)
}

#[cfg(test)]
mod tests;
