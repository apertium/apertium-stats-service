use std::path::PathBuf;

fn main() {
    // tree-sitter-lexd
    let ts_lexd_dir = PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-lexd/src");
    cc::Build::new()
        .include(&ts_lexd_dir)
        .file(ts_lexd_dir.join("parser.c"))
        .compile("tree-sitter-lexd");

    // tree-sitter-cg
    let ts_cg_dir = PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-cg/src");
    cc::Build::new()
        .include(&ts_cg_dir)
        .file(ts_cg_dir.join("parser.c"))
        .compile("tree-sitter-cg");
}
