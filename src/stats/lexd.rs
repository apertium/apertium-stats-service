use std::collections::HashSet;

use rocket_contrib::{json, json::JsonValue};
use slog::Logger;
use tree_sitter::{Language, Parser, TreeCursor};

use crate::{models::StatKind, stats::StatsError};

extern "C" {
    fn tree_sitter_lexd() -> Language;
}

pub fn get_stats(_logger: &Logger, body: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_lexd() };
    parser
        .set_language(language)
        .map_err(|e| StatsError::Lexd(format!("Unable to load tree-sitter parser: {}", e)))?;
    let tree = parser
        .parse(body, None)
        .ok_or_else(|| StatsError::Lexd("Unable to parse lexd file".to_string()))?;
    let mut lexicons: HashSet<&str> = HashSet::new();
    let mut patterns: HashSet<&str> = HashSet::new();
    let mut lex_entries: usize = 0;
    let mut pat_entries: usize = 0;

    let mut walker: TreeCursor = tree.root_node().walk();

    for child in tree.root_node().children(&mut walker) {
        let mut walker2: TreeCursor = child.walk();
        if child.kind() == "pattern_block" {
            if child.child(0).unwrap().kind() == "pattern_start" {
                patterns.insert("");
            }
            for line in child.children(&mut walker2) {
                if line.kind() == "pattern_line" {
                    pat_entries += 1;
                } else if line.kind() == "identifier" {
                    patterns.insert(&body[line.byte_range()]);
                }
            }
        } else if child.kind() == "lexicon_block" {
            for line in child.children(&mut walker2) {
                if line.kind() == "lexicon_line" {
                    lex_entries += 1;
                } else if line.kind() == "identifier" {
                    lexicons.insert(&body[line.byte_range()]);
                }
            }
        }
    }

    Ok(vec![
        (StatKind::Lexicons, json!(lexicons.len())),
        (StatKind::LexiconEntries, json!(lex_entries)),
        (StatKind::Patterns, json!(patterns.len())),
        (StatKind::PatternEntries, json!(pat_entries)),
    ])
}
