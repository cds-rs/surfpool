//! The document checks: `transaction-lifecycle.md` claims its table
//! and diagram are generated from the spec, and these tests keep that
//! claim true.

use surfpool_spec_harness::SpecDoc;

use super::{spec, *};

/// The spec document beside this module, with the cargo aliases that
/// regenerate it.
fn spec_doc() -> SpecDoc {
    SpecDoc {
        path: concat!(env!("CARGO_MANIFEST_DIR"), "/src/transaction-lifecycle.md"),
        diagrams_dir: concat!(env!("CARGO_MANIFEST_DIR"), "/src/diagrams"),
        update_alias: "surfpool-update-transaction-spec",
        render_alias: "surfpool-render-transaction-diagrams",
    }
}

/// The document claims its blocks are generated; holding every block
/// equal to a fresh render is what keeps that claim true.
#[test]
fn the_document_matches_the_spec() {
    spec_doc().assert_blocks_current(&generated_blocks());
}

/// Ignored so a plain test run never writes to the source tree;
/// `cargo surfpool-update-transaction-spec` runs it explicitly.
#[test]
#[ignore = "writes transaction-lifecycle.md; run via cargo surfpool-update-transaction-spec"]
fn regenerate_the_transaction_spec_tables() {
    spec_doc().regenerate(&generated_blocks());
}

#[test]
fn the_diagrams_match_their_renderings() {
    spec_doc().assert_diagrams_current();
}

/// Ignored so a plain test run never needs the mermaid CLI;
/// `cargo surfpool-render-transaction-diagrams` runs it explicitly.
#[test]
#[ignore = "renders SVGs with mmdc; run via cargo surfpool-render-transaction-diagrams"]
fn render_the_transaction_diagrams() {
    spec_doc().render_diagrams();
}

/// Every generated block, named by its marker in the document. All
/// render from the spec's table, so the document cannot say something
/// the conformance sweep does not hold the machine to.
fn generated_blocks() -> Vec<(&'static str, String)> {
    vec![
        ("diagram", render_diagram()),
        ("transitions", render_transitions()),
        ("links", render_links()),
    ]
}

/// Proves the linked item exists and stringifies the same tokens, so a
/// rename breaks the build here and regeneration follows: the target of
/// a generated link cannot silently drift.
macro_rules! link_target {
    ($ty:ident :: $variant:ident) => {{
        let _ = |value: &$ty| matches!(value, $ty::$variant { .. });
        concat!(stringify!($ty), "::", stringify!($variant))
    }};
}

/// A reference-style link whose label is the target path itself:
/// `[display][Type::Variant]`.
fn reference(display: &str, target: &'static str) -> String {
    format!("[{display}][{target}]")
}

fn state_target(state: TransactionLifecycleState) -> &'static str {
    use TransactionLifecycleState as S;
    match state {
        S::Unknown => link_target!(TransactionLifecycleState::Unknown),
        S::Admitted => link_target!(TransactionLifecycleState::Admitted),
        S::Rejected => link_target!(TransactionLifecycleState::Rejected),
        S::Processed => link_target!(TransactionLifecycleState::Processed),
        S::Confirmed => link_target!(TransactionLifecycleState::Confirmed),
        S::Finalized => link_target!(TransactionLifecycleState::Finalized),
        S::Expired => link_target!(TransactionLifecycleState::Expired),
    }
}

fn transition_target(transition: TransactionTransition) -> &'static str {
    use TransactionTransition as T;
    match transition {
        T::Admit => link_target!(TransactionTransition::Admit),
        T::Reject => link_target!(TransactionTransition::Reject),
        T::Execute => link_target!(TransactionTransition::Execute),
        T::Expire => link_target!(TransactionTransition::Expire),
        T::Confirm => link_target!(TransactionTransition::Confirm),
        T::Finalize => link_target!(TransactionTransition::Finalize),
    }
}

