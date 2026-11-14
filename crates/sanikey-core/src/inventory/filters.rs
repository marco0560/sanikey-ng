//! Precedenza e matching case-insensitive dei pattern d'ingestione.

use crate::config::IngestionConfig;
use crate::logging::{Component, TraceField, TraceValue, redact_secret_assignments, trace_current};
use serde_json::json;

pub(super) struct IngestionMatcher {
    include: Vec<GlobPattern>,
    exclude: Vec<GlobPattern>,
}

/// Compila e applica le regole include ed exclude d'ingestione.
impl IngestionMatcher {
    pub(super) fn new(ingestion: &IngestionConfig) -> Self {
        let trace = trace_current(
            Component::Core,
            "inventory::filters::IngestionMatcher::new",
            || {
                vec![
                    TraceField::new(
                        "include_patterns",
                        TraceValue::Data(json!(
                            ingestion
                                .include_patterns
                                .iter()
                                .map(|value| redact_secret_assignments(value))
                                .collect::<Vec<_>>()
                        )),
                    ),
                    TraceField::new(
                        "exclude_patterns",
                        TraceValue::Data(json!(
                            ingestion
                                .exclude_patterns
                                .iter()
                                .map(|value| redact_secret_assignments(value))
                                .collect::<Vec<_>>()
                        )),
                    ),
                ]
            },
        );
        let matcher = Self {
            include: compile_patterns(&ingestion.include_patterns),
            exclude: compile_patterns(&ingestion.exclude_patterns),
        };
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!({
                "include": matcher.include.iter().map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0))).collect::<Vec<_>>(),
                "exclude": matcher.exclude.iter().map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0))).collect::<Vec<_>>(),
            })));
        }
        matcher
    }

    pub(super) fn is_excluded(&self, relative_path: &str) -> bool {
        let trace = trace_current(
            Component::Core,
            "inventory::filters::IngestionMatcher::is_excluded",
            || {
                vec![
                    TraceField::new(
                        "matcher",
                        TraceValue::Data(json!({
                            "include": self.include.iter().map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0))).collect::<Vec<_>>(),
                            "exclude": self.exclude.iter().map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0))).collect::<Vec<_>>(),
                        })),
                    ),
                    TraceField::new(
                        "relative_path",
                        TraceValue::Data(json!(redact_secret_assignments(relative_path))),
                    ),
                ]
            },
        );
        let excluded = if any_match(&self.include, relative_path) {
            false
        } else {
            any_match(&self.exclude, relative_path)
        };
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(excluded)));
        }
        excluded
    }
}

#[derive(Clone, Debug)]
struct GlobPattern(Vec<PatternToken>);

#[derive(Clone, Debug)]
enum PatternToken {
    Literal(char),
    Any,
    Many,
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
}

#[derive(Clone, Debug)]
enum ClassItem {
    Literal(char),
    Range(char, char),
}

