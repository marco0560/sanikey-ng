//! Comparatori SQLite per gli artefatti di migrazione.

use super::sqlite_rows::{fts_search_results_equal, table_rows_equal};
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
struct SqliteSnapshot {
    schema: Vec<(String, String, String, Option<String>)>,
    tables: Vec<String>,
    fts_tables: Vec<String>,
}

pub(super) fn compare_sqlite_paths(
    reference: &Path,
    candidate: &Path,
    search_queries: &[String],
) -> Result<(), String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let reference_db = Connection::open_with_flags(reference, flags)
        .map_err(|error| format!("impossibile aprire SQLite di riferimento: {error}"))?;
    let candidate_db = Connection::open_with_flags(candidate, flags)
        .map_err(|error| format!("impossibile aprire SQLite candidato: {error}"))?;
    compare_sqlite_connections(&reference_db, &candidate_db, search_queries)
}

pub(super) fn compare_sqlite_connections(
    reference: &Connection,
    candidate: &Connection,
    search_queries: &[String],
) -> Result<(), String> {
    let reference_snapshot = sqlite_snapshot(reference, search_queries)?;
    let candidate_snapshot = sqlite_snapshot(candidate, search_queries)?;
    if reference_snapshot != candidate_snapshot {
        return Err("schema o oggetti SQLite differenti".to_owned());
    }

    for table in &reference_snapshot.tables {
        let is_fts = reference_snapshot.fts_tables.contains(table);
        if !table_rows_equal(reference, candidate, table, is_fts)? {
            return Err(format!("righe SQLite differenti nella tabella {table}"));
        }
    }

    for table in &reference_snapshot.fts_tables {
        for query in search_queries {
            if !fts_search_results_equal(reference, candidate, table, query)? {
                return Err(format!(
                    "risultati FTS differenti per la ricerca {query:?} nella tabella {table}"
                ));
            }
        }
    }
    Ok(())
}

fn sqlite_snapshot(connection: &Connection, queries: &[String]) -> Result<SqliteSnapshot, String> {
    let mut schema = {
        let mut statement = connection
            .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
            .map_err(|error| format!("impossibile leggere lo schema SQLite: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|error| format!("impossibile enumerare lo schema SQLite: {error}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("schema SQLite non leggibile: {error}"))?
    };

    let table_kinds = {
        let mut statement = connection
            .prepare("PRAGMA main.table_list")
            .map_err(|error| format!("impossibile leggere le tabelle SQLite: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(|error| format!("impossibile enumerare le tabelle SQLite: {error}"))?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(|error| format!("tabella SQLite non leggibile: {error}"))?
    };

    schema.retain(|(_, name, table, _)| {
        !table_kinds.get(name).is_some_and(|kind| kind == "shadow")
            && !table_kinds.get(table).is_some_and(|kind| kind == "shadow")
    });

    verify_sqlite_integrity(connection)?;
    verify_foreign_keys(connection)?;

    let tables: Vec<String> = table_kinds
        .iter()
        .filter(|(_, kind)| matches!(kind.as_str(), "table" | "virtual" | "view"))
        .map(|(name, _)| name.clone())
        .collect();

    let fts_tables: Vec<String> = schema
        .iter()
        .filter(|(kind, _, _, sql)| {
            kind == "table"
                && sql
                    .as_deref()
                    .is_some_and(|definition| definition.to_ascii_lowercase().contains("using fts"))
        })
        .map(|(_, name, _, _)| name.clone())
        .collect();
    if fts_tables.is_empty() && !queries.is_empty() {
        return Err(
            "sono state passate query FTS ma il database non contiene tabelle FTS".to_owned(),
        );
    }
    if !fts_tables.is_empty() && queries.is_empty() {
        return Err(
            "il database contiene FTS: passare almeno un --query rappresentativo".to_owned(),
        );
    }
    Ok(SqliteSnapshot {
        schema,
        tables,
        fts_tables,
    })
}

fn verify_sqlite_integrity(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA integrity_check")
        .map_err(|error| format!("controllo integrità SQLite fallito: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("controllo integrità SQLite fallito: {error}"))?;
    let first = rows
        .next()
        .map_err(|error| format!("controllo integrità SQLite fallito: {error}"))?
        .ok_or_else(|| "controllo integrità SQLite senza esito".to_owned())?
        .get::<_, String>(0)
        .map_err(|error| format!("esito integrità SQLite non leggibile: {error}"))?;
    if first != "ok" {
        return Err(format!("database SQLite non integro: {first}"));
    }
    Ok(())
}

fn verify_foreign_keys(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| format!("controllo delle foreign key SQLite fallito: {error}"))?;
    let mut rows = statement
        .query([])
        .map_err(|error| format!("controllo delle foreign key SQLite fallito: {error}"))?;
    if rows
        .next()
        .map_err(|error| format!("controllo delle foreign key SQLite fallito: {error}"))?
        .is_some()
    {
        return Err("database SQLite con violazioni delle foreign key".to_owned());
    }
    Ok(())
}
