//! Source-level inventory for planning and auditing logging instrumentation.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};
use syn::visit::{self, Visit};
use syn::{
    Attribute, ExprCall, ExprClosure, ExprPath, ForeignItemFn, ImplItemFn, ItemFn, ItemForeignMod,
    ItemImpl, ItemMacro, ItemMod, ItemTrait, Macro, Meta, TraitItemFn,
};

/// Print a deterministic inventory of the product crates, excluding test files.
pub(crate) fn run() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for package in [
        "sanikey-core",
        "sanikey-cli",
        "sanikey-desktop",
        "sanikey-windows-volume",
        "sanikey-macos-volume",
    ] {
        collect_sources(&root.join("crates").join(package).join("src"), &mut files)?;
    }
    files.sort();
    let mut functions = Vec::new();
    let mut opaque_macros = Vec::new();
    let mut trace_hints = Vec::new();
    let mut trace_macro_hints = Vec::new();
    let mut trace_function_hints = Vec::new();
    for path in files {
        let source =
            fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let syntax =
            syn::parse_file(&source).map_err(|error| format!("{}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?;
        let mut visitor = Inventory::default();
        visitor.visit_file(&syntax);
        let digest = format!("{:x}", Sha256::digest(source.as_bytes()));
        let file = relative.to_string_lossy().replace('\\', "/");
        for (ordinal, item) in visitor.entries.into_iter().enumerate() {
            functions.push(json!({
                "id": format!("{}#{}:{}", file, digest, ordinal),
                "file": file,
                "source_sha256": digest,
                "ordinal": ordinal,
                "kind": item.kind,
                "scope": item.scope,
                "name": item.name,
                "conditions": item.conditions,
                "instrument_attribute": item.instrument_attribute,
                "abi": item.abi,
                "coverage": {"status": "unreviewed"},
            }));
        }
        trace_macro_hints.extend(visitor.opaque_macros.iter()
            .filter(|(_, path, kind)| *kind == "invocation" && is_trace_macro_hint(path))
            .map(|(scope, path, _)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "hint": "trace macro name; not proof of symbol coverage"})));
        opaque_macros.extend(visitor.opaque_macros.into_iter().map(|(scope, path, kind)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "kind": kind})));
        trace_hints.extend(visitor.trace_current_sites.into_iter().map(|scope| json!({"file": file, "source_sha256": digest, "scope": scope, "hint": "trace_current call site; not proof of symbol coverage"})));
        trace_function_hints.extend(visitor.trace_function_sites.into_iter().map(|(scope, path)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "hint": "trace wrapper name; not proof of symbol coverage"})));
    }
    let review_path = root.join("docs/process/log-inventory-review.json");
    let review_source = fs::read_to_string(&review_path)
        .map_err(|error| format!("{}: {error}", review_path.display()))?;
    let reviews = parse_review_manifest(&review_source, &functions, &root)
        .map_err(|error| format!("{}: {error}", review_path.display()))?;
    annotate_reviews(&mut functions, &reviews);
    let report = json!({
        "schema": 2,
        "review_manifest": "docs/process/log-inventory-review.json",
        "review_schema": 2,
        "entries": functions,
        "opaque_macros": opaque_macros,
        "trace_current_call_sites": trace_hints,
        "trace_macro_call_sites": trace_macro_hints,
        "trace_function_call_sites": trace_function_hints,
        "limitations": [
            "Inventario sintattico: gli attributi di strumentazione non dimostrano la copertura a runtime.",
            "Gli esiti covered ed exception sono revisioni manuali con evidenza; non sono dedotti dagli indizi sintattici.",
            "L'evidenza registrata è testo fornito dal revisore e la sua validità semantica non è verificata automaticamente.",
            "I token e gli attributi macro non sono espansi; funzioni generate richiedono un inventario separato.",
            "Le condizioni cfg sono riportate come sintassi ereditata, ma non valutate per piattaforma o feature.",
            "Gli elementi con cfg_attr sono conservati salvo esclusione indipendente: l'attributo condizionale potrebbe non applicarsi in produzione.",
            "Le dichiarazioni FFI sono elencate ai confini sintattici; argomenti e risultati effettivamente scambiati non sono osservati.",
            "Le chiamate a simboli esterni non sono collegate automaticamente alle dichiarazioni FFI; macro e risoluzione dei nomi possono nascondere il confine effettivo.",
            "L'identità id è stabile per lo stesso contenuto del file; modifiche precedenti possono cambiare gli ordinali successivi.",
            "Le chiamate trace_current sono indizi sintattici separati e non dimostrano la strumentazione di un simbolo.",
            "Le macro trace_*_call sono riconosciute solo dal nome; i loro token non sono espansi e la presenza non prova la copertura.",
            "Le funzioni wrapper di tracing sono riconosciute solo dal nome e possono includere falsi positivi; la presenza non prova la copertura.",
            "Sono escluse le directory tests, i file tests.rs e *_tests.rs e gli elementi marcati cfg(test) o test.",
            "I file esterni di moduli non ereditano automaticamente le condizioni cfg della dichiarazione nel file padre; la verifica dei moduli condizionali resta necessaria."
        ]
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[derive(Debug)]
struct ReviewRecord {
    status: String,
    evidence: String,
    evidence_file: String,
    evidence_sha256: String,
    rationale: Option<String>,
}

fn annotate_reviews(entries: &mut [serde_json::Value], reviews: &HashMap<String, ReviewRecord>) {
    for entry in entries {
        let id = entry["id"].as_str().expect("inventory entries have ids");
        entry["coverage"] = match reviews.get(id) {
            Some(review) => {
                let mut outcome = json!({
                    "status": review.status,
                    "evidence": review.evidence,
                    "evidence_file": review.evidence_file,
                    "evidence_sha256": review.evidence_sha256,
                });
                if let Some(rationale) = &review.rationale {
                    outcome["rationale"] = json!(rationale);
                }
                outcome
            }
            None => {
                let mut outcome = json!({"status": "unreviewed"});
                if let Some(hint) = mechanical_exception_hint(
                    entry["kind"]
                        .as_str()
                        .expect("inventory entries have kinds"),
                ) {
                    outcome["suggested_exception_reason"] = json!(hint);
                }
                outcome
            }
        };
    }
}

/// Parse review records and reject stale IDs or source/evidence hash mismatches.
fn parse_review_manifest(
    source: &str,
    current_entries: &[serde_json::Value],
    repository_root: &Path,
) -> Result<HashMap<String, ReviewRecord>, String> {
    let manifest: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("JSON non valido: {error}"))?;
    if manifest["schema"].as_u64() != Some(2) {
        return Err("schema del registro revisioni non supportato (atteso 2)".into());
    }
    let records = manifest["reviews"]
        .as_array()
        .ok_or_else(|| "il campo reviews deve essere un array".to_string())?;
    let current: HashMap<&str, &serde_json::Value> = current_entries
        .iter()
        .filter_map(|entry| Some((entry["id"].as_str()?, entry)))
        .collect();
    let mut seen = HashSet::new();
    let mut reviews = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        let prefix = format!("reviews[{index}]");
        let id = record["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| format!("{prefix}.id deve essere una stringa non vuota"))?;
        if !seen.insert(id) {
            return Err(format!("ID duplicato nel registro revisioni: {id}"));
        }
        let entry = current
            .get(id)
            .ok_or_else(|| format!("ID obsoleto o sconosciuto nel registro revisioni: {id}"))?;
        let source_sha256 = entry["source_sha256"]
            .as_str()
            .ok_or_else(|| format!("hash sorgente mancante per l'ID corrente: {id}"))?;
        let id_hash = id
            .rsplit_once('#')
            .and_then(|(_, suffix)| suffix.rsplit_once(':').map(|(hash, _)| hash))
            .ok_or_else(|| format!("formato ID non valido: {id}"))?;
        let recorded_hash = record["source_sha256"]
            .as_str()
            .ok_or_else(|| format!("{prefix}.source_sha256 deve essere una stringa"))?;
        if id_hash != source_sha256 || recorded_hash != source_sha256 {
            return Err(format!("hash sorgente non corrispondente per l'ID: {id}"));
        }
        let status = record["status"]
            .as_str()
            .ok_or_else(|| format!("{prefix}.status deve essere covered oppure exception"))?;
        if !matches!(status, "covered" | "exception") {
            return Err(format!(
                "{prefix}.status deve essere covered oppure exception"
            ));
        }
        let evidence = record["evidence"]
            .as_str()
            .map(str::trim)
            .filter(|evidence| !evidence.is_empty())
            .ok_or_else(|| format!("{prefix}.evidence deve descrivere una prova non vuota"))?;
        let evidence_file = record["evidence_file"]
            .as_str()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| format!("{prefix}.evidence_file deve indicare un path relativo"))?;
        let evidence_sha256 = record["evidence_sha256"]
            .as_str()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| format!("{prefix}.evidence_sha256 deve essere SHA-256 esadecimale"))?;
        verify_evidence_file(repository_root, evidence_file, evidence_sha256)
            .map_err(|error| format!("{prefix}: {error}"))?;
        let rationale = match status {
            "exception" => Some(
                record["rationale"]
                    .as_str()
                    .map(str::trim)
                    .filter(|rationale| !rationale.is_empty())
                    .ok_or_else(|| format!("{prefix}.rationale è obbligatoria per un'eccezione"))?
                    .to_string(),
            ),
            _ => None,
        };
        let kind = entry["kind"].as_str().unwrap_or_default();
        if status == "covered" && mechanical_exception_hint(kind).is_some() {
            return Err(format!(
                "{prefix}: {kind} richiede esito exception perché non ha una span runtime autonoma"
            ));
        }
        reviews.insert(
            id.to_string(),
            ReviewRecord {
                status: status.to_string(),
                evidence: evidence.to_string(),
                evidence_file: evidence_file.to_string(),
                evidence_sha256: evidence_sha256.to_ascii_lowercase(),
                rationale,
            },
        );
    }
    Ok(reviews)
}

