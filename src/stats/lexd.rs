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
		let mut walker2: TreeCursor = child.walk();
		if child.kind() == "pattern_block" {
		   if child.child(0).unwrap().kind() == "pattern_start" {
			  patterns.insert("".to_string());
		   }
		   for line in child.children(&mut walker2) {
		   	   if line.kind() == "pattern_line" {
				   pat_entries += 1;
			   } else if line.kind() == "identifier" {
			   	  patterns.insert(body[line.byte_range()].to_string());
			   }
		   }
		} else if child.kind() == "lexicon_block" {
		   for line in child.children(&mut walker2) {
		   	   if line.kind() == "lexicon_line" {
			   	  lex_entries += 1;
			   } else if line.kind() == "identifier" {
			   	  lexicons.insert(body[line.byte_range()].to_string());
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