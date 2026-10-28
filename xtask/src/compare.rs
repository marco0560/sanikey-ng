//! Confronti deterministici degli artefatti della migrazione.

use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const USAGE: &str = "Uso: cargo xtask compare <originals|sqlite|json|text|frontend> <riferimento> <candidato> [--query <ricerca>]...";

/// Esegue il comparatore richiesto e restituisce un errore per ogni differenza significativa.
pub(super) fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() < 3 {
        return Err(USAGE.to_owned());
    }

    let kind = args[0].as_str();
    if kind != "sqlite" && args.len() != 3 {
        return Err(format!(
            "argomenti aggiuntivi non ammessi per {kind}\n{USAGE}"
        ));
    }
    let reference = Path::new(&args[1]);
    let candidate = Path::new(&args[2]);
    ensure_not_symlink(reference)?;
    ensure_not_symlink(candidate)?;
    match kind {
        "originals" => compare_originals(reference, candidate)?,
        "sqlite" => {
            let queries = parse_search_queries(&args[3..])?;
            compare_sqlite_paths(reference, candidate, &queries)?;
        }
        "json" => compare_json_files(reference, candidate)?,
        "text" => compare_text_files(reference, candidate)?,
        "frontend" => compare_frontend_directories(reference, candidate)?,
        _ => return Err(format!("tipo di confronto non supportato: {kind}\n{USAGE}")),
    }
    println!("confronto {kind}: nessuna differenza significativa");
    Ok(())
}

fn parse_search_queries(args: &[String]) -> Result<Vec<String>, String> {
    let mut queries = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] != "--query" {
            return Err(format!(
                "argomento SQLite non supportato: {}\n{USAGE}",
                args[index]
            ));
        }
        let query = args
            .get(index + 1)
            .ok_or_else(|| "--query richiede un testo di ricerca".to_owned())?;
        if query.trim().is_empty() {
            return Err("--query non può essere vuoto".to_owned());
        }
        queries.push(query.clone());
        index += 2;
    }
    Ok(queries)
}

fn compare_originals(reference: &Path, candidate: &Path) -> Result<(), String> {
    ensure_not_symlink(reference)?;
    ensure_not_symlink(candidate)?;
    let reference_metadata = fs::metadata(reference)
        .map_err(|error| format!("impossibile leggere {}: {error}", reference.display()))?;
    let candidate_metadata = fs::metadata(candidate)
        .map_err(|error| format!("impossibile leggere {}: {error}", candidate.display()))?;
    match (reference_metadata.is_dir(), candidate_metadata.is_dir()) {
        (false, false) => compare_file_hash(reference, candidate),
        (true, true) => {
            let reference_files = collect_files(reference)?;
            let candidate_files = collect_files(candidate)?;
            compare_file_sets(
                "originali",
                &reference_files,
                &candidate_files,
                compare_file_hash,
            )
        }
        _ => Err("un lato contiene una directory e l'altro un file".to_owned()),
    }
}

fn ensure_not_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "collegamento simbolico non ammesso: {}",
            path.display()
        ));
    }
    Ok(())
}