fn verify_evidence_file(
    repository_root: &Path,
    relative_path: &str,
    expected_sha256: &str,
) -> Result<(), String> {
    if relative_path.starts_with('/')
        || relative_path.contains('\\')
        || relative_path.contains(':')
        || relative_path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!(
            "evidence_file deve essere relativo, normalizzato e senza componenti parent: {relative_path}"
        ));
    }
    let root = fs::canonicalize(repository_root)
        .map_err(|error| format!("impossibile risolvere la root del repository: {error}"))?;
    let candidate = root.join(relative_path);
    let evidence = fs::canonicalize(&candidate).map_err(|error| {
        format!("file di evidenza mancante o non leggibile ({relative_path}): {error}")
    })?;
    if !evidence.starts_with(&root) {
        return Err(format!(
            "il file di evidenza esce dal repository: {relative_path}"
        ));
    }
    if !evidence.is_file() {
        return Err(format!(
            "il file di evidenza non è un file regolare: {relative_path}"
        ));
    }
    let contents = fs::read(&evidence).map_err(|error| {
        format!("impossibile leggere il file di evidenza {relative_path}: {error}")
    })?;
    let actual_sha256 = sha256_hex(&contents);
    if actual_sha256 != expected_sha256.to_ascii_lowercase() {
        return Err(format!(
            "SHA-256 dell'evidenza cambiato per {relative_path}: atteso {expected_sha256}, rilevato {actual_sha256}"
        ));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Suggest a mechanical exception reason without deciding its review outcome.
fn mechanical_exception_hint(kind: &str) -> Option<&'static str> {
    match kind {
        "const_fn" | "const_method" => {
            Some("una funzione const non può chiamare API di tracing runtime nel proprio corpo")
        }
        "trait_declaration" => {
            Some("la dichiarazione del trait non ha un corpo; tracciare l'implementazione concreta")
        }
        _ => None,
    }
}