/// Analizza e confronta un pattern glob case-insensitive.
impl GlobPattern {
    fn parse(pattern: &str) -> Self {
        let trace = trace_current(
            Component::Core,
            "inventory::filters::GlobPattern::parse",
            || {
                vec![TraceField::new(
                    "pattern",
                    TraceValue::Data(json!(redact_secret_assignments(pattern))),
                )]
            },
        );
        let characters: Vec<char> = pattern.to_lowercase().chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < characters.len() {
            match characters[index] {
                '*' => {
                    if !matches!(tokens.last(), Some(PatternToken::Many)) {
                        tokens.push(PatternToken::Many);
                    }
                    index += 1;
                }
                '?' => {
                    tokens.push(PatternToken::Any);
                    index += 1;
                }
                '[' => match parse_class(&characters, index) {
                    Some((token, next_index)) => {
                        tokens.push(token);
                        index = next_index;
                    }
                    None => {
                        tokens.push(PatternToken::Literal('['));
                        index += 1;
                    }
                },
                literal => {
                    tokens.push(PatternToken::Literal(literal));
                    index += 1;
                }
            }
        }
        let parsed = Self(tokens);
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(redact_secret_assignments(
                &format!("{:?}", parsed.0)
            ))));
        }
        parsed
    }

    fn is_match(&self, value: &str) -> bool {
        let trace = trace_current(
            Component::Core,
            "inventory::filters::GlobPattern::is_match",
            || {
                vec![
                    TraceField::new(
                        "pattern",
                        TraceValue::Data(json!(redact_secret_assignments(&format!(
                            "{:?}",
                            self.0
                        )))),
                    ),
                    TraceField::new(
                        "value",
                        TraceValue::Data(json!(redact_secret_assignments(value))),
                    ),
                ]
            },
        );
        let characters: Vec<char> = value.to_lowercase().chars().collect();
        let mut previous = vec![false; characters.len() + 1];
        previous[0] = true;
        for token in &self.0 {
            let mut next = vec![false; characters.len() + 1];
            match token {
                PatternToken::Many => {
                    let mut reachable = false;
                    for index in 0..=characters.len() {
                        reachable |= previous[index];
                        next[index] = reachable;
                    }
                }
                PatternToken::Any => {
                    next[1..].copy_from_slice(&previous[..characters.len()]);
                }
                PatternToken::Literal(expected) => {
                    for (index, actual) in characters.iter().enumerate() {
                        next[index + 1] = previous[index] && actual == expected;
                    }
                }
                PatternToken::Class { negated, items } => {
                    for (index, actual) in characters.iter().enumerate() {
                        let contained = items.iter().any(|item| match item {
                            ClassItem::Literal(literal) => actual == literal,
                            ClassItem::Range(start, end) => start <= actual && actual <= end,
                        });
                        next[index + 1] = previous[index] && (contained != *negated);
                    }
                }
            }
            previous = next;
        }
        let matched = previous[characters.len()];
        if let Some(trace) = trace {
            let _ = trace.finish(TraceValue::Data(json!(matched)));
        }
        matched
    }
}

fn compile_patterns(patterns: &[String]) -> Vec<GlobPattern> {
    let trace = trace_current(
        Component::Core,
        "inventory::filters::compile_patterns",
        || {
            vec![TraceField::new(
                "patterns",
                TraceValue::Data(json!(
                    patterns
                        .iter()
                        .map(|value| redact_secret_assignments(value))
                        .collect::<Vec<_>>()
                )),
            )]
        },
    );
    let mut compiled = Vec::new();
    for pattern in patterns {
        let normalized = pattern.to_lowercase();
        compiled.push(GlobPattern::parse(&normalized));
        if let Some(without_recursive_prefix) = normalized.strip_prefix("**/") {
            compiled.push(GlobPattern::parse(without_recursive_prefix));
        }
    }
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(
            compiled
                .iter()
                .map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0)))
                .collect::<Vec<_>>()
        )));
    }
    compiled
}

fn any_match(patterns: &[GlobPattern], relative_path: &str) -> bool {
    let trace = trace_current(Component::Core, "inventory::filters::any_match", || {
        vec![
            TraceField::new(
                "patterns",
                TraceValue::Data(json!(
                    patterns
                        .iter()
                        .map(|pattern| redact_secret_assignments(&format!("{:?}", pattern.0)))
                        .collect::<Vec<_>>()
                )),
            ),
            TraceField::new(
                "relative_path",
                TraceValue::Data(json!(redact_secret_assignments(relative_path))),
            ),
        ]
    });
    let basename = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let matched = patterns
        .iter()
        .any(|pattern| pattern.is_match(relative_path) || pattern.is_match(basename));
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(matched)));
    }
    matched
}

