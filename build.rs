use std::path::PathBuf;

fn main() {
    let ts_lexd_dir = PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-lexd/src");
    let ts_cg_dir = PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-cg/src");
    cc::Build::new()
        .includes(vec![&ts_lexd_dir, &ts_cg_dir])
        .files(vec![ts_lexd_dir.join("parser.c"), ts_cg_dir.join("parser.c")])
        .compile("tree-sitter");
}
