use rocket_contrib::{json, json::JsonValue};
use slog::Logger;
use tree_sitter::{Language, Parser, TreeCursor};

use crate::{models::StatKind, stats::StatsError};

extern "C" {
    fn tree_sitter_cg() -> Language;
}

pub fn get_stats(_logger: &Logger, body: &str) -> Result<Vec<(StatKind, JsonValue)>, StatsError> {
    let mut parser = Parser::new();
    let language = unsafe { tree_sitter_cg() };
    parser
        .set_language(language)
        .map_err(|e| StatsError::Rlx(format!("Unable to load tree-sitter parser: {}", e)))?;
    let tree = parser
        .parse(body, None)
        .ok_or_else(|| StatsError::Rlx("Unable to parse rlx file".to_string()))?;

    let mut rules: usize = 0;

    let mut walker: TreeCursor = tree.root_node().walk();
    for child in tree.root_node().children(&mut walker) {
        if child.kind() == "rule" {
            rules += 1;
        }
    }

    Ok(vec![(StatKind::Rules, json!(rules))])
}
