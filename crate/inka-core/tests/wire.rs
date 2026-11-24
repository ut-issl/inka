//! The page the Web UI receives, held against the commands it must agree
//! with and against the fixtures the front end is tested on.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::argument::load_argument;
use inka_core::eval::evaluate;
use inka_core::review::{QueueState, derive_queue};
use inka_core::wire::{Page, ReviewGroup, Snapshot, Status, page};

mod common;
use common::fixture_dir;

const FIXTURES: [&str; 2] = ["aocs", "dogfood"];

fn snapshot(name: &str) -> Snapshot {
    match page(&fixture_dir(name)) {
        Page::Argument(snapshot) => snapshot,
        Page::Rejected(rejection) => panic!("fixture `{name}` should present: {rejection:?}"),
    }
}

/// The page of a fixture project as the front end's tests read it.
fn fixture_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/fixtures")
        .join(format!("{name}.page.json"))
}

/// The front end is developed and tested against these files, so they are
/// what the server would send for the fixture projects, byte for byte.
/// Regenerate with `INKA_UPDATE_GENERATED=1 cargo test -p inka-core`.
#[test]
fn the_front_end_fixtures_are_the_pages_of_the_fixture_projects() {
    for name in FIXTURES {
        // Arrange
        let path = fixture_file(name);

        // Act
        let mut fresh =
            serde_json::to_string_pretty(&page(&fixture_dir(name))).expect("a page serializes");
        fresh.push('\n');

        // Assert
        if std::env::var_os("INKA_UPDATE_GENERATED").is_some() {
            fs::write(&path, &fresh).expect("the fixture is written");
        }
        let stored = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            stored == fresh,
            "{} is out of date; run `INKA_UPDATE_GENERATED=1 cargo test -p inka-core`",
            path.display()
        );
    }
}

#[test]
fn a_page_names_no_path_outside_the_project() {
    for name in FIXTURES {
        // Arrange & Act
        let snapshot = snapshot(name);

        // Assert: nothing machine-specific can reach a checked-in fixture.
        assert_eq!(snapshot.warnings, []);
    }
}

#[test]
fn statuses_agree_with_eval() {
    for name in FIXTURES {
        // Arrange
        let dir = fixture_dir(name);
        let argument = load_argument(&dir).expect("the fixture loads");
        let records = approvals::load_dir(&dir.join(APPROVALS_DIR)).expect("the records load");
        let evaluation = evaluate(
            &argument.view,
            &argument.manifest.phases,
            &records,
            &argument.evidence,
        )
        .expect("the fixture evaluates");

        // Act
        let snapshot = snapshot(name);

        // Assert
        assert_eq!(
            snapshot.established as usize,
            evaluation.established_count()
        );
        assert_eq!(snapshot.total as usize, evaluation.total());
        for (node, verdict) in snapshot.nodes.iter().zip(&evaluation.nodes) {
            assert_eq!(node.id, verdict.id);
            assert_eq!(
                matches!(node.status, Status::Established),
                verdict.established
            );
        }
        let blamed: Vec<&str> = snapshot
            .nodes
            .iter()
            .filter(|node| matches!(node.status, Status::Blamed { .. }))
            .map(|node| node.id.as_str())
            .collect();
        let expected: Vec<&str> = evaluation
            .blame
            .iter()
            .map(|blame| blame.node.as_str())
            .collect();
        assert_eq!(blamed, expected);
    }
}

#[test]
fn the_queue_agrees_with_review() {
    for name in FIXTURES {
        // Arrange
        let dir = fixture_dir(name);
        let argument = load_argument(&dir).expect("the fixture loads");
        let records = approvals::load_dir(&dir.join(APPROVALS_DIR)).expect("the records load");
        let queue = derive_queue(
            &argument.view,
            &argument.manifest.phases,
            &records,
            &argument.evidence,
        )
        .expect("the fixture's queue derives");
        let of = |wanted: fn(&QueueState) -> bool| -> Vec<&str> {
            queue
                .items
                .iter()
                .filter(|item| wanted(&item.state))
                .map(|item| item.id.as_str())
                .collect()
        };

        // Act
        let snapshot = snapshot(name);

        // Assert: the walk is the interactive session's, the groups are
        // `--list`'s sections.
        assert_eq!(
            snapshot.queue.walk,
            of(|state| !matches!(state, QueueState::Declined { .. }))
        );
        for group in &snapshot.queue.groups {
            let expected: Vec<&str> = match group.state {
                ReviewGroup::Unapproved => of(|state| matches!(state, QueueState::Missing)),
                ReviewGroup::Stale => of(|state| matches!(state, QueueState::Stale { .. })),
                ReviewGroup::Declined => of(|state| matches!(state, QueueState::Declined { .. })),
                ReviewGroup::Blocked => queue.blocked.iter().map(|item| item.id.as_str()).collect(),
            };
            assert_eq!(group.targets, expected);
            assert_ne!(group.targets.len(), 0);
        }
        let listed: usize = snapshot
            .queue
            .groups
            .iter()
            .map(|group| group.targets.len())
            .sum();
        assert_eq!(listed, queue.items.len() + queue.blocked.len());
    }
}

#[test]
fn every_node_is_ordered_once_after_its_parents() {
    for name in FIXTURES {
        // Arrange & Act
        let snapshot = snapshot(name);

        // Assert
        assert_eq!(snapshot.order.len(), snapshot.nodes.len());
        let position = |id: &str| {
            snapshot
                .order
                .iter()
                .position(|ordered| ordered == id)
                .expect("every node is ordered")
        };
        for node in &snapshot.nodes {
            for parent in &node.parents {
                assert!(position(parent) < position(&node.id));
            }
        }
        let grouped: usize = snapshot
            .namespaces
            .iter()
            .map(|group| group.nodes.len())
            .sum();
        assert_eq!(grouped, snapshot.nodes.len());
    }
}