fn is_trace_macro_hint(path: &str) -> bool {
    let name = path.rsplit("::").next().unwrap_or(path);
    name.starts_with("trace_") && name.ends_with("_call")
}

/// Recognizes known wrapper names as hints, without proving instrumentation.
fn is_trace_function_hint(path: &str) -> bool {
    is_trace_macro_hint(path) || path.rsplit("::").next() == Some("trace_export_operation")
}

fn collect_sources(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() && entry.file_name() != "tests" {
            collect_sources(&path, files)?;
        } else if kind.is_file()
            && path.extension().is_some_and(|extension| extension == "rs")
            && entry.file_name() != "tests.rs"
            && !entry.file_name().to_string_lossy().ends_with("_tests.rs")
        {
            files.push(path);
        }
    }
    Ok(())
}

#[derive(Default)]
struct Inventory {
    scope: Vec<String>,
    conditions: Vec<String>,
    entries: Vec<Entry>,
    opaque_macros: Vec<(String, String, &'static str)>,
    trace_current_sites: Vec<String>,
    trace_function_sites: Vec<(String, String)>,
    foreign_abi: Option<String>,
}

struct Entry {
    kind: &'static str,
    scope: String,
    name: String,
    conditions: Vec<String>,
    instrument_attribute: bool,
    abi: Option<String>,
}

fn has_test_condition(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => {
            use syn::parse::Parser;
            use syn::punctuated::Punctuated;
            let Ok(items) =
                Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse2(list.tokens.clone())
            else {
                return false;
            };
            let flags: Vec<bool> = items.iter().map(has_test_condition).collect();
            match list
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .as_deref()
            {
                Some("all") => flags.iter().any(|flag| *flag),
                Some("any") => !flags.is_empty() && flags.iter().all(|flag| *flag),
                Some("not") => false,
                _ => false,
            }
        }
        Meta::NameValue(_) => false,
    }
}

