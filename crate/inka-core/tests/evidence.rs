//! Producer-independent v2 vectors and local evidence resolution scenarios.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use inka_core::evidence::{
    Bundle, EvidenceConfig, EvidenceSet, MissingEvidence, SourceConfig, load_sources,
};

fn vectors() -> Vec<serde_json::Value> {
    [
        include_str!("../../../tests/golden/evidence/v2/contracts.json"),
        include_str!("../../../tests/golden/evidence/v2/automatic-catalog.json"),
    ]
    .into_iter()
    .flat_map(|text| serde_json::from_str::<Vec<serde_json::Value>>(text).unwrap())
    .collect()
}

fn passing() -> Bundle {
    Bundle::parse(&vectors()[0]["bundle"].to_string()).unwrap()
}

#[test]
fn independent_conformance_vectors_match_validation_and_outcomes() {
    for vector in vectors() {
        let actual = Bundle::parse(&vector["bundle"].to_string());
        assert_eq!(
            actual.is_ok(),
            vector["valid"].as_bool().unwrap(),
            "{}: {actual:?}",
            vector["name"]
        );
        if let Ok(bundle) = actual {
            let failures = bundle.targets[0].failures();
            let status = failures.first().map_or("pass".to_owned(), |failure| {
                failure
                    .result
                    .map_or("missing".to_owned(), |result| result.to_string())
            });
            assert_eq!(
                status,
                vector["status"].as_str().unwrap(),
                "{}",
                vector["name"]
            );
        }
    }
}

#[test]
fn duplicate_object_fields_are_rejected_before_they_can_be_overwritten() {
    let text = serde_json::to_string(&passing()).unwrap();
    assert!(Bundle::parse(&text.replacen("\"v\":2", "\"v\":1,\"v\":2", 1)).is_err());
    assert!(
        Bundle::parse(&text.replacen(
            "\"kind\":\"file\"",
            "\"kind\":\"function\",\"kind\":\"file\"",
            1
        ))
        .is_err()
    );
}

#[test]
fn content_normalization_is_crlf_only_and_selection_is_part_of_the_contract() {
    let bundle = passing();
    let original = &bundle.targets[0];
    let hash = original.hash(&bundle.source.id);
    for content in [
        original.content.replace('\n', "\r"),
        format!("{} ", original.content),
        format!("// note\n{}", original.content),
    ] {
        let mut changed = original.clone();
        changed.content = content;
        assert_ne!(hash, changed.hash(&bundle.source.id));
    }
    let mut changed = original.clone();
    changed.required.push("windows/works".into());
    assert_ne!(hash, changed.hash(&bundle.source.id));
    changed = original.clone();
    changed.profile = "file-source:v2".into();
    assert_ne!(hash, changed.hash(&bundle.source.id));
}

#[test]
fn alias_routes_to_identity_without_becoming_part_of_the_contract() {
    let bundle = passing();
    let mut set = EvidenceSet::default();
    set.insert(
        "upstream".into(),
        bundle.source.id.clone(),
        Some(bundle.clone()),
    )
    .unwrap();
    let (source, target) = set.resolve("upstream/tests/example.rs").unwrap();
    assert_eq!(
        target.hash(&source.id),
        bundle.targets[0].hash(&bundle.source.id)
    );
    assert!(matches!(
        set.resolve("upstream/absent"),
        Err(MissingEvidence::MissingTarget { .. })
    ));
    assert!(matches!(
        set.resolve("other/tests/example.rs"),
        Err(MissingEvidence::UnknownSource { .. })
    ));
    assert!(matches!(
        set.resolve("not-a-reference"),
        Err(MissingEvidence::InvalidReference { .. })
    ));
    assert!(
        set.insert("alias2".into(), bundle.source.id.clone(), Some(bundle))
            .is_err()
    );
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "inka-evidence-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn source_checkout_is_unnecessary_and_missing_corrupt_and_wrong_source_are_distinct() {
    let temp = Temp::new();
    let bundle = passing();
    let mut config = EvidenceConfig {
        sources: BTreeMap::from([(
            "upstream".into(),
            SourceConfig {
                id: bundle.source.id.clone(),
                file: "import.json".into(),
            },
        )]),
    };
    let set = load_sources(&temp.0, &config).unwrap();
    assert!(matches!(
        set.resolve("upstream/tests/example.rs"),
        Err(MissingEvidence::MissingBundle { .. })
    ));
    fs::write(temp.0.join("import.json"), "{ broken").unwrap();
    assert!(
        load_sources(&temp.0, &config)
            .unwrap_err()
            .to_string()
            .contains("import.json")
    );
    fs::write(
        temp.0.join("import.json"),
        serde_json::to_string(&bundle).unwrap(),
    )
    .unwrap();
    assert!(
        load_sources(&temp.0, &config)
            .unwrap()
            .resolve("upstream/tests/example.rs")
            .is_ok()
    );
    config.sources.get_mut("upstream").unwrap().id = "github:another/source".into();
    assert!(
        load_sources(&temp.0, &config)
            .unwrap_err()
            .to_string()
            .contains("source identity mismatch")
    );
}

