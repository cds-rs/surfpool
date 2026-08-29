use std::{env, fs, path::Path};

/// The spec documents that get a rustdoc variant.
const SPEC_DOCS: &[&str] = &["startup-lifecycle", "transaction-lifecycle"];

/// Produces the rustdoc variant of each spec document: every mermaid
/// region is replaced by its pre-rendered SVG from `src/diagrams/`, so
/// rustdoc shows the drawing while the source file keeps the editable
/// fence, which GitHub and editors render natively. The per-document
/// staleness tests hold the SVGs to their sources; this script only
/// splices.
fn main() {
    println!("cargo:rerun-if-changed=src/diagrams");
    for doc in SPEC_DOCS {
        println!("cargo:rerun-if-changed=src/{doc}.md");
        splice(doc);
    }
}

fn splice(doc: &str) {
    let source_path = format!("src/{doc}.md");
    let source =
        fs::read_to_string(&source_path).unwrap_or_else(|_| panic!("{source_path} should exist"));

    let mut output = String::new();
    let mut rest = source.as_str();
    loop {
        let Some(start) = rest.find("<!-- BEGIN MERMAID: ") else {
            output.push_str(rest);
            break;
        };
        let name_start = start + "<!-- BEGIN MERMAID: ".len();
        let name_end = rest[name_start..]
            .find(" -->")
            .expect("a mermaid marker name")
            + name_start;
        let name = &rest[name_start..name_end];
        let end_marker = format!("<!-- END MERMAID: {name} -->");
        let end = rest
            .find(&end_marker)
            .unwrap_or_else(|| panic!("no closing marker for mermaid region {name}"))
            + end_marker.len();

        output.push_str(&rest[..start]);
        let svg_path = format!("src/diagrams/{name}.svg");
        // A missing SVG keeps the editable fence in the rustdoc variant
        // instead of failing the build, so a fresh diagram can be
        // regenerated and then rendered; the staleness test is what
        // enforces the SVG's existence.
        match fs::read_to_string(&svg_path) {
            Ok(svg) => output.push_str(&svg),
            Err(_) => {
                println!("cargo:warning={svg_path} is missing; run the diagram render alias");
                output.push_str(&rest[start..end]);
            }
        }
        rest = &rest[end..];
    }

    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join(format!("{doc}.rustdoc.md"));
    fs::write(out, output).expect("write the rustdoc variant");
}
