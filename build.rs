use std::path::PathBuf;

fn main() {
    let includes = vec![
        PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-lexd/src"),
        PathBuf::from(r"src/stats/tree-sitter-apertium/tree-sitter-cg/src"),
    ];
    cc::Build::new()
        .includes(&includes)
        .files(vec![includes[0].join("parser.c"), includes[1].join("parser.c")])
        .compile("tree-sitter");
}
