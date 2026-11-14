//! Source-level inventory for planning and auditing logging instrumentation.

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
            }));
        }
        trace_macro_hints.extend(visitor.opaque_macros.iter()
            .filter(|(_, path, kind)| *kind == "invocation" && is_trace_macro_hint(path))
            .map(|(scope, path, _)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "hint": "trace macro name; not proof of symbol coverage"})));
        opaque_macros.extend(visitor.opaque_macros.into_iter().map(|(scope, path, kind)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "kind": kind})));
        trace_hints.extend(visitor.trace_current_sites.into_iter().map(|scope| json!({"file": file, "source_sha256": digest, "scope": scope, "hint": "trace_current call site; not proof of symbol coverage"})));
        trace_function_hints.extend(visitor.trace_function_sites.into_iter().map(|(scope, path)| json!({"file": file, "source_sha256": digest, "scope": scope, "path": path, "hint": "trace wrapper name; not proof of symbol coverage"})));
    }
    let report = json!({
        "schema": 2,
        "entries": functions,
        "opaque_macros": opaque_macros,
        "trace_current_call_sites": trace_hints,
        "trace_macro_call_sites": trace_macro_hints,
        "trace_function_call_sites": trace_function_hints,
        "limitations": [
            "Inventario sintattico: gli attributi di strumentazione non dimostrano la copertura a runtime.",
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
    use super::{Inventory, Visit, is_trace_function_hint, is_trace_macro_hint};

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
