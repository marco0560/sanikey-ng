//! Precedenza e matching case-insensitive dei pattern d'ingestione.

use crate::config::IngestionConfig;

pub(super) struct IngestionMatcher {
    include: Vec<GlobPattern>,
    exclude: Vec<GlobPattern>,
}

impl IngestionMatcher {
    pub(super) fn new(ingestion: &IngestionConfig) -> Self {
        Self {
            include: compile_patterns(&ingestion.include_patterns),
            exclude: compile_patterns(&ingestion.exclude_patterns),
        }
    }

    pub(super) fn is_excluded(&self, relative_path: &str) -> bool {
        if any_match(&self.include, relative_path) {
            return false;
        }
        any_match(&self.exclude, relative_path)
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

impl GlobPattern {
    fn parse(pattern: &str) -> Self {
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
        Self(tokens)
    }

    fn is_match(&self, value: &str) -> bool {
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
        previous[characters.len()]
    }
}

fn compile_patterns(patterns: &[String]) -> Vec<GlobPattern> {
    let mut compiled = Vec::new();
    for pattern in patterns {
        let normalized = pattern.to_lowercase();
        compiled.push(GlobPattern::parse(&normalized));
        if let Some(without_recursive_prefix) = normalized.strip_prefix("**/") {
            compiled.push(GlobPattern::parse(without_recursive_prefix));
        }
    }
    compiled
}

fn any_match(patterns: &[GlobPattern], relative_path: &str) -> bool {
    let basename = relative_path.rsplit('/').next().unwrap_or(relative_path);
    patterns
        .iter()
        .any(|pattern| pattern.is_match(relative_path) || pattern.is_match(basename))
}

fn parse_class(characters: &[char], start: usize) -> Option<(PatternToken, usize)> {
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
    if cursor == characters.len() || cursor == content_start {
        return None;
    }
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
}
