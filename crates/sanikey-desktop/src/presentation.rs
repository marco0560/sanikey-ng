//! Presentation text and its classification for diagnostic capture.

use sanikey_core::logging::{
    Component, DisplayControl, Session, TraceField, TraceValue, redact_secret_assignments,
    trace_current, trace_sensitive_text,
};

/// Classification supplied by the producer of displayed text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Classification {
    /// Static application text.
    Public,
    /// Patient data, paths, or other private information.
    Private,
    /// Free text whose contents cannot be classified more precisely.
    Unclassified,
    /// A value known to be a credential or other secret.
    #[allow(dead_code)] // Reserved for explicitly identified secret controls.
    Secret,
}

/// Text shown to the operator with its capture classification.
pub(crate) struct PresentedText<'a> {
    text: &'a str,
    classification: Classification,
}

/// Separates the displayed text from its classified log representation.
impl<'a> PresentedText<'a> {
    pub(crate) fn new(text: &'a str, classification: Classification) -> Self {
        let trace = trace_current(
            Component::Desktop,
            "desktop::presentation::PresentedText::new",
            || {
                vec![
                    TraceField::new("classification", TraceValue::data(classification.name())),
                    TraceField::new("text", trace_classified_text(text, classification)),
                ]
            },
        );
        let value = Self {
            text,
            classification,
        };
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::record(&[
                TraceField::new(
                    "classification",
                    TraceValue::data(value.classification.name()),
                ),
                TraceField::new("text", value.trace_text()),
            ]));
        }
        value
    }

    /// Returns the unchanged text for display.
    pub(crate) fn displayed(&self) -> &'a str {
        let trace = trace_current(
            Component::Desktop,
            "desktop::presentation::PresentedText::displayed",
            || {
                vec![
                    TraceField::new(
                        "classification",
                        TraceValue::data(self.classification.name()),
                    ),
                    TraceField::new("text", self.trace_text()),
                ]
            },
        );
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::record(&[
                TraceField::new(
                    "classification",
                    TraceValue::data(self.classification.name()),
                ),
                TraceField::new("text", self.trace_text()),
            ]));
        }
        self.text
    }

    /// Returns text eligible for the private diagnostic stream.
    /// Known secrets have no representation in that stream.
    pub(crate) fn diagnostic(&self) -> Option<String> {
        let trace = trace_current(
            Component::Desktop,
            "desktop::presentation::PresentedText::diagnostic",
            || {
                vec![
                    TraceField::new(
                        "classification",
                        TraceValue::data(self.classification.name()),
                    ),
                    TraceField::new("text", self.trace_text()),
                ]
            },
        );
        let value = match self.classification {
            Classification::Public | Classification::Private | Classification::Unclassified => {
                Some(redact_secret_assignments(self.text))
            }
            Classification::Secret => None,
        };
        if let Some(trace) = trace {
            let result = value
                .as_deref()
                .map(trace_sensitive_text)
                .unwrap_or(TraceValue::Secret);
            let _ = trace.finish(TraceValue::record(&[
                TraceField::new("text", result),
                TraceField::new("secret_omitted", TraceValue::data(value.is_none())),
            ]));
        }
        value
    }

    /// Captures this value when a session is active, and returns it for display.
    pub(crate) fn capture(
        &self,
        session: Option<&Session>,
        failed: &mut bool,
        control: DisplayControl,
    ) -> &'a str {
        let trace = trace_current(
            Component::Desktop,
            "desktop::presentation::PresentedText::capture",
            || {
                vec![
                    TraceField::new(
                        "classification",
                        TraceValue::data(self.classification.name()),
                    ),
                    TraceField::new("text", self.trace_text()),
                    TraceField::new("control", trace_sensitive_text(&format!("{control:?}"))),
                    TraceField::new("session_available", TraceValue::data(session.is_some())),
                    TraceField::new("capture_failed_before_call", TraceValue::data(*failed)),
                ]
            },
        );
        let diagnostic = self.diagnostic();
        if let Some(session) = session
            && session
                .record_displayed_text(control, diagnostic.as_deref())
                .is_err()
        {
            *failed = true;
        }
        let displayed = self.displayed();
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::record(&[
                TraceField::new("displayed_text", self.trace_text()),
                TraceField::new(
                    "diagnostic_text",
                    diagnostic
                        .as_deref()
                        .map_or_else(|| TraceValue::Secret, trace_sensitive_text),
                ),
                TraceField::new("capture_failed", TraceValue::data(*failed)),
            ]));
        }
        displayed
    }

    fn trace_text(&self) -> TraceValue {
        trace_classified_text(self.text, self.classification)
    }
}

