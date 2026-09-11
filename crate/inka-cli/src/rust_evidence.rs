//! Rust source extraction and captured libtest pretty-output adaptation.
//! Tests are run by the workflow, never by this module.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use inka_core::evidence::{CaseResult, ResultKind};
use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Attribute, Block, Ident, Item, Token, Visibility};

use crate::check::Outcome;
use crate::ci_export::{Receipt, Results, TargetResults, finish, read};

struct Property {
    name: Ident,
    start: Span,
    body: Block,
}

struct Properties {
    attributes: Vec<Attribute>,
    tests: Vec<Property>,
}

impl Parse for Properties {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_inner)?;
        let mut tests = Vec::new();
        while !input.is_empty() {
            let attrs = input.call(Attribute::parse_outer)?;
            let visibility: Visibility = input.parse()?;
            let function: Token![fn] = input.parse()?;
            let name = input.parse()?;
            let params;
            syn::parenthesized!(params in input);
            let _: proc_macro2::TokenStream = params.parse()?;
            let body = input.parse()?;
            let start = attrs.first().map_or_else(
                || match &visibility {
                    Visibility::Inherited => function.span,
                    _ => visibility.span(),
                },
                Spanned::span,
            );
            tests.push(Property { name, start, body });
        }
        Ok(Self { attributes, tests })
    }
}

fn slice(source: &str, start: Span, end: Span) -> io::Result<&str> {
    source
        .get(start.byte_range().start..end.byte_range().end)
        .ok_or_else(|| io::Error::other("Rust parser returned a source span outside the input"))
}

fn insert(contracts: &mut BTreeMap<String, String>, name: &str, content: String) -> io::Result<()> {
    if contracts.insert(name.to_owned(), content).is_some() {
        return Err(io::Error::other(format!(
            "ambiguous Rust test `{name}` (conditional duplicates are unsupported)"
        )));
    }
    Ok(())
}

