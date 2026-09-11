//! Source-side evidence generation. Test selection/execution belongs to the
//! caller; a receipt binds pre-run content to independently supplied results.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use inka_core::evidence::{Bundle, CaseResult, Execution, Kind, Source, Target};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::check::Outcome;
use crate::cli::CiExportCommand;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub v: u32,
    pub source: Source,
    pub execution: Execution,
    pub targets: Vec<PlanTarget>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTarget {
    pub id: String,
    pub file: PathBuf,
    pub required: Vec<String>,
}

/// Local pre-run receipt; only its bundle is delivered to the consumer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub v: u32,
    pub bundle: Bundle,
    pub files: Vec<FileGuard>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileGuard {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Results {
    pub v: u32,
    pub source: Source,
    pub execution: Execution,
    pub targets: Vec<TargetResults>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetResults {
    pub id: String,
    pub interface_hash: String,
    pub results: Vec<CaseResult>,
}

pub fn read<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text)
        .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Serialize completely before publishing, so a failed write cannot leave a
/// truncated JSON file that another step mistakes for a finished bundle.
pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".inka-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn raw_hash(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub fn guard(path: &Path) -> io::Result<FileGuard> {
    Ok(FileGuard {
        path: fs::canonicalize(path)?,
        sha256: raw_hash(&fs::read(path)?),
    })
}

impl Receipt {
    pub fn validate(&self) -> io::Result<()> {
        if self.v != 1 {
            return Err(io::Error::other("unsupported preparation receipt version"));
        }
        self.bundle.validate().map_err(io::Error::other)?;
        if self.files.is_empty() || self.bundle.targets.is_empty() {
            return Err(io::Error::other(
                "a preparation receipt requires targets and source guards",
            ));
        }
        for file in &self.files {
            if raw_hash(&fs::read(&file.path)?) != file.sha256 {
                return Err(io::Error::other(format!(
                    "source content changed after preparation: {}",
                    file.path.display()
                )));
            }
        }
        Ok(())
    }
}

fn prepare(plan_path: &Path, out: &Path) -> io::Result<()> {
    let plan: Plan = read(plan_path)?;
    if plan.v != 1 {
        return Err(io::Error::other("unsupported preparation plan version"));
    }
    let base = plan_path.parent().unwrap_or(Path::new("."));
    let mut receipt = Receipt {
        v: 1,
        bundle: Bundle {
            v: 2,
            source: plan.source,
            execution: plan.execution,
            environment: None,
            targets: Vec::new(),
        },
        files: Vec::new(),
    };
    for spec in plan.targets {
        let path = base.join(spec.file);
        // Read a guard first; validation below catches a change between reads.
        receipt.files.push(guard(&path)?);
        let mut target = Target {
            id: spec.id,
            kind: Kind::File,
            profile: "file-source:v1".into(),
            content: fs::read_to_string(&path)?,
            interface_hash: String::new(),
            required: spec.required,
            results: Vec::new(),
        };
        target.interface_hash = target.hash(&receipt.bundle.source.id).to_string();
        receipt.bundle.targets.push(target);
    }
    receipt.validate()?;
    write_json(out, &receipt)
}

pub fn finish(mut receipt: Receipt, results: Results, out: &Path) -> io::Result<Outcome> {
    receipt.validate()?;
    if results.v != 1
        || results.source != receipt.bundle.source
        || results.execution != receipt.bundle.execution
    {
        return Err(io::Error::other(
            "results do not match the prepared source revision and execution attempt",
        ));
    }
    if receipt
        .bundle
        .targets
        .iter()
        .any(|target| !target.results.is_empty())
    {
        return Err(io::Error::other(
            "preparation receipt already contains execution results",
        ));
    }
    let mut seen = HashSet::new();
    for result in results.targets {
        if !seen.insert(result.id.clone()) {
            return Err(io::Error::other(format!(
                "duplicate result target `{}`",
                result.id
            )));
        }
        let target = receipt
            .bundle
            .targets
            .iter_mut()
            .find(|target| target.id == result.id)
            .ok_or_else(|| io::Error::other(format!("unprepared result target `{}`", result.id)))?;
        if target.interface_hash != result.interface_hash {
            return Err(io::Error::other(format!(
                "results refer to different content for `{}`",
                result.id
            )));
        }
        target.results = result.results;
    }
    receipt.bundle.validate().map_err(io::Error::other)?;
    let passed = receipt
        .bundle
        .targets
        .iter()
        .all(|target| target.failures().is_empty());
    write_json(out, &receipt.bundle)?;
    Ok(if passed { Outcome::Ok } else { Outcome::Failed })
}

pub fn run(command: CiExportCommand) -> io::Result<Outcome> {
    match command {
        CiExportCommand::Prepare { plan, out } => {
            prepare(&plan, &out)?;
            Ok(Outcome::Ok)
        }
        CiExportCommand::Finish {
            receipt,
            results,
            out,
        } => finish(read(&receipt)?, read(&results)?, &out),
        CiExportCommand::Validate { file } => {
            Bundle::parse(&fs::read_to_string(file)?).map_err(io::Error::other)?;
            Ok(Outcome::Ok)
        }
    }
}