fn trace_classified_text(text: &str, classification: Classification) -> TraceValue {
    match classification {
        Classification::Secret => TraceValue::Secret,
        Classification::Public | Classification::Private | Classification::Unclassified => {
            trace_sensitive_text(text)
        }
    }
}

/// Maps presentation classifications to stable identifiers.
impl Classification {
    const fn name(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Unclassified => "unclassified",
            Self::Secret => "secret",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Classification, PresentedText};

    #[test]
    fn private_and_unclassified_text_are_available_only_to_the_diagnostic_path() {
        for class in [Classification::Private, Classification::Unclassified] {
            let text = PresentedText::new("valore sintetico", class);
            assert_eq!(text.displayed(), "valore sintetico");
            assert_eq!(text.diagnostic().as_deref(), Some("valore sintetico"));
        }
    }

    #[test]
    fn explicitly_classified_secret_is_never_returned_for_capture() {
        let text = PresentedText::new("segreto sintetico", Classification::Secret);
        assert_eq!(text.displayed(), "segreto sintetico");
        assert_eq!(text.diagnostic(), None);
    }

    #[test]
    fn static_text_is_available_for_diagnostic_capture() {
        let text = PresentedText::new("Stato", Classification::Public);
        assert_eq!(text.diagnostic().as_deref(), Some("Stato"));
    }

    #[test]
    fn displayed_text_is_unchanged_while_diagnostic_text_redacts_secret_assignments() {
        let source = "Titolo sintetico\napi_key = \"SECRET-SYNTHETIC-DO-NOT-LOG\"";
        let text = PresentedText::new(source, Classification::Unclassified);

        assert_eq!(text.displayed(), source);
        let diagnostic = text.diagnostic().expect("testo sintetico diagnostico");
        assert!(diagnostic.contains("Titolo sintetico"));
        assert!(diagnostic.contains("secret_omitted"));
        assert!(!diagnostic.contains("SECRET-SYNTHETIC-DO-NOT-LOG"));
    }