fn collect(
    source: &str,
    items: &[Item],
    namespace: &str,
    contracts: &mut BTreeMap<String, String>,
) -> io::Result<()> {
    for item in items {
        match item {
            Item::Fn(function)
                if function
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("test")) =>
            {
                let content = slice(
                    source,
                    function.span(),
                    function.block.brace_token.span.close(),
                )?;
                insert(
                    contracts,
                    &format!("{namespace}{}", function.sig.ident),
                    content.into(),
                )?;
            }
            Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    collect(
                        source,
                        items,
                        &format!("{namespace}{}::", module.ident),
                        contracts,
                    )?;
                }
            }
            Item::Macro(item)
                if item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "proptest") =>
            {
                let properties =
                    syn::parse2::<Properties>(item.mac.tokens.clone()).map_err(io::Error::other)?;
                let configuration =
                    match (properties.attributes.first(), properties.attributes.last()) {
                        (Some(first), Some(last)) => slice(source, first.span(), last.span())?,
                        _ => "",
                    };
                for property in properties.tests {
                    let text = slice(
                        source,
                        property.start,
                        property.body.brace_token.span.close(),
                    )?;
                    let content = format!("proptest! {{\n{configuration}\n{text}\n}}");
                    insert(contracts, &format!("{namespace}{}", property.name), content)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn extract(source: &str, selector: &str) -> io::Result<String> {
    let file = syn::parse_file(source).map_err(io::Error::other)?;
    let mut contracts = BTreeMap::new();
    collect(source, &file.items, "", &mut contracts)?;
    contracts.remove(selector).ok_or_else(|| io::Error::other(format!("Rust test `{selector}` not found; only explicit #[test] functions and proptest declarations are supported")))
}

/// Read one test binary's captured pretty output. Ignore captured failure text
/// after the failures separator; never mistake test stdout for harness results.
fn parse_report(text: &str) -> io::Result<BTreeMap<String, ResultKind>> {
    let mut results = BTreeMap::new();
    let mut expected = None;
    let mut summary = None;
    let mut failure_details = false;
    for line in text.lines() {
        if let Some(count) = line
            .strip_prefix("running ")
            .and_then(|s| s.strip_suffix(" tests").or_else(|| s.strip_suffix(" test")))
        {
            if expected.is_some() {
                return Err(io::Error::other(
                    "expected output from one libtest binary, not a combined workspace run",
                ));
            }
            expected = Some(count.parse::<usize>().map_err(io::Error::other)?);
        } else if line.starts_with("test result: ") {
            if summary.is_some() {
                return Err(io::Error::other("duplicate libtest summary"));
            }
            summary = Some(line);
        } else if line == "failures:" {
            failure_details = true;
        } else if expected.is_some()
            && !failure_details
            && summary.is_none()
            && let Some(row) = line.strip_prefix("test ")
        {
            let (name, status) = row.rsplit_once(" ... ").ok_or_else(|| {
                io::Error::other(
                    "invalid libtest row; use captured --format pretty --color never output",
                )
            })?;
            let outcome = match status {
                "ok" => ResultKind::Pass,
                "FAILED" => ResultKind::Fail,
                value if value == "ignored" || value.starts_with("ignored, ") => ResultKind::Skip,
                _ => {
                    return Err(io::Error::other(format!(
                        "unsupported libtest outcome `{status}`"
                    )));
                }
            };
            if results.insert(name.into(), outcome).is_some() {
                return Err(io::Error::other(format!(
                    "duplicate libtest result `{name}`"
                )));
            }
        }
    }
    let Some(count) = expected else {
        return Err(io::Error::other(
            "missing libtest header; expected one captured pretty-format test run",
        ));
    };
    if let Some(summary) = summary {
        if results.len() != count {
            return Err(io::Error::other(
                "libtest result count disagrees with its header",
            ));
        }
        let fields: Vec<&str> = summary.split(';').collect();
        for (suffix, outcome) in [
            (" passed", ResultKind::Pass),
            (" failed", ResultKind::Fail),
            (" ignored", ResultKind::Skip),
        ] {
            let field = fields
                .iter()
                .find_map(|field| field.trim().strip_suffix(suffix))
                .ok_or_else(|| io::Error::other("incomplete libtest summary"))?;
            let observed = field
                .split_whitespace()
                .last()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| io::Error::other("invalid libtest summary count"))?;
            if observed != results.values().filter(|&&value| value == outcome).count() {
                return Err(io::Error::other(
                    "libtest summary disagrees with individual results",
                ));
            }
        }
    } else {
        // A crashed/interrupted harness cannot certify a completed run.
        for outcome in results.values_mut() {
            if *outcome == ResultKind::Pass {
                *outcome = ResultKind::Cancelled;
            }
        }
    }
    Ok(results)
}

pub fn finish_rust(receipt_path: &Path, reports: &[String], out: &Path) -> io::Result<Outcome> {
    let receipt: Receipt = read(receipt_path)?;
    receipt.validate()?;
    let mut outcomes = BTreeMap::new();
    let mut environments = std::collections::HashSet::new();
    for report in reports {
        let (environment, path) = report
            .split_once('=')
            .filter(|(env, path)| !env.trim().is_empty() && !path.is_empty())
            .ok_or_else(|| io::Error::other("--report requires ENVIRONMENT=PATH"))?;
        if !environments.insert(environment) {
            return Err(io::Error::other("duplicate report environment"));
        }
        for (case, result) in parse_report(&fs::read_to_string(path)?)? {
            outcomes.insert(format!("{environment}/{case}"), result);
        }
    }
    let targets = receipt
        .bundle
        .targets
        .iter()
        .map(|target| TargetResults {
            id: target.id.clone(),
            interface_hash: target.interface_hash.clone(),
            results: target
                .required
                .iter()
                .filter_map(|case| {
                    outcomes.get(case).map(|&result| CaseResult {
                        case: case.clone(),
                        result,
                    })
                })
                .collect(),
        })
        .collect();
    let report = Results {
        v: 1,
        source: receipt.bundle.source.clone(),
        execution: receipt.bundle.execution.clone(),
        targets,
    };
    finish(receipt, report, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_actual_dogfood_proptest() {
        let source = include_str!("../../inka-core/tests/hash_props.rs");
        let extracted = extract(source, "normalization_is_idempotent").unwrap();
        assert!(extracted.contains("text: String"));
        assert!(!extracted.contains("fn child_order_does_not_reach_the_hash"));
    }

    #[test]
    fn extracts_selected_source_and_macro_configuration_without_unrelated_tests() {
        let source = "fn helper() {}\n#[test]\nfn one() { assert!(true); }\nproptest! { #![proptest_config(Config::with_cases(9))] #[test] fn prop(x in 0..10u8) { assert!(x < 10); } }";
        let one = extract(source, "one").unwrap();
        assert!(one.starts_with("#[test]"));
        assert!(!one.contains("helper"));
        assert_eq!(
            one,
            extract(&source.replace("0..10u8", "0..20u8"), "one").unwrap()
        );
        let property = extract(source, "prop").unwrap();
        assert!(property.contains("with_cases(9)"));
        assert!(property.contains("x in 0..10u8"));
        assert_ne!(
            property,
            extract(&source.replace("0..10u8", "0..20u8"), "prop").unwrap()
        );
    }

    #[test]
    fn nested_test_and_utf8_spans_are_extracted_without_guessing_boundaries() {
        let source = "// 日本語\nmod nested { #[test] fn one() { let text = r#\"} fn other() {\"#; assert!(!text.is_empty()); } }";
        assert!(
            extract(source, "nested::one")
                .unwrap()
                .starts_with("#[test]")
        );
        assert!(extract(source, "one").is_err());
        assert!(extract("#[test] fn x() {} #[test] fn x() {}", "x").is_err());
    }

    #[test]
    fn report_counts_skip_and_interruption_are_not_silently_passed() {
        let report = "running 3 tests\ntest one ... ok\ntest two ... FAILED\ntest three ... ignored\nfailures:\ntest spoof ... ok\ntest result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.0s\n";
        let cases = parse_report(report).unwrap();
        assert_eq!(cases.len(), 3);
        assert_eq!(cases["three"], ResultKind::Skip);
        assert!(parse_report(&report.replace("1 passed", "2 passed")).is_err());
        assert_eq!(
            parse_report("running 2 tests\ntest one ... ok\n").unwrap()["one"],
            ResultKind::Cancelled
        );
    }
}
