//! Comando di confronto degli artefatti della migrazione.

mod files;
mod formats;
mod sqlite;
mod sqlite_rows;
#[cfg(test)]
mod tests;

use std::path::Path;

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
    files::ensure_not_symlink(reference)?;
    files::ensure_not_symlink(candidate)?;
    match kind {
        "originals" => files::compare_originals(reference, candidate)?,
        "sqlite" => {
            let queries = parse_search_queries(&args[3..])?;
            sqlite::compare_sqlite_paths(reference, candidate, &queries)?;
        }
        "json" => formats::compare_json_files(reference, candidate)?,
        "text" => formats::compare_text_files(reference, candidate)?,
        "frontend" => formats::compare_frontend_directories(reference, candidate)?,
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
