use std::collections::HashMap;

use super::format_source;
use crate::source::SourceMap;

/// The values every test project defines.
const VALUES: [(&str, &str); 1] = [("pointing_accuracy", "0.1 deg")];

fn format(text: &str) -> String {
    try_format(text).expect("the text should parse")
}

fn try_format(text: &str) -> Result<String, Vec<String>> {
    let values: HashMap<String, String> = VALUES
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    let mut map = SourceMap::new();
    let file_id = map.add_file("test.inka".into(), text.to_owned());
    format_source(file_id, text, &values)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect())
}

/// Asserts that `input` formats to `expected` and that `expected` is a fixed
/// point.
fn assert_formats(input: &str, expected: &str) {
    assert_eq!(format(input), expected);
    assert_eq!(format(expected), expected, "formatting must be idempotent");
}

#[test]
fn lays_out_the_formatting_page_example() {
    let input = concat!(
        "// File comment.\n",
        "\n",
        "structure pointing_ok   #mission #aocs { // Comment on the block.\n",
        "  ```\n",
        "  In steady-state observation mode, the pointing control error is at most {$pointing_accuracy}\n",
        "  ```\n",
        "  children: attitude_knowledge_ok, // Comment on this child.\n",
        "    control_law_ok, mass_properties_valid // Comment on the last child.\n",
        "}\n",
        "fiat mass_properties_valid {\n",
        "  promote: bind\n",
        "  until: M3 // Awaiting the mass-properties test.\n",
        "  ```\n",
        "  The mass properties used by the control design match the flight model\n",
        "  ```\n",
        "}\n",
        "structure single {\n",
        "```\n",
        "...\n",
        "```\n",
        "children: only_child }\n",
    );
    let expected = concat!(
        "// File comment.\n",
        "\n",
        "// Comment on the block.\n",
        "structure pointing_ok #mission #aocs {\n",
        "    ```\n",
        "    In steady-state observation mode, the pointing control error is at most {$pointing_accuracy}\n",
        "    ```\n",
        "    children:\n",
        "        // Comment on this child.\n",
        "        attitude_knowledge_ok,\n",
        "        control_law_ok,\n",
        "        // Comment on the last child.\n",
        "        mass_properties_valid\n",
        "}\n",
        "\n",
        "fiat mass_properties_valid {\n",
        "    ```\n",
        "    The mass properties used by the control design match the flight model\n",
        "    ```\n",
        "    // Awaiting the mass-properties test.\n",
        "    until: M3\n",
        "    promote: bind\n",
        "}\n",
        "\n",
        "structure single {\n",
        "    ```\n",
        "    ...\n",
        "    ```\n",
        "    children: only_child\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn a_fence_naming_an_undefined_value_keeps_its_content_as_written() {
    // Interpolation errors are not parse errors, so the block is formatted,
    // but its contract cannot be rendered to prove re-indenting is safe.
    let input = "fiat f {\n  ```\n  uses {$undefined}\n  ```\n}\n";
    let expected = "fiat f {\n    ```\n  uses {$undefined}\n    ```\n}\n";

    assert_formats(input, expected);
}

#[test]
fn a_comment_on_children_is_written_above_the_property() {
    let input = concat!(
        "structure s {\n",
        "```\n",
        "x\n",
        "```\n",
        "// why these children\n",
        "children: a, b\n",
        "}\n",
    );
    let expected = concat!(
        "structure s {\n",
        "    ```\n",
        "    x\n",
        "    ```\n",
        "    // why these children\n",
        "    children:\n",
        "        a,\n",
        "        b\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn a_single_child_with_a_comment_goes_on_its_own_line() {
    let input = "structure s {\n```\nx\n```\nchildren: a // the only one\n}\n";
    let expected = concat!(
        "structure s {\n",
        "    ```\n",
        "    x\n",
        "    ```\n",
        "    children:\n",
        "        // the only one\n",
        "        a\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn namespaced_children_are_written_as_written() {
    let input = "objective root {\n```\nx\n```\nchildren: power::battery, thermal::radiator\n}\n";
    let expected = concat!(
        "objective root {\n",
        "    ```\n",
        "    x\n",
        "    ```\n",
        "    children:\n",
        "        power::battery,\n",
        "        thermal::radiator\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn bind_sources_and_objective_promotion_are_written_in_order() {
    let input = concat!(
        "objective root { promote: structure\n",
        "until: CDR\n",
        "```\n",
        "goal\n",
        "```\n",
        "children: a }\n",
        "bind a { veriq: \"aocs.check\" // the check\n",
        "```\n",
        "a\n",
        "```\n",
        "}\n",
        "bind b {\n",
        "```\n",
        "b\n",
        "```\n",
        "test: \"tests/b.rs::b\"\n",
        "until: PDR\n",
        "}\n",
    );
    let expected = concat!(
        "objective root {\n",
        "    ```\n",
        "    goal\n",
        "    ```\n",
        "    children: a\n",
        "    until: CDR\n",
        "    promote: structure\n",
        "}\n",
        "\n",
        "bind a {\n",
        "    ```\n",
        "    a\n",
        "    ```\n",
        "    // the check\n",
        "    veriq: \"aocs.check\"\n",
        "}\n",
        "\n",
        "bind b {\n",
        "    ```\n",
        "    b\n",
        "    ```\n",
        "    test: \"tests/b.rs::b\"\n",
        "    until: PDR\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn dangling_comments_move_above_the_block_header() {
    let input = concat!(
        "fiat f {\n",
        "```\n",
        "f\n",
        "```\n",
        "// dangling before the brace\n",
        "}\n",
    );
    let expected = concat!(
        "// dangling before the brace\n",
        "fiat f {\n",
        "    ```\n",
        "    f\n",
        "    ```\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn comments_after_the_last_block_join_the_file_comment_at_the_top() {
    let input = concat!(
        "// header\n",
        "\n",
        "fiat f {\n",
        "```\n",
        "f\n",
        "```\n",
        "}\n",
        "\n",
        "// footer\n",
    );
    let expected = concat!(
        "// header\n",
        "// footer\n",
        "\n",
        "fiat f {\n",
        "    ```\n",
        "    f\n",
        "    ```\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn comment_lines_keep_their_text_and_empty_lines() {
    let input = concat!(
        "//   indented text\n",
        "//\n",
        "//second\n",
        "fiat f {\n",
        "```\n",
        "f\n",
        "```\n",
        "}\n",
    );
    let expected = concat!(
        "//   indented text\n",
        "//\n",
        "// second\n",
        "fiat f {\n",
        "    ```\n",
        "    f\n",
        "    ```\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn a_file_of_only_comments_keeps_them_without_a_blank_line() {
    assert_formats("// just a note\n\n", "// just a note\n");
}

#[test]
fn an_empty_file_stays_empty() {
    assert_formats("\n\n", "");
}

#[test]
fn fences_keep_their_length_and_info_string() {
    let input = concat!(
        "fiat f {\n",
        "  ````md  \n",
        "  ```\n",
        "  inner fence\n",
        "  ```\n",
        "  `````\n",
        "}\n",
    );
    let expected = concat!(
        "fiat f {\n",
        "    ````md\n",
        "    ```\n",
        "    inner fence\n",
        "    ```\n",
        "    ````\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn an_info_string_starting_with_a_backtick_stays_apart_from_the_fence() {
    // Written directly after the fence, "`x" would lengthen it to four
    // backticks and the three-backtick closing fence would no longer close.
    let input = "fiat f {\n``` `x\nhello\n```\n}\n";
    let expected = "fiat f {\n    ``` `x\n    hello\n    ```\n}\n";

    assert_formats(input, expected);
}

#[test]
fn a_multi_word_info_string_is_kept_verbatim() {
    let input = "fiat f {\n```  rust ignore \nhello\n```\n}\n";
    let expected = "fiat f {\n    ```rust ignore\n    hello\n    ```\n}\n";

    assert_formats(input, expected);
}

#[test]
fn crlf_input_is_written_with_lf() {
    let input = "fiat f {\r\n```\r\nf\r\n```\r\n}\r\n";
    let expected = "fiat f {\n    ```\n    f\n    ```\n}\n";

    assert_formats(input, expected);
}

#[test]
fn blocks_are_separated_by_exactly_one_blank_line() {
    let input = "fiat a {\n```\na\n```\n}\n\n\n\nfiat b {\n```\nb\n```\n}";
    let expected = concat!(
        "fiat a {\n",
        "    ```\n",
        "    a\n",
        "    ```\n",
        "}\n",
        "\n",
        "fiat b {\n",
        "    ```\n",
        "    b\n",
        "    ```\n",
        "}\n",
    );

    assert_formats(input, expected);
}

#[test]
fn a_text_with_a_parse_error_is_not_formatted() {
    let errors = try_format("fiat f {\n```\nf\n```\nreason: x\n}\n")
        .expect_err("an unknown property is a parse error");

    assert!(
        errors
            .iter()
            .any(|error| error.contains("unknown property"))
    );
}

#[test]
fn a_text_with_a_lex_error_is_not_formatted() {
    let errors = try_format("fiat f {\n```\nunclosed\n").expect_err("an unclosed fence");

    assert!(errors.iter().any(|error| error.contains("unclosed fenced")));
}