#[test]
fn configuration_rejects_alias_and_source_collisions() {
    let config = r#"
[project]
name = "example"
[phases]
order = ["M0"]
current = "M0"
[values]
files = []
[evidence.sources.upstream]
id = "github:example/source"
file = "import.json"
"#;
    assert!(inka_core::manifest::parse_manifest(config).is_ok());
    assert!(
        inka_core::manifest::parse_manifest(&config.replace("sources.upstream", "sources.123"))
            .is_err()
    );
    let duplicate = format!(
        "{config}\n[evidence.sources.second]\nid = \"github:example/source\"\nfile = \"other.json\"\n"
    );
    assert!(inka_core::manifest::parse_manifest(&duplicate).is_err());
}

#[test]
fn automatic_catalog_preserves_only_unchanged_approval_contracts() {
    let bundles: BTreeMap<String, Bundle> = vectors()
        .into_iter()
        .filter(|vector| vector["valid"] == true)
        .map(|vector| {
            (
                vector["name"].as_str().unwrap().to_owned(),
                Bundle::parse(&vector["bundle"].to_string()).unwrap(),
            )
        })
        .collect();
    let base = &bundles["parameterized-function"];
    let hash = base.targets[0].hash(&base.source.id);
    for name in ["case-order", "parameter-missing"] {
        let bundle = &bundles[name];
        assert_eq!(hash, bundle.targets[0].hash(&bundle.source.id), "{name}");
    }
    for name in [
        "case-added",
        "case-removed",
        "declaration-renamed",
        "file-moved",
        "parameter-renamed",
    ] {
        let bundle = &bundles[name];
        assert_ne!(hash, bundle.targets[0].hash(&bundle.source.id), "{name}");
    }
    let failures = bundles["parameter-missing"].targets[0].failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].case, "linux/test_add[one]");
    assert_eq!(failures[0].result, None);
}

#[test]
fn publishing_unrelated_targets_preserves_the_selected_contract() {
    let original = passing();
    let mut expanded = original.clone();
    let mut unrelated = original.targets[0].clone();
    unrelated.id = "tests/unrelated.rs::works".into();
    unrelated.interface_hash = unrelated.hash(&expanded.source.id).to_string();
    expanded.targets.insert(0, unrelated);
    // A newer imported revision/attempt may also contain unrelated targets.
    expanded.source.revision = "next-revision".into();
    expanded.execution.attempt += 1;
    let mut set = EvidenceSet::default();
    set.insert(
        "upstream".into(),
        expanded.source.id.clone(),
        Some(expanded),
    )
    .unwrap();
    let (source, target) = set.resolve("upstream/tests/example.rs").unwrap();
    assert_eq!(target, &original.targets[0]);
    assert_eq!(
        target.hash(&source.id),
        original.targets[0].hash(&original.source.id)
    );
}

#[test]
fn target_ids_are_opaque_and_are_not_percent_decoded_or_normalized() {
    let mut bundle = passing();
    let template = bundle.targets[0].clone();
    let ids = [
        "tests/math.test.ts::adds%3A%3Avalues",
        "tests/math.test.ts::adds::values",
        "tests/math.test.ts::adds%253A%253Avalues",
        "explicit legacy target",
    ];
    bundle.targets = ids
        .iter()
        .map(|id| {
            let mut target = template.clone();
            target.id = (*id).into();
            target.interface_hash = target.hash(&bundle.source.id).to_string();
            target
        })
        .collect();
    let mut set = EvidenceSet::default();
    set.insert("upstream".into(), bundle.source.id.clone(), Some(bundle))
        .unwrap();
    for id in ids {
        let (_, target) = set.resolve(&format!("upstream/{id}")).unwrap();
        assert_eq!(target.id, id);
    }
    assert!(matches!(
        set.resolve("upstream/tests/math.test.ts::adds%3a%3avalues"),
        Err(MissingEvidence::MissingTarget { .. })
    ));
}
