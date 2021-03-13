use std::{
    collections::{HashSet}
};

use tree_sitter::{Parser, Language, TreeCursor};

use rocket_contrib::{json, json::JsonValue};

use slog::{warn, Logger};

use crate::{models::StatKind, stats::StatsError};

extern "C" { fn tree_sitter_lexd() -> Language; }

pub fn get_stats(logger: &Logger, body: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_lexd() };
    parser.set_language(language).unwrap();
    let tree = parser.parse(body, None).unwrap();
    let mut lexicons: HashSet<String> = HashSet::new();
	let mut patterns: HashSet<String> = HashSet::new();
	let mut lex_entries: usize = 0;
	let mut pat_entries: usize = 0;

	let mut walker: TreeCursor = tree.root_node().walk();

    for child in tree.root_node().children(&mut walker) {
		if child.kind() == "pattern_block" {
		   //patterns++;
		   pat_entries += 1;
		} else if child.kind() == "lexicon_block" {
		   //lexicons++;
		   lex_entries += 1;
		}
	}

	Ok(vec![
		(StatKind::Lexicons, json!(lexicons.len())),
		(StatKind::LexiconEntries, json!(lex_entries)),
		(StatKind::Patterns, json!(patterns.len())),
		(StatKind::PatternEntries, json!(pat_entries)),
	])
}