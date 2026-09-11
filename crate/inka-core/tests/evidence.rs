//! Producer-independent v2 vectors and local evidence resolution scenarios.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use inka_core::evidence::{
    Bundle, EvidenceConfig, EvidenceSet, MissingEvidence, SourceConfig, load_sources,
};

fn vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!(
        "../../../tests/golden/evidence/v2/contracts.json"
    ))
    .unwrap()
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
