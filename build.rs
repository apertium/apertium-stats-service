use cc;
use std::path::PathBuf;

fn main() {
    let ts_dir: PathBuf = ["src", "stats", "tree-sitter-apertium", "tree-sitter-lexd", "src"]
        .iter()
        .collect();
    cc::Build::new()
        .include(&ts_dir)
        .file(ts_dir.join("parser.c"))
        .compile("tree-sitter-lexd");
}
