//! The `inka export` command: the argument as cytoscape.js `elements` JSON.
//!
//! This is inka's machine interface to the argument graph. The built-in viewer
//! is one consumer of it, not a privileged one: anything that can read JSON can
//! read the structure, so the UI is never the only way to see an argument.
//!
//! The shape lives here rather than in the shared view, because it is one
//! command's serialization and not part of the model. What the view fixes —
//! which nodes exist, their text, and every ordering — this module only
//! transcribes, so the output is deterministic for the same reason the view is.
//!
//! ```json
//! [
//!   { "data": { "id": "detumble_ok", "mode": "structure", "namespace": "safety",
//!               "description": "…", "tags": [], "root": false } },
//!   { "data": { "id": "detumble_ok->mtq_authority_ok",
//!               "source": "detumble_ok", "target": "mtq_authority_ok" } }
//! ]
//! ```
//!
//! Two properties consumers rely on: node elements come in the view's sorted
//! block-ID order, and edge elements come parent by parent in each parent's
//! declaration order. There is no `label` field — a node's label is its id, so
//! a renderer reads `id` and the two cannot drift apart.

use std::io::{self, Write};
use std::path::Path;

use inka_core::view::{ArgumentView, Deadline, Evidence, EvidenceKind, Mode, ViewEdge, ViewNode};
use serde::Serialize;

use crate::check::Outcome;
use crate::load::load_view;

/// One cytoscape element: a node or an edge, each wrapped in `data`.
///
/// Untagged, because cytoscape tells the two apart by the presence of
/// `source`/`target` rather than by a discriminator field.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Element<'v> {
    Node { data: NodeData<'v> },
    Edge { data: EdgeData<'v> },
}

#[derive(Debug, Serialize)]
struct NodeData<'v> {
    id: &'v str,
    mode: Mode,
    namespace: &'v str,
    description: &'v str,
    tags: &'v [String],
    /// True on an objective the argument hangs from.
    root: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence: Option<EvidenceData<'v>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline: Option<DeadlineData<'v>>,
}

#[derive(Debug, Serialize)]
struct EvidenceData<'v> {
    kind: &'static str,
    target: &'v str,
}

#[derive(Debug, Serialize)]
struct DeadlineData<'v> {
    until: &'v str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'v str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    promote: Option<Mode>,
}

#[derive(Debug, Serialize)]
struct EdgeData<'v> {
    /// `parent->child`, which is unique because a pair appears at most once.
    id: String,
    source: &'v str,
    target: &'v str,
}

/// Serializes a view as cytoscape `elements` JSON, ending in a newline.
///
/// Pretty-printed on purpose: the output is read by people as often as by
/// programs, and it is checked into fixtures and snapshots where a line diff is
/// worth more than a few saved bytes.
///
/// # Errors
///
/// Propagates a serialization failure.
pub fn to_json(view: &ArgumentView) -> serde_json::Result<String> {
    let elements: Vec<Element<'_>> = view
        .nodes
        .iter()
        .map(|node| node_element(view, node))
        .chain(view.edges.iter().map(edge_element))
        .collect();
    let mut json = serde_json::to_string_pretty(&elements)?;
    json.push('\n');
    Ok(json)
}

fn node_element<'v>(view: &'v ArgumentView, node: &'v ViewNode) -> Element<'v> {
    Element::Node {
        data: NodeData {
            id: &node.id,
            mode: node.mode,
            namespace: &node.namespace,
            description: &node.description,
            tags: &node.tags,
            root: view.roots.iter().any(|root| root == &node.id),
            evidence: node.evidence.as_ref().map(evidence_data),
            deadline: node.deadline.as_ref().map(deadline_data),
        },
    }
}

fn edge_element(edge: &ViewEdge) -> Element<'_> {
    Element::Edge {
        data: EdgeData {
            id: format!("{}->{}", edge.parent, edge.child),
            source: &edge.parent,
            target: &edge.child,
        },
    }
}

fn evidence_data(evidence: &Evidence) -> EvidenceData<'_> {
    EvidenceData {
        // The scheme is spelled out here rather than serialized from the enum:
        // it is part of the wire contract, not of the model's spelling.
        kind: match evidence.kind {
            EvidenceKind::Veriq => "veriq",
            EvidenceKind::Test => "test",
        },
        target: &evidence.target,
    }
}

fn deadline_data(deadline: &Deadline) -> DeadlineData<'_> {
    DeadlineData {
        until: &deadline.until,
        reason: deadline.reason.as_deref(),
        promote: deadline.promote,
    }
}

