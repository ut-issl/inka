//! Reading the `JUnit` report nextest writes.

use std::collections::HashMap;
use std::path::Path;

use inka_evidence::Outcome;

use crate::error::Error;

/// Final per-test outcomes from one run, keyed by suite and test name.
#[derive(Debug)]
pub struct Report {
    outcomes: HashMap<(String, String), Outcome>,
}

impl Report {
    /// A report with no results: the shape of a run that was never needed,
    /// such as one whose every selected test is ignored.
    pub fn empty() -> Self {
        Self {
            outcomes: HashMap::new(),
        }
    }

    /// The outcome the run reported for one test, if any: a test that never
    /// ran (aborted or cancelled run) is simply absent.
    pub fn outcome(&self, binary_id: &str, name: &str) -> Option<Outcome> {
        self.outcomes
            .get(&(binary_id.to_owned(), name.to_owned()))
            .copied()
    }
}

/// Parses the `JUnit` file nextest wrote for this run.
///
/// Only the final verdict of each testcase counts: a `failure` or `error`
/// child means fail, a `skipped` child skip, otherwise pass. Retry
/// children (`flakyFailure`, `rerunFailure`) describe earlier attempts of
/// a test that finally passed, so they do not enter the outcome.
pub fn parse(path: &Path) -> Result<Report, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            Error::JunitMissing {
                path: path.to_owned(),
            }
        } else {
            Error::Read {
                path: path.to_owned(),
                source,
            }
        }
    })?;
    parse_text(&text).map_err(|detail| Error::JunitMalformed {
        path: path.to_owned(),
        detail,
    })
}

/// Parses the report text itself; the caller attaches the path.
fn parse_text(text: &str) -> Result<Report, String> {
    let document = roxmltree::Document::parse(text).map_err(|error| error.to_string())?;
    let mut outcomes = HashMap::new();
    for case in document
        .descendants()
        .filter(|node| node.has_tag_name("testcase"))
    {
        let suite = case
            .ancestors()
            .find(|node| node.has_tag_name("testsuite"))
            .and_then(|node| node.attribute("name"))
            .ok_or("a testcase has no enclosing named testsuite")?;
        let name = case
            .attribute("name")
            .ok_or("a testcase carries no name attribute")?;
        let outcome = if case
            .children()
            .any(|child| child.has_tag_name("failure") || child.has_tag_name("error"))
        {
            Outcome::Fail
        } else if case.children().any(|child| child.has_tag_name("skipped")) {
            Outcome::Skip
        } else {
            Outcome::Pass
        };
        if outcomes
            .insert((suite.to_owned(), name.to_owned()), outcome)
            .is_some()
        {
            // Conflicting records for one execution are malformed input,
            // never a silent overwrite.
            return Err(format!("duplicate testcase `{name}` in suite `{suite}`"));
        }
    }
    Ok(Report { outcomes })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="4" failures="1" errors="0">
    <testsuite name="sample" tests="2" failures="1" errors="0">
        <testcase name="tests::passes" classname="sample"/>
        <testcase name="tests::fails" classname="sample">
            <failure message="assertion failed" type="test failure">boom</failure>
            <system-out>output</system-out>
        </testcase>
    </testsuite>
    <testsuite name="sample::props" tests="2" failures="0" errors="0">
        <testcase name="flaky" classname="sample::props">
            <flakyFailure message="first try" type="test failure">retry</flakyFailure>
        </testcase>
        <testcase name="broke" classname="sample::props">
            <error message="exec" type="execution error"/>
        </testcase>
    </testsuite>
</testsuites>
"#;

    #[test]
    fn maps_final_verdicts_and_ignores_retries() {
        let report = parse_text(REPORT).expect("the fixture XML parses");

        assert_eq!(
            report.outcome("sample", "tests::passes"),
            Some(Outcome::Pass)
        );
        assert_eq!(
            report.outcome("sample", "tests::fails"),
            Some(Outcome::Fail)
        );
        assert_eq!(
            report.outcome("sample::props", "flaky"),
            Some(Outcome::Pass)
        );
        assert_eq!(
            report.outcome("sample::props", "broke"),
            Some(Outcome::Fail)
        );
        assert_eq!(report.outcome("sample", "tests::missing"), None);
    }

    #[test]
    fn rejects_a_nameless_testcase() {
        let broken = r#"<testsuites><testsuite name="s"><testcase/></testsuite></testsuites>"#;
        assert!(parse_text(broken).is_err());
    }

    #[test]
    fn rejects_text_that_is_not_xml() {
        assert!(parse_text("not xml").is_err());
    }

    #[test]
    fn rejects_a_duplicate_testcase() {
        let duplicated = r#"<testsuites><testsuite name="s">
            <testcase name="t"/>
            <testcase name="t"><failure message="again"/></testcase>
        </testsuite></testsuites>"#;

        let error = parse_text(duplicated).expect_err("conflicting records are malformed");
        assert!(error.contains("duplicate testcase `t`"));
    }
}