fn compare_file_hash(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_hash = sha256_file(reference)?;
    let candidate_hash = sha256_file(candidate)?;
    if reference_hash == candidate_hash {
        Ok(())
    } else {
        Err("SHA-256 differente".to_owned())
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("impossibile aprire {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn compare_json_files(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_json: Value = serde_json::from_slice(
        &fs::read(reference)
            .map_err(|error| format!("impossibile leggere {}: {error}", reference.display()))?,
    )
    .map_err(|error| format!("JSON non valido in {}: {error}", reference.display()))?;
    let candidate_json: Value = serde_json::from_slice(
        &fs::read(candidate)
            .map_err(|error| format!("impossibile leggere {}: {error}", candidate.display()))?,
    )
    .map_err(|error| format!("JSON non valido in {}: {error}", candidate.display()))?;
    compare_json_values(&reference_json, &candidate_json)
}

fn compare_json_values(reference: &Value, candidate: &Value) -> Result<(), String> {
    if reference == candidate {
        Ok(())
    } else {
        Err("struttura o valori JSON differenti".to_owned())
    }
}

fn compare_text_files(reference: &Path, candidate: &Path) -> Result<(), String> {
    let reference_text = fs::read_to_string(reference)
        .map_err(|error| format!("testo di riferimento non UTF-8 o illeggibile: {error}"))?;
    let candidate_text = fs::read_to_string(candidate)
        .map_err(|error| format!("testo candidato non UTF-8 o illeggibile: {error}"))?;
    compare_text(&reference_text, &candidate_text)
}

fn compare_text(reference: &str, candidate: &str) -> Result<(), String> {
    if normalize_line_endings(reference) == normalize_line_endings(candidate) {
        Ok(())
    } else {
        let line = normalize_line_endings(reference)
            .lines()
            .zip(normalize_line_endings(candidate).lines())
            .position(|(left, right)| left != right)
            .map_or_else(
                || reference.lines().count().min(candidate.lines().count()) + 1,
                |index| index + 1,
            );
        Err(format!("testo differente dalla riga {line}"))
    }
}

fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn compare_frontend_directories(reference: &Path, candidate: &Path) -> Result<(), String> {
    if !reference.is_dir() || !candidate.is_dir() {
        return Err("il confronto frontend richiede due directory".to_owned());
    }
    let reference_files = collect_files(reference)?;
    let candidate_files = collect_files(candidate)?;
    compare_file_sets(
        "frontend",
        &reference_files,
        &candidate_files,
        compare_frontend_file,
    )
}

fn compare_frontend_file(reference: &Path, candidate: &Path) -> Result<(), String> {
    let extension = reference
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "json" {
        compare_json_files(reference, candidate)
    } else if matches!(
        extension.as_str(),
        "css" | "csv" | "html" | "js" | "md" | "svg" | "txt" | "xml"
    ) {
        compare_text_files(reference, candidate)
    } else {
        compare_file_hash(reference, candidate)
    }
}

fn collect_files(root: &Path) -> Result<BTreeMap<PathBuf, PathBuf>, String> {
    fn visit(
        root: &Path,
        current: &Path,
        files: &mut BTreeMap<PathBuf, PathBuf>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(current)
            .map_err(|error| format!("impossibile leggere {}: {error}", current.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("errore enumerando {}: {error}", current.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("impossibile leggere {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "collegamento simbolico non ammesso: {}",
                    path.display()
                ));
            }
            if file_type.is_dir() {
                visit(root, &path, files)?;
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| format!("percorso fuori dalla radice: {error}"))?
                    .to_path_buf();
                files.insert(relative, path);
            }
        }
        Ok(())
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn compare_file_sets(
    label: &str,
    reference: &BTreeMap<PathBuf, PathBuf>,
    candidate: &BTreeMap<PathBuf, PathBuf>,
    compare_file: fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    if reference.keys().ne(candidate.keys()) {
        return Err(format!("insieme dei percorsi {label} differente"));
    }
    for (relative, reference_path) in reference {
        let candidate_path = candidate
            .get(relative)
            .expect("gli insiemi dei percorsi sono stati confrontati");
        compare_file(reference_path, candidate_path)
            .map_err(|error| format!("{}: {error}", relative.display()))?;
    }
    Ok(())
}

fn compare_sqlite_paths(
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

fn compare_sqlite_connections(
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

#[derive(Debug, PartialEq, Eq)]
struct SqliteSnapshot {
    schema: Vec<(String, String, String, Option<String>)>,
    tables: Vec<String>,
    fts_tables: Vec<String>,
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

fn table_rows_equal(
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

fn fts_search_results_equal(
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

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::{
        compare_file_hash, compare_frontend_directories, compare_json_values, compare_originals,
        compare_sqlite_connections, compare_text, compare_text_files, fts_search_results_equal,
        sha256_file,
    };
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
            fs::write(root.join("index.html"), "<main>synthetic</main>\n")
                .expect("documento frontend");
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
}
