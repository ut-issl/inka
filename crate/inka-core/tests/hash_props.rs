//! Property tests for the v1 view hash.
//!
//! The golden vectors freeze *one* answer per input; these state the rules the
//! answers obey for every input. Two of them are the whole reason hashing is
//! worth doing: whitespace an author cannot see must never invalidate an
//! approval, and content an author can read must never fail to.
//!
//! This file is the intended first `ci:` bind target, so it is written to be
//! read as a claim rather than as a test script.

use inka_core::hash::LocalView;
use inka_core::normalize;
use proptest::prelude::*;

/// A contract line: words, no leading or trailing space, never blank.
fn line() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{1,8}", 1..4).prop_map(|words| words.join(" "))
}

/// A contract in normal form already: non-blank lines joined with `\n`.
fn contract() -> impl Strategy<Value = String> {
    prop::collection::vec(line(), 1..5).prop_map(|lines| lines.join("\n"))
}

/// A list of child contracts together with one of its permutations.
fn children_and_a_permutation() -> impl Strategy<Value = (Vec<String>, Vec<String>)> {
    prop::collection::vec(contract(), 0..5)
        .prop_flat_map(|children| (Just(children.clone()), Just(children).prop_shuffle()))
}

/// The view hash of a fiat carrying `description`, which is the shortest local
/// view that still exercises description normalization.
fn hash(description: &str) -> String {
    LocalView::fiat(description, None).view_hash().to_string()
}

proptest! {
    /// Normalizing normalized text changes nothing, for any input at all.
    ///
    /// Every other property leans on this: if the normal form were not a fixed
    /// point, a hash would depend on how many times the text had been through
    /// the pipeline.
    #[test]
    fn normalization_is_idempotent(text: String) {
        let once = normalize::description(&text);
        let twice = normalize::description(&once);
        prop_assert_eq!(twice, once);
    }

    /// The normal form of already-normal text is itself.
    #[test]
    fn a_normalized_contract_is_its_own_normal_form(contract in contract()) {
        prop_assert_eq!(normalize::description(&contract), contract);
    }

    /// Indenting a description inside its fence is not an edit.
    #[test]
    fn the_fence_indent_does_not_reach_the_hash(
        contract in contract(),
        indent in 0usize..8,
    ) {
        let padding = " ".repeat(indent);
        let indented: Vec<String> = contract.lines().map(|l| format!("{padding}{l}")).collect();
        prop_assert_eq!(hash(&indented.join("\n")), hash(&contract));
    }

    /// Neither is a checkout that rewrote the line endings.
    #[test]
    fn line_endings_do_not_reach_the_hash(contract in contract()) {
        prop_assert_eq!(hash(&contract.replace('\n', "\r\n")), hash(&contract));
    }

    /// Nor is an editor that left whitespace at the ends of lines, or blank
    /// lines padding the fence.
    #[test]
    fn trailing_whitespace_and_fence_padding_do_not_reach_the_hash(
        contract in contract(),
        trailing in 0usize..4,
        padding in 0usize..3,
    ) {
        let spaces = " ".repeat(trailing);
        let body: Vec<String> = contract.lines().map(|l| format!("{l}{spaces}")).collect();
        let blank = "\n".repeat(padding + 1);
        let padded = format!("{blank}{}{blank}", body.join("\n"));
        prop_assert_eq!(hash(&padded), hash(&contract));
    }

    /// Contracts that read differently hash differently.
    ///
    /// Stated over the normal form, which is what "reads differently" means:
    /// two texts with the same normal form are the same contract.
    #[test]
    fn contracts_that_differ_after_normalization_hash_differently(
        left in contract(),
        right in contract(),
    ) {
        prop_assume!(normalize::description(&left) != normalize::description(&right));
        prop_assert_ne!(hash(&left), hash(&right));
    }

    /// Internal line breaks are content, so moving one is an edit.
    #[test]
    fn joining_two_lines_changes_the_hash(first in line(), second in line()) {
        let broken = format!("{first}\n{second}");
        let joined = format!("{first} {second}");
        prop_assert_ne!(hash(&broken), hash(&joined));
    }

    /// Reordering a `children:` list is filing, not content.
    #[test]
    fn child_order_does_not_reach_the_hash(
        description in contract(),
        (children, permuted) in children_and_a_permutation(),
    ) {
        let children: Vec<&str> = children.iter().map(String::as_str).collect();
        let permuted: Vec<&str> = permuted.iter().map(String::as_str).collect();

        prop_assert_eq!(
            LocalView::structure(&description, None, children).view_hash(),
            LocalView::structure(&description, None, permuted).view_hash()
        );
    }
}