fn meta_label(meta: &Meta) -> String {
    match meta {
        Meta::Path(path) => path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
        Meta::List(list) => {
            format!(
                "{}({})",
                list.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::"),
                list.tokens
            )
        }
        Meta::NameValue(value) => {
            format!(
                "{}=<value>",
                value
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            )
        }
    }
}

fn test_only(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if attribute.path().is_ident("test") {
            return true;
        }
        if attribute.path().is_ident("cfg") {
            return attribute
                .meta
                .require_list()
                .ok()
                .and_then(|list| syn::parse2::<Meta>(list.tokens.clone()).ok())
                .is_some_and(|meta| has_test_condition(&meta));
        }
        // A conditional attribute does not necessarily apply in production:
        // cfg_attr(test, cfg(test)) leaves the item present when test is false.
        // Keep it unless an independent cfg proves it test-only.
        false
    })
}

/// Records syntax entries and their enclosing conditions and scopes.
impl Inventory {
    fn add(&mut self, kind: &'static str, name: String, attributes: &[Attribute]) {
        self.entries.push(Entry {
            kind,
            scope: self.scope.join("::"),
            name,
            conditions: self
                .conditions
                .iter()
                .cloned()
                .chain(
                    attributes
                        .iter()
                        .filter(|attribute| {
                            attribute.path().is_ident("cfg")
                                || attribute.path().is_ident("cfg_attr")
                        })
                        .map(|attribute| meta_label(&attribute.meta)),
                )
                .collect(),
            instrument_attribute: attributes.iter().any(|attribute| {
                attribute
                    .path()
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "instrument")
            }),
            abi: None,
        });
    }

    fn enter(&mut self, name: String, attributes: &[Attribute], f: impl FnOnce(&mut Self)) {
        self.scope.push(name);
        let prior = self.conditions.len();
        self.conditions.extend(
            attributes
                .iter()
                .filter(|attribute| {
                    attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
                })
                .map(|attribute| meta_label(&attribute.meta)),
        );
        f(self);
        self.conditions.truncate(prior);
        self.scope.pop();
    }
}