    #[test]
    fn capture_keeps_changed_non_secret_text_and_redacts_secret_from_log() {
        use sanikey_core::logging::{Component, DisplayControl, Level, Session};
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let temporary_root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../.Temp");
        fs::create_dir_all(&temporary_root).expect("directory temporanea di progetto");
        let directory = temporary_root.join(format!(
            "sanikey-presentation-trace-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("directory log sintetica");
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Desktop, Level::Trace)
                .expect("sessione diagnostica sintetica");
        let control = DisplayControl::new("synthetic_unclassified_text");
        let mut failed = false;
        let first = "Titolo uno\napi_key = \"SECRET-FIRST-SYNTHETIC\"";
        let second = "Titolo due\napi_key = \"SECRET-SECOND-SYNTHETIC\"";
        let explicit_secret = "SECRET-CLASSIFIED-SYNTHETIC";

        assert_eq!(
            session.scoped(|| {
                PresentedText::new(first, Classification::Unclassified).capture(
                    Some(&session),
                    &mut failed,
                    control,
                )
            }),
            first
        );
        assert_eq!(
            session.scoped(|| {
                PresentedText::new(second, Classification::Unclassified).capture(
                    Some(&session),
                    &mut failed,
                    control,
                )
            }),
            second
        );
        assert_eq!(
            session.scoped(|| {
                PresentedText::new(explicit_secret, Classification::Secret).capture(
                    Some(&session),
                    &mut failed,
                    DisplayControl::new("synthetic_secret_text"),
                )
            }),
            explicit_secret
        );
        assert!(!failed);
        session.finish(Component::Desktop).expect("chiusura log");

        let contents = fs::read_to_string(path).expect("log diagnostico sintetico");
        assert!(contents.contains("Titolo uno"));
        assert!(contents.contains("Titolo due"));
        assert!(contents.contains("desktop::presentation::PresentedText::capture"));
        assert!(contents.contains("desktop::presentation::PresentedText::diagnostic"));
        assert!(!contents.contains("SECRET-FIRST-SYNTHETIC"));
        assert!(!contents.contains("SECRET-SECOND-SYNTHETIC"));
        assert!(!contents.contains(explicit_secret));
        assert!(contents.contains("desktop::presentation::PresentedText::new"));
        assert!(contents.contains("desktop::presentation::PresentedText::displayed"));
        assert!(contents.contains("secret_omitted"));
        let records: Vec<serde_json::Value> = contents
            .lines()
            .map(|line| serde_json::from_str(line).expect("record JSON sintetico"))
            .collect();
        for method in ["new", "displayed", "diagnostic", "capture"] {
            let function = format!("desktop::presentation::PresentedText::{method}");
            for classification in ["unclassified", "secret"] {
                let entry = records
                    .iter()
                    .find(|record| {
                        record["type"] == "function_enter"
                            && record["function"] == function
                            && record["arguments"].as_array().is_some_and(|arguments| {
                                arguments.iter().any(|field| {
                                    field["name"] == "classification"
                                        && field["content"]["value"] == classification
                                })
                            })
                    })
                    .unwrap_or_else(|| panic!("ingresso {function} {classification} mancante"));
                let exit = records
                    .iter()
                    .find(|record| {
                        record["type"] == "function_exit" && record["call_id"] == entry["call_id"]
                    })
                    .unwrap_or_else(|| panic!("uscita {function} {classification} mancante"));
                assert!(exit["duration_ms"].is_number());
                assert_eq!(exit["outcome"], "returned");
                if method == "capture" {
                    let args = entry["arguments"].to_string();
                    assert!(args.contains("session_available"));
                    assert!(args.contains("capture_failed_before_call"));
                    assert!(args.contains("synthetic_"));
                    assert!(exit["result"].to_string().contains("capture_failed"));
                }
                if classification == "secret" {
                    assert!(entry["arguments"].to_string().contains("secret_omitted"));
                    assert!(exit["result"].to_string().contains("secret_omitted"));
                } else {
                    assert!(entry["arguments"].to_string().contains("Titolo"));
                    assert!(exit["result"].to_string().contains("Titolo"));
                }
            }
        }
        fs::remove_dir_all(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn capture_redacts_escaped_json_secrets_and_keeps_private_text_at_diagnostic_and_trace_levels()
    {
        use sanikey_core::logging::{Component, DisplayControl, Level, Session};
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let temporary_root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../.Temp");
        fs::create_dir_all(&temporary_root).expect("directory temporanea di progetto");
        let source = r#"Ricerca privata: {"api\u005fkey":"SECRET-JSON-ESCAPED-SYNTHETIC","ordinary":"PRIVATE-JSON-VALUE-SYNTHETIC"}"#;

        for level in [Level::Diagnostic, Level::Trace] {
            let directory = temporary_root.join(format!(
                "sanikey-presentation-json-redaction-{}-{}-{level:?}",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).expect("directory log sintetica");
            let (session, path) =
                Session::create_in_directory_with_level(&directory, Component::Desktop, level)
                    .expect("sessione diagnostica sintetica");
            let mut failed = false;
            let displayed = session.scoped(|| {
                PresentedText::new(source, Classification::Unclassified).capture(
                    Some(&session),
                    &mut failed,
                    DisplayControl::new("synthetic_json_query"),
                )
            });
            assert_eq!(displayed, source, "il testo GUI deve restare invariato");
            assert!(!failed);
            session
                .finish(Component::Desktop)
                .expect("chiusura log sintetico");

            let contents = fs::read_to_string(path).expect("log diagnostico sintetico");
            assert!(contents.contains("PRIVATE-JSON-VALUE-SYNTHETIC"));
            assert!(!contents.contains("SECRET-JSON-ESCAPED-SYNTHETIC"));
            assert!(contents.contains("secret_omitted"));
            fs::remove_dir_all(directory).expect("rimozione directory log sintetica");
        }
    }
}