fn transition_name(transition: TransactionTransition) -> &'static str {
    use TransactionTransition as T;
    match transition {
        T::Admit => "Admit",
        T::Reject => "Reject",
        T::Execute => "Execute",
        T::Expire => "Expire",
        T::Confirm => "Confirm",
        T::Finalize => "Finalize",
    }
}

/// The accepting rows of the spec's table, one row per cell, with the
/// policy column verbatim; the refusals are covered by the authored
/// property line, and `the_table_is_total` is what makes that line
/// true.
fn render_transitions() -> String {
    let rows: Vec<[String; 4]> = spec::TABLE
        .iter()
        .filter_map(|row| {
            let spec::Next::To(next) = row.next else {
                return None;
            };
            Some([
                reference(spec::state_name(row.state), state_target(row.state)),
                reference(transition_name(row.transition), transition_target(row.transition)),
                reference(spec::state_name(next), state_target(next)),
                if row.policy.is_empty() {
                    "unconditional".to_string()
                } else {
                    row.policy.to_string()
                },
            ])
        })
        .collect();
    render_table(["State", "Transition", "New state", "Edge policy"], &rows)
}

/// The one-line annotation each state carries in the diagram.
fn state_note(state: TransactionLifecycleState) -> &'static str {
    use TransactionLifecycleState as S;
    match state {
        S::Unknown => "arrived, no admission decision yet",
        S::Admitted => "accepted, queued for execution",
        S::Rejected => "refused before the ledger; reported to the submitter",
        S::Processed => "on the ledger; an execution error is payload",
        S::Confirmed => "included in a confirmed block",
        S::Finalized => "past the finalization threshold",
        S::Expired => "aged out in the queue; the signature never resolves",
    }
}

/// The state diagram, drawn straight off the table: one edge per
/// accepting cell, an end marker per terminal state (a state whose
/// every cell refuses).
fn render_diagram() -> String {
    let mut out = String::from(
        "<!-- BEGIN MERMAID: transaction-lifecycle -->\n```mermaid\nstateDiagram-v2\n    [*] --> Unknown\n",
    );
    for state in spec::STATES {
        out.push_str(&format!(
            "    {name} : {name}<br/>{note}\n",
            name = spec::state_name(state),
            note = state_note(state)
        ));
    }
    for row in spec::TABLE {
        if let spec::Next::To(next) = row.next {
            out.push_str(&format!(
                "    {} --> {} : {}\n",
                spec::state_name(row.state),
                spec::state_name(next),
                transition_name(row.transition)
            ));
        }
    }
    for state in spec::STATES {
        let terminal = spec::TABLE
            .iter()
            .all(|row| row.state != state || row.next == spec::Next::Refuse);
        if terminal {
            out.push_str(&format!("    {} --> [*]\n", spec::state_name(state)));
        }
    }
    out.push_str("```\n<!-- END MERMAID: transaction-lifecycle -->\n");
    out
}

/// The reference-link definitions for every target the tables emit.
fn render_links() -> String {
    let mut targets: Vec<&'static str> = spec::STATES.map(state_target).to_vec();
    targets.extend(TransactionTransition::ALL.map(transition_target));
    targets.sort_unstable();
    targets.dedup();
    targets
        .into_iter()
        .map(|target| format!("[{target}]: {target}\n"))
        .collect()
}

fn render_table(headers: [&str; 4], rows: &[[String; 4]]) -> String {
    let mut widths = headers.map(str::len);
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(cell.len());
        }
    }
    let mut out = String::new();
    let render_row = |cells: [&str; 4]| {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(column, cell)| format!("{cell:width$}", width = widths[column]))
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    out.push_str(&render_row(headers));
    out.push_str(&format!(
        "|{}|\n",
        widths
            .map(|width| "-".repeat(width + 2))
            .to_vec()
            .join("|")
    ));
    for row in rows {
        out.push_str(&render_row([
            row[0].as_str(),
            row[1].as_str(),
            row[2].as_str(),
            row[3].as_str(),
        ]));
    }
    out
}