/// Visits Rust syntax to inventory functions, macros and FFI declarations.
impl<'ast> Visit<'ast> for Inventory {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if !test_only(&node.attrs) {
            self.enter(node.ident.to_string(), &node.attrs, |this| {
                visit::visit_item_mod(this, node)
            });
        }
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        if !test_only(&node.attrs) {
            let name = match node.self_ty.as_ref() {
                syn::Type::Path(path) => path
                    .path
                    .segments
                    .last()
                    .map_or_else(|| "<impl>".into(), |part| part.ident.to_string()),
                _ => "<impl>".into(),
            };
            let trait_name = node.trait_.as_ref().map(|(_, path, _)| {
                format!(
                    "trait:{}",
                    path.segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::")
                )
            });
            self.enter(
                trait_name.map_or(name.clone(), |trait_name| format!("{name}::{trait_name}")),
                &node.attrs,
                |this| visit::visit_item_impl(this, node),
            );
        }
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        if !test_only(&node.attrs) {
            self.enter(node.ident.to_string(), &node.attrs, |this| {
                visit::visit_item_trait(this, node)
            });
        }
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast ItemForeignMod) {
        let abi = node
            .abi
            .name
            .as_ref()
            .map_or_else(|| "C".into(), |name| name.value());
        let previous = self.foreign_abi.replace(abi.clone());
        self.enter(format!("extern:{abi}"), &node.attrs, |this| {
            visit::visit_item_foreign_mod(this, node)
        });
        self.foreign_abi = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if !test_only(&node.attrs) {
            self.add(
                if node.sig.constness.is_some() {
                    "const_method"
                } else {
                    "method"
                },
                node.sig.ident.to_string(),
                &node.attrs,
            );
            self.enter(node.sig.ident.to_string(), &node.attrs, |this| {
                visit::visit_impl_item_fn(this, node)
            });
        }
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        if !test_only(&node.attrs) {
            self.add(
                if node.default.is_some() {
                    "trait_default"
                } else {
                    "trait_declaration"
                },
                node.sig.ident.to_string(),
                &node.attrs,
            );
            self.enter(node.sig.ident.to_string(), &node.attrs, |this| {
                visit::visit_trait_item_fn(this, node)
            });
        }
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        self.add("closure", "<closure>".into(), &node.attrs);
        visit::visit_expr_closure(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let syn::Expr::Path(ExprPath { path, .. }) = node.func.as_ref() {
            let name = path
                .segments
                .last()
                .map(|segment| segment.ident.to_string());
            if name.as_deref() == Some("trace_current") {
                self.trace_current_sites.push(self.scope.join("::"));
            } else if name.as_deref().is_some_and(is_trace_function_hint) {
                self.trace_function_sites
                    .push((self.scope.join("::"), name.unwrap()));
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        let path = node
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        self.opaque_macros
            .push((self.scope.join("::"), path.clone(), "invocation"));
        self.add("macro_invocation", path, &[]);
    }

    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        if let Some(name) = &node.ident {
            let name = name.to_string();
            self.add("macro_definition", name.clone(), &node.attrs);
            self.opaque_macros
                .push((self.scope.join("::"), name, "definition"));
        } else {
            visit::visit_item_macro(self, node);
        }
    }

    fn visit_foreign_item_fn(&mut self, node: &'ast ForeignItemFn) {
        if !test_only(&node.attrs) {
            self.add("ffi_declaration", node.sig.ident.to_string(), &node.attrs);
            if let Some(entry) = self.entries.last_mut() {
                entry.abi = self.foreign_abi.clone();
            }
        }
        visit::visit_foreign_item_fn(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if !test_only(&node.attrs) {
            self.add(
                if node.sig.abi.is_some() {
                    "ffi_function"
                } else if node.sig.constness.is_some() {
                    "const_fn"
                } else {
                    "function"
                },
                node.sig.ident.to_string(),
                &node.attrs,
            );
            if let (Some(abi), Some(entry)) = (&node.sig.abi, self.entries.last_mut()) {
                entry.abi = Some(
                    abi.name
                        .as_ref()
                        .map_or_else(|| "C".into(), |name| name.value()),
                );
            }
            self.enter(node.sig.ident.to_string(), &node.attrs, |this| {
                visit::visit_item_fn(this, node)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Inventory, Visit, annotate_reviews, is_trace_function_hint, is_trace_macro_hint,
        mechanical_exception_hint, parse_review_manifest, sha256_hex,
    };
    use serde_json::{Value, json};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_EVIDENCE_REPO: AtomicUsize = AtomicUsize::new(0);

    struct EvidenceRepo(PathBuf);

    /// Creates and inspects a synthetic repository for evidence validation.
    impl EvidenceRepo {
        fn new() -> Self {
            let id = NEXT_EVIDENCE_REPO.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "sanikey-log-inventory-review-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(path.join("tests")).expect("directory prova evidenza");
            fs::write(path.join("tests/evidence.rs"), b"evidence fixture v1\n")
                .expect("fixture evidenza sintetica");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn evidence_sha256(&self) -> String {
            sha256_hex(&fs::read(self.0.join("tests/evidence.rs")).expect("lettura evidenza"))
        }
    }

    /// Removes the synthetic repository after each evidence-validation test.
    impl Drop for EvidenceRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn review_record(
        id: &str,
        source_sha256: &str,
        status: &str,
        evidence: &str,
        evidence_sha256: &str,
        rationale: Option<&str>,
    ) -> Value {
        let mut record = json!({
            "id": id,
            "source_sha256": source_sha256,
            "status": status,
            "evidence": evidence,
            "evidence_file": "tests/evidence.rs",
            "evidence_sha256": evidence_sha256,
        });
        if let Some(rationale) = rationale {
            record["rationale"] = json!(rationale);
        }
        record
    }

    #[test]
    fn recognizes_only_trace_macro_name_hints() {
        assert!(is_trace_macro_hint("crate::logging::trace_build_call"));
        assert!(!is_trace_macro_hint("trace_current"));
        assert!(!is_trace_macro_hint("some_other_call"));
        assert!(is_trace_function_hint("trace_export_operation"));
        assert!(!is_trace_function_hint("trace_sensitive_text"));
    }

    #[test]
    fn inventories_functions_methods_and_closures_without_test_modules() {
        let file = syn::parse_file(
            r#"
            fn work() { let _callback = || 1; }
            impl Example { fn method(&self) {} const fn constant() {} }
            #[cfg(test)] mod tests { fn hidden() {} }
        "#,
        )
        .unwrap();
        let mut inventory = Inventory::default();
        inventory.visit_file(&file);
        let entries: Vec<_> = inventory
            .entries
            .iter()
            .map(|entry| (entry.kind, entry.name.as_str()))
            .collect();
        assert_eq!(
            entries,
            [
                ("function", "work"),
                ("closure", "<closure>"),
                ("method", "method"),
                ("const_method", "constant")
            ]
        );
    }

    #[test]
    fn coverage_defaults_to_unreviewed_and_mechanical_cases_are_only_hints() {
        let file = syn::parse_file(
            r#"
            #[tracing::instrument]
            fn instrument_attribute_is_not_proof() {}
            fn wrapper_hint_is_not_proof() { trace_core_call!("entry"); }
            const fn compile_time_only() {}
            impl Example {
                const fn constant_method() {}
                fn ordinary_method(&self) {}
            }
            trait Service { fn declaration(&self); }
        "#,
        )
        .unwrap();
        let mut inventory = Inventory::default();
        inventory.visit_file(&file);

        let instrumented = inventory
            .entries
            .iter()
            .find(|entry| {
                entry.name == "instrument_attribute_is_not_proof" && entry.kind == "function"
            })
            .unwrap();
        assert!(instrumented.instrument_attribute);
        assert_eq!(mechanical_exception_hint("function"), None);
        assert!(mechanical_exception_hint("const_fn").is_some());
        assert!(mechanical_exception_hint("const_method").is_some());
        assert!(mechanical_exception_hint("trait_declaration").is_some());
        assert_eq!(mechanical_exception_hint("trait_default"), None);

        let entries: Vec<Value> = inventory
            .entries
            .iter()
            .enumerate()
            .map(|(ordinal, entry)| {
                json!({
                    "id": format!("fixture.rs#{}:{ordinal}", "a".repeat(64)),
                    "kind": entry.kind,
                    "instrument_attribute": entry.instrument_attribute,
                    "coverage": {"status": "unreviewed"}
                })
            })
            .collect();
        let mut entries = entries;
        annotate_reviews(&mut entries, &std::collections::HashMap::new());
        assert!(
            entries
                .iter()
                .all(|entry| entry["coverage"]["status"] == "unreviewed")
        );
        let const_entry = entries
            .iter()
            .find(|entry| entry["kind"] == "const_fn")
            .unwrap();
        assert!(const_entry["coverage"]["suggested_exception_reason"].is_string());
        let instrumented_entry = entries
            .iter()
            .find(|entry| entry["instrument_attribute"] == true)
            .unwrap();
        assert_eq!(instrumented_entry["coverage"]["status"], "unreviewed");
    }

    #[test]
    fn explicit_covered_outcome_is_hash_bound_and_changes_coverage_status() {
        let evidence_root = EvidenceRepo::new();
        let evidence_sha256 = evidence_root.evidence_sha256();
        let id = format!("crates/sanikey-core/src/a.rs#{}:0", "a".repeat(64));
        let source_hash = "a".repeat(64);
        let mut entry = json!({
            "id": id,
            "source_sha256": source_hash,
            "kind": "function",
            "coverage": {"status": "unreviewed"}
        });
        let records = parse_review_manifest(
            &serde_json::to_string(&json!({
                "schema": 2,
                "reviews": [review_record(
                    &id,
                    &source_hash,
                    "covered",
                    "test sintetico: file e riga di asserzione",
                    &evidence_sha256,
                    None,
                )]
            }))
            .unwrap(),
            std::slice::from_ref(&entry),
            evidence_root.path(),
        )
        .unwrap();

        annotate_reviews(std::slice::from_mut(&mut entry), &records);
        assert_eq!(entry["coverage"]["status"], "covered");
        assert_eq!(
            entry["coverage"]["evidence"],
            "test sintetico: file e riga di asserzione"
        );
        assert_eq!(entry["coverage"]["evidence_file"], "tests/evidence.rs");
        assert_eq!(entry["coverage"]["evidence_sha256"], evidence_sha256);
    }

    #[test]
    fn explicit_exception_requires_rationale_and_special_cases_cannot_be_covered() {
        let evidence_root = EvidenceRepo::new();
        let evidence_sha256 = evidence_root.evidence_sha256();
        let hash = "d".repeat(64);
        let special_id = format!("crates/sanikey-core/src/c.rs#{hash}:2");
        let trait_id = format!("crates/sanikey-core/src/c.rs#{hash}:3");
        let entries = [
            json!({
                "id": special_id,
                "source_sha256": hash,
                "kind": "const_fn",
                "coverage": {"status": "unreviewed"}
            }),
            json!({
                "id": trait_id,
                "source_sha256": hash,
                "kind": "trait_declaration",
                "coverage": {"status": "unreviewed"}
            }),
        ];
        let record = review_record(
            &special_id,
            &hash,
            "exception",
            "firma const verificata nel sorgente",
            &evidence_sha256,
            Some("non può invocare API di tracing runtime"),
        );
        let records = parse_review_manifest(
            &json!({"schema": 2, "reviews": [record]}).to_string(),
            &entries,
            evidence_root.path(),
        )
        .unwrap();
        let mut annotated = entries.clone();
        annotate_reviews(&mut annotated, &records);
        assert_eq!(annotated[0]["coverage"]["status"], "exception");
        assert_eq!(
            annotated[0]["coverage"]["rationale"],
            "non può invocare API di tracing runtime"
        );

        let covered_special = review_record(
            &special_id,
            &hash,
            "covered",
            "prova",
            &evidence_sha256,
            None,
        );
        assert!(
            parse_review_manifest(
                &json!({"schema": 2, "reviews": [covered_special]}).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("richiede esito exception")
        );

        let covered_trait =
            review_record(&trait_id, &hash, "covered", "prova", &evidence_sha256, None);
        assert!(
            parse_review_manifest(
                &json!({"schema": 2, "reviews": [covered_trait]}).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("richiede esito exception")
        );

        let missing_rationale = review_record(
            &special_id,
            &hash,
            "exception",
            "prova",
            &evidence_sha256,
            None,
        );
        assert!(
            parse_review_manifest(
                &json!({"schema": 2, "reviews": [missing_rationale]}).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("rationale")
        );
    }

    #[test]
    fn rejects_duplicate_stale_hash_mismatched_invalid_status_and_missing_evidence() {
        let evidence_root = EvidenceRepo::new();
        let evidence_sha256 = evidence_root.evidence_sha256();
        let hash = "b".repeat(64);
        let id = format!("crates/sanikey-core/src/b.rs#{hash}:4");
        let entry = json!({"id": id, "source_sha256": hash});
        let entries = [entry];
        let valid = review_record(
            &id,
            &hash,
            "covered",
            "fixture sintetica e asserzione test",
            &evidence_sha256,
            None,
        );
        let manifest = |reviews: Vec<Value>| json!({"schema": 2, "reviews": reviews});

        assert!(
            parse_review_manifest(
                &manifest(vec![valid.clone(), valid.clone()]).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("duplicato")
        );
        assert!(parse_review_manifest(
            &manifest(vec![json!({"id":"stale.rs#dead:0","source_sha256":"dead","status":"covered","evidence":"x"})]).to_string(),
            &entries,
            evidence_root.path(),
        )
        .unwrap_err()
        .contains("obsoleto"));

        let mut bad_hash = valid.clone();
        bad_hash["source_sha256"] = json!("c".repeat(64));
        assert!(
            parse_review_manifest(
                &manifest(vec![bad_hash]).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("hash sorgente")
        );

        let mut invalid_status = valid.clone();
        invalid_status["status"] = json!("reviewed");
        assert!(
            parse_review_manifest(
                &manifest(vec![invalid_status]).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("covered oppure exception")
        );

        let mut no_evidence = valid;
        no_evidence["evidence"] = json!("  ");
        assert!(
            parse_review_manifest(
                &manifest(vec![no_evidence]).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("evidence")
        );
    }

    #[test]
    fn rejects_absolute_parent_missing_and_changed_evidence_files() {
        let evidence_root = EvidenceRepo::new();
        let evidence_sha256 = evidence_root.evidence_sha256();
        let source_hash = "e".repeat(64);
        let id = format!("crates/sanikey-core/src/e.rs#{source_hash}:1");
        let entries = [json!({
            "id": id,
            "source_sha256": source_hash,
            "kind": "function"
        })];
        let valid = review_record(
            &id,
            &source_hash,
            "covered",
            "fixture locale verificata",
            &evidence_sha256,
            None,
        );
        let manifest = |review: Value| json!({"schema": 2, "reviews": [review]});

        assert!(
            parse_review_manifest(
                &manifest(valid.clone()).to_string(),
                &entries,
                evidence_root.path(),
            )
            .is_ok()
        );

        let mut absolute = valid.clone();
        absolute["evidence_file"] = json!(evidence_root.0.join("tests/evidence.rs"));
        assert!(
            parse_review_manifest(
                &manifest(absolute).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("relativo")
        );

        let mut parent = valid.clone();
        parent["evidence_file"] = json!("../tests/evidence.rs");
        assert!(
            parse_review_manifest(
                &manifest(parent).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("componenti parent")
        );

        let mut missing = valid.clone();
        missing["evidence_file"] = json!("tests/missing.rs");
        assert!(
            parse_review_manifest(
                &manifest(missing).to_string(),
                &entries,
                evidence_root.path(),
            )
            .unwrap_err()
            .contains("mancante")
        );

        fs::write(
            evidence_root.0.join("tests/evidence.rs"),
            b"evidence fixture v2\n",
        )
        .expect("modifica evidenza sintetica");
        assert!(
            parse_review_manifest(&manifest(valid).to_string(), &entries, evidence_root.path(),)
                .unwrap_err()
                .contains("SHA-256 dell'evidenza cambiato")
        );
    }

    #[test]
    fn preserves_identity_context_conditions_macros_traits_and_ffi_boundaries() {
        let file = syn::parse_file(
            r#"
            #[cfg(feature = "desktop")]
            mod ui {
                macro_rules! make { () => { fn generated() {} } }
                make!();
                #[cfg(any(test, windows))] fn supported_on_test_or_windows() {}
                #[cfg(all(unix, feature = "native"))] fn native() { trace_current("entry"); }
                fn wrapped() { trace_cli_call("entry"); }
                trait Service { fn declared(&self); fn defaulted(&self) {} }
                impl Service for Device { fn declared(&self) {} }
                extern "C" { pub fn host_callback(value: i32) -> i32; }
                extern "system" fn exported() {}
                #[cfg(test)] fn test_only() {}
                #[cfg(all(test, unix))] fn unix_test_only() {}
                #[cfg_attr(test, cfg(test))] fn attr_test_only() {}
            }
            "#,
        )
        .unwrap();
        let mut inventory = Inventory::default();
        inventory.visit_file(&file);

        let get = |name: &str, kind: &str| {
            inventory
                .entries
                .iter()
                .find(|entry| entry.name == name && entry.kind == kind)
                .unwrap()
        };
        assert_eq!(get("native", "function").conditions.len(), 2);
        assert!(
            get("supported_on_test_or_windows", "function")
                .conditions
                .iter()
                .any(|condition| condition.contains("windows"))
        );
        assert!(
            get("declared", "trait_declaration")
                .scope
                .ends_with("Service")
        );
        assert_eq!(get("defaulted", "trait_default").kind, "trait_default");
        assert_eq!(
            get("host_callback", "ffi_declaration").abi.as_deref(),
            Some("C")
        );
        assert_eq!(
            get("exported", "ffi_function").abi.as_deref(),
            Some("system")
        );
        assert!(
            inventory
                .entries
                .iter()
                .any(|entry| entry.kind == "macro_invocation" && entry.name == "make")
        );
        assert_eq!(inventory.opaque_macros.len(), 2);
        assert_eq!(inventory.trace_current_sites, ["ui::native"]);
        assert_eq!(
            inventory.trace_function_sites,
            [("ui::wrapped".into(), "trace_cli_call".into())]
        );
        assert!(
            !inventory
                .entries
                .iter()
                .any(|entry| matches!(entry.name.as_str(), "test_only" | "unix_test_only"))
        );
        assert!(
            inventory
                .entries
                .iter()
                .any(|entry| entry.name == "attr_test_only")
        );
    }
}
