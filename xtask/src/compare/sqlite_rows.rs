//! Comparatori righe SQLite per gli artefatti di migrazione.

use rusqlite::{Connection, types::ValueRef};

pub(super) fn table_rows_equal(
    reference: &Connection,
    candidate: &Connection,
    table: &str,
    include_rowid: bool,
) -> Result<bool, String> {
    let quoted_table = quote_identifier(table);
    let selected_columns = if include_rowid { "rowid, *" } else { "*" };
    let select = format!("SELECT {selected_columns} FROM {quoted_table}");
    let reference_shape = reference
        .prepare(&select)
        .map_err(|error| format!("impossibile leggere la tabella SQLite {table}: {error}"))?;
    let candidate_shape = candidate
        .prepare(&select)
        .map_err(|error| format!("impossibile leggere la tabella SQLite {table}: {error}"))?;
    let reference_columns: Vec<String> = reference_shape
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let candidate_columns: Vec<String> = candidate_shape
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if reference_columns != candidate_columns {
        return Ok(false);
    }

    let order = reference_columns
        .iter()
        .map(|column| {
            let quoted_column = quote_identifier(column);
            format!("typeof({quoted_column}), {quoted_column} COLLATE BINARY")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("{select} ORDER BY {order}");
    let mut reference_statement = reference
        .prepare(&sql)
        .map_err(|error| format!("impossibile ordinare la tabella SQLite {table}: {error}"))?;
    let mut candidate_statement = candidate
        .prepare(&sql)
        .map_err(|error| format!("impossibile ordinare la tabella SQLite {table}: {error}"))?;
    let column_count = reference_columns.len();
    let mut reference_rows = reference_statement
        .query([])
        .map_err(|error| format!("impossibile interrogare la tabella SQLite {table}: {error}"))?;
    let mut candidate_rows = candidate_statement
        .query([])
        .map_err(|error| format!("impossibile interrogare la tabella SQLite {table}: {error}"))?;
    row_sequences_equal(&mut reference_rows, &mut candidate_rows, column_count)
}

pub(super) fn fts_search_results_equal(
    reference: &Connection,
    candidate: &Connection,
    table: &str,
    query: &str,
) -> Result<bool, String> {
    let table_sql = quote_identifier(table);
    let sql =
        format!("SELECT rowid FROM {table_sql} WHERE {table_sql} MATCH ?1 ORDER BY rank, rowid");
    let mut reference_statement = reference
        .prepare(&sql)
        .map_err(|error| format!("ricerca FTS non disponibile in {table}: {error}"))?;
    let mut candidate_statement = candidate
        .prepare(&sql)
        .map_err(|error| format!("ricerca FTS non disponibile in {table}: {error}"))?;
    let mut reference_rows = reference_statement
        .query([query])
        .map_err(|error| format!("ricerca FTS non valida in {table}: {error}"))?;
    let mut candidate_rows = candidate_statement
        .query([query])
        .map_err(|error| format!("ricerca FTS non valida in {table}: {error}"))?;
    row_sequences_equal(&mut reference_rows, &mut candidate_rows, 1)
}

fn row_sequences_equal(
    reference: &mut rusqlite::Rows<'_>,
    candidate: &mut rusqlite::Rows<'_>,
    column_count: usize,
) -> Result<bool, String> {
    loop {
        let reference_row = reference
            .next()
            .map_err(|error| format!("riga SQLite di riferimento non leggibile: {error}"))?;
        let candidate_row = candidate
            .next()
            .map_err(|error| format!("riga SQLite candidata non leggibile: {error}"))?;
        match (reference_row, candidate_row) {
            (None, None) => return Ok(true),
            (Some(_), None) | (None, Some(_)) => return Ok(false),
            (Some(reference_row), Some(candidate_row)) => {
                for column in 0..column_count {
                    let reference_value = reference_row
                        .get_ref(column)
                        .map(canonical_sqlite_value)
                        .map_err(|error| {
                            format!("valore SQLite di riferimento non leggibile: {error}")
                        })?;
                    let candidate_value = candidate_row
                        .get_ref(column)
                        .map(canonical_sqlite_value)
                        .map_err(|error| {
                            format!("valore SQLite candidato non leggibile: {error}")
                        })?;
                    if reference_value != candidate_value {
                        return Ok(false);
                    }
                }
            }
        }
    }
}

fn canonical_sqlite_value(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "n".to_owned(),
        ValueRef::Integer(number) => format!("i:{number}"),
        ValueRef::Real(number) => format!("r:{:016x}", number.to_bits()),
        ValueRef::Text(text) => format!("t:{}", encode_hex(text)),
        ValueRef::Blob(bytes) => format!("b:{}", encode_hex(bytes)),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
