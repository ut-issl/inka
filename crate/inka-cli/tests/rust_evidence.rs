//! Run a real test binary outside inka, then adapt its captured results.

use std::fs;
use std::path::PathBuf;
use std::process::Command as Process;

use assert_cmd::Command;
use inka_core::evidence::{Bundle, Kind, ResultKind};
use serde_json::json;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn actual_libtest_function_results_are_bound_to_prepared_source() {
    let temp = Temp(std::env::temp_dir().join(format!("inka-rust-adapter-{}", std::process::id())));
    fs::create_dir(&temp.0).unwrap();
    let source = "#[test] fn passing() { assert_eq!(2 + 2, 4); }\n#[test] fn failing() { panic!(\"expected\"); }\n#[test] #[ignore] fn ignored() {}\n";
    fs::write(temp.0.join("source.rs"), source).unwrap();
    let plan = json!({"v":1,"source":{"id":"github:example/rust","revision":"test-revision"},"execution":{"id":"synthetic-test","attempt":1},"targets":[
        {"id":"passing-contract","file":"source.rs","selector":"passing","required":["linux/passing"]},
        {"id":"failing-contract","file":"source.rs","selector":"failing","required":["linux/failing"]},
        {"id":"ignored-contract","file":"source.rs","selector":"ignored","required":["linux/ignored"]}
    ]});
    fs::write(temp.0.join("plan.json"), plan.to_string()).unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .current_dir(&temp.0)
        .args([
            "ci-export",
            "prepare-rust",
            "--plan",
            "plan.json",
            "--out",
            "receipt.json",
        ])
        .assert()
        .success();
    let binary = temp
        .0
        .join(format!("tests{}", std::env::consts::EXE_SUFFIX));
    assert!(
        Process::new("rustc")
            .arg("--test")
            .arg(temp.0.join("source.rs"))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let output = Process::new(&binary)
        .args(["--format", "pretty", "--color", "never"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    fs::write(temp.0.join("report.txt"), &output.stdout).unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .current_dir(&temp.0)
        .args([
            "ci-export",
            "finish-rust",
            "--receipt",
            "receipt.json",
            "--report",
            "linux=report.txt",
            "--out",
            "bundle.json",
        ])
        .assert()
        .failure();
    let bundle = Bundle::parse(&fs::read_to_string(temp.0.join("bundle.json")).unwrap()).unwrap();
    assert_eq!(bundle.targets[0].kind, Kind::Function);
    assert_eq!(bundle.targets[0].results[0].result, ResultKind::Pass);
    assert!(!bundle.targets[0].content.contains("fn failing"));
    assert_eq!(bundle.targets[1].results[0].result, ResultKind::Fail);
    assert_eq!(bundle.targets[2].results[0].result, ResultKind::Skip);
}