/// Runs `inka export` against `project_dir`, writing JSON to `out` and any
/// findings to `err`.
///
/// Nothing is written to `out` unless the whole argument projected: a partial
/// export would be indistinguishable from a smaller argument.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out` or `err`.
pub fn run(
    project_dir: &Path,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    let Some(view) = load_view(project_dir, err, color)? else {
        return Ok(Outcome::Failed);
    };
    let json = to_json(&view).map_err(io::Error::other)?;
    out.write_all(json.as_bytes())?;
    Ok(Outcome::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::Value;

    /// A hand-built view: the export transcribes a projection, so the tests
    /// state the projection directly rather than parsing a project to reach it.
    fn view() -> ArgumentView {
        ArgumentView {
            nodes: vec![
                ViewNode {
                    id: "root".to_owned(),
                    mode: Mode::Objective,
                    namespace: "objective".to_owned(),
                    description: "the root claim".to_owned(),
                    tags: vec!["top".to_owned()],
                    evidence: None,
                    deadline: None,
                    children: vec!["verified".to_owned(), "assumed".to_owned()],
                },
                ViewNode {
                    id: "verified".to_owned(),
                    mode: Mode::Bind,
                    namespace: "safety".to_owned(),
                    description: "checked by a solver".to_owned(),
                    tags: Vec::new(),
                    evidence: Some(Evidence {
                        kind: EvidenceKind::Veriq,
                        target: "aocs.verify_detumble".to_owned(),
                    }),
                    deadline: None,
                    children: Vec::new(),
                },
                ViewNode {
                    id: "assumed".to_owned(),
                    mode: Mode::Fiat,
                    namespace: "safety".to_owned(),
                    description: "taken as given".to_owned(),
                    tags: Vec::new(),
                    evidence: None,
                    deadline: Some(Deadline {
                        until: "FM".to_owned(),
                        reason: Some("measured later".to_owned()),
                        promote: Some(Mode::Bind),
                    }),
                    children: Vec::new(),
                },
            ],
            edges: vec![
                ViewEdge {
                    parent: "root".to_owned(),
                    child: "verified".to_owned(),
                },
                ViewEdge {
                    parent: "root".to_owned(),
                    child: "assumed".to_owned(),
                },
            ],
            roots: vec!["root".to_owned()],
        }
    }

    fn elements() -> Vec<Value> {
        let json = to_json(&view()).expect("the view should serialize");
        serde_json::from_str(&json).expect("the export should be valid JSON")
    }

    fn data_of(element: &Value) -> &Value {
        element.get("data").expect("every element wraps its data")
    }

    #[test]
    fn emits_one_element_per_node_and_per_edge() {
        // Arrange & Act
        let elements = elements();

        // Assert
        assert_eq!(elements.len(), 5);
        assert_eq!(
            elements
                .iter()
                .filter(|element| data_of(element).get("source").is_some())
                .count(),
            2
        );
    }

    #[test]
    fn keeps_the_views_order_so_the_output_is_deterministic() {
        let elements = elements();
        let ids: Vec<&str> = elements
            .iter()
            .map(|element| data_of(element)["id"].as_str().expect("ids are strings"))
            .collect();

        assert_eq!(
            ids,
            [
                "root",
                "verified",
                "assumed",
                "root->verified",
                "root->assumed"
            ]
        );
    }

    #[test]
    fn writes_the_same_bytes_for_the_same_view() {
        let first = to_json(&view()).expect("the view should serialize");
        let second = to_json(&view()).expect("the view should serialize");

        assert_eq!(first, second);
    }

    #[test]
    fn spells_each_discharge_mode_out() {
        let elements = elements();
        let modes: Vec<&str> = elements
            .iter()
            .filter_map(|element| data_of(element).get("mode"))
            .map(|mode| mode.as_str().expect("modes are strings"))
            .collect();

        assert_eq!(modes, ["objective", "bind", "fiat"]);
    }

    #[test]
    fn marks_the_objective_roots_and_nothing_else() {
        let elements = elements();
        let roots: Vec<&str> = elements
            .iter()
            .map(data_of)
            .filter(|data| data.get("root") == Some(&Value::Bool(true)))
            .map(|data| data["id"].as_str().expect("ids are strings"))
            .collect();

        assert_eq!(roots, ["root"]);
    }

    #[test]
    fn carries_the_evidence_target_of_a_bind() {
        let elements = elements();
        let bind = data_of(&elements[1]);

        assert_eq!(bind["evidence"]["kind"], "veriq");
        assert_eq!(bind["evidence"]["target"], "aocs.verify_detumble");
    }

    #[test]
    fn carries_a_deadline_with_its_rationale_and_promotion() {
        let elements = elements();
        let fiat = data_of(&elements[2]);

        assert_eq!(fiat["deadline"]["until"], "FM");
        assert_eq!(fiat["deadline"]["reason"], "measured later");
        assert_eq!(fiat["deadline"]["promote"], "bind");
    }

    #[test]
    fn omits_evidence_and_deadline_rather_than_emitting_nulls() {
        let elements = elements();
        let objective = data_of(&elements[0]);

        assert!(objective.get("evidence").is_none());
        assert!(objective.get("deadline").is_none());
        // A node that has neither still carries the fields every node has.
        assert_eq!(objective["tags"][0], "top");
        assert_eq!(objective["namespace"], "objective");
    }

    #[test]
    fn never_emits_a_label_because_the_label_is_the_id() {
        let elements = elements();

        assert!(
            elements
                .iter()
                .all(|element| data_of(element).get("label").is_none())
        );
    }

    #[test]
    fn names_an_edge_after_the_pair_it_connects() {
        let elements = elements();
        let edge = data_of(&elements[3]);

        assert_eq!(edge["id"], "root->verified");
        assert_eq!(edge["source"], "root");
        assert_eq!(edge["target"], "verified");
    }

    #[test]
    fn ends_the_document_with_a_newline() {
        let json = to_json(&view()).expect("the view should serialize");

        assert!(json.ends_with("]\n"));
    }
}