fn parse_class(characters: &[char], start: usize) -> Option<(PatternToken, usize)> {
    let trace = trace_current(Component::Core, "inventory::filters::parse_class", || {
        vec![
            TraceField::new(
                "characters",
                TraceValue::Data(json!(redact_secret_assignments(
                    &characters.iter().collect::<String>()
                ))),
            ),
            TraceField::new("start", TraceValue::Data(json!(start))),
        ]
    });
    let mut cursor = start + 1;
    let negated = characters.get(cursor) == Some(&'!');
    if negated {
        cursor += 1;
    }
    let content_start = cursor;
    if characters.get(cursor) == Some(&']') {
        cursor += 1;
    }
    while cursor < characters.len() && characters[cursor] != ']' {
        cursor += 1;
    }
    let result = if cursor == characters.len() || cursor == content_start {
        None
    } else {
        let content = &characters[content_start..cursor];
        let mut items = Vec::new();
        let mut index = 0;
        while index < content.len() {
            if index + 2 < content.len() && content[index + 1] == '-' {
                items.push(ClassItem::Range(content[index], content[index + 2]));
                index += 3;
            } else {
                items.push(ClassItem::Literal(content[index]));
                index += 1;
            }
        }
        Some((PatternToken::Class { negated, items }, cursor + 1))
    };
    if let Some(trace) = trace {
        let _ = trace.finish(TraceValue::Data(json!(redact_secret_assignments(
            &format!("{result:?}")
        ))));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::IngestionMatcher;
    use crate::config::IngestionConfig;
    use crate::logging::{Component, Level, Session};
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_directory() -> std::path::PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "sanikey-filter-trace-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn include_patterns_recover_excluded_files_and_matching_stays_case_insensitive() {
        let config = IngestionConfig {
            exclude_patterns: vec!["**/Help/**".into(), "*.tmp".into()],
            include_patterns: vec!["**/Help/keep.txt".into()],
        };
        let matcher = IngestionMatcher::new(&config);

        assert!(matcher.is_excluded("person/Help/remove.txt"));
        assert!(!matcher.is_excluded("person/Help/keep.txt"));
        assert!(matcher.is_excluded("REPORT.TMP"));
        assert!(!matcher.is_excluded("person/report.txt"));
    }

    #[test]
    fn trace_records_representable_filter_inputs_and_results() {
        let directory = test_directory();
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione sintetica");
        let config = IngestionConfig {
            exclude_patterns: vec!["*.private".into(), "Report[0-9].txt".into()],
            include_patterns: Vec::new(),
        };
        session.scoped(|| {
            let matcher = IngestionMatcher::new(&config);
            assert!(matcher.is_excluded("synthetic.private"));
            assert!(matcher.is_excluded("report7.TXT"));
        });
        drop(session);

        let contents = fs::read_to_string(&path).expect("log sintetico");
        assert!(contents.contains("inventory::filters::IngestionMatcher::new"));
        assert!(contents.contains("inventory::filters::compile_patterns"));
        assert!(contents.contains("inventory::filters::GlobPattern::parse"));
        assert!(contents.contains("inventory::filters::parse_class"));
        assert!(contents.contains("inventory::filters::IngestionMatcher::is_excluded"));
        assert!(contents.contains("inventory::filters::any_match"));
        assert!(contents.contains("inventory::filters::GlobPattern::is_match"));
        assert!(contents.contains("synthetic.private"));
        assert!(contents.contains("Class {"));
        assert!(!contents.contains("unsupported_type"));
        assert!(!contents.contains("secret_omitted"));
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }

    #[test]
    fn trace_redacts_secret_like_filter_values_and_preserves_other_patterns() {
        let directory = test_directory();
        let (session, path) =
            Session::create_in_directory_with_level(&directory, Component::Core, Level::Trace)
                .expect("sessione sintetica");
        let config = IngestionConfig {
            exclude_patterns: vec!["password = synthetic-filter-secret".into()],
            include_patterns: vec!["ordinary-filter-value".into()],
        };
        session.scoped(|| {
            let matcher = IngestionMatcher::new(&config);
            let _ = matcher.is_excluded("ordinary-filter-value");
        });
        drop(session);

        let contents = fs::read_to_string(&path).expect("log sintetico");
        assert!(contents.contains("ordinary-filter-value"));
        assert!(contents.contains("secret_omitted"));
        assert!(!contents.contains("synthetic-filter-secret"));
        fs::remove_file(path).expect("rimozione log sintetico");
        fs::remove_dir(directory).expect("rimozione directory sintetica");
    }
}
