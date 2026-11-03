//! `until:` phases checked against the manifest's declared order.
//!
//! Evaluation compares a deadline's phase by its position in `phases.order`,
//! so a phase the order does not list cannot be judged at all. That is a
//! property of the source text, so `inka check` reports it at the phase name
//! instead of leaving `eval` to fail without a location.

use std::collections::HashSet;

use crate::ast::Block;
use crate::interpolate::RenderedDescription;
use crate::manifest::PhasesSection;
use crate::source::Error;

/// One error per `until:` whose phase `phases.order` does not list, anchored
/// at the phase name, in block order.
#[must_use]
pub fn check(blocks: &[Block<RenderedDescription>], phases: &PhasesSection) -> Vec<Error> {
    let declared: HashSet<&str> = phases.order.iter().map(String::as_str).collect();
    blocks
        .iter()
        .filter_map(Block::deadline)
        .filter(|deadline| !declared.contains(deadline.until.value.as_str()))
        .map(|deadline| {
            Error::new(
                deadline.until.span,
                format!(
                    "unknown phase `{}` in `until:`; phases.order lists {}",
                    deadline.until.value,
                    listing(&phases.order),
                ),
            )
        })
        .collect()
}

/// The declared phases as a backticked, comma-separated list.
fn listing(order: &[String]) -> String {
    order
        .iter()
        .map(|phase| format!("`{phase}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use crate::interpolate::interpolate;
    use crate::lexer::Lexer;
    use crate::parser::parse;
    use crate::source::SourceMap;

    /// Lexes, parses, and interpolates one synthetic file, the same pipeline
    /// the project loader runs.
    fn blocks(source: &str) -> Vec<Block<RenderedDescription>> {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), source.to_owned());
        let tokens = Lexer::new(file_id, source)
            .lex()
            .expect("test source should lex");
        let parsed = parse(file_id, &tokens);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        parsed
            .blocks
            .into_iter()
            .map(|block| {
                let rendered = interpolate(&block.common().description, &HashMap::new())
                    .expect("test source should interpolate");
                block.with_description(rendered)
            })
            .collect()
    }

    fn phases(order: &[&str]) -> PhasesSection {
        PhasesSection {
            order: order.iter().map(|&phase| phase.to_owned()).collect(),
            current: order[0].to_owned(),
        }
    }

    #[test]
    fn reports_an_undeclared_phase_at_its_name() {
        // Arrange: one declared deadline and one typo, on a fiat and a bind.
        let source = "\
fiat known {
  ```
  k
  ```
  until: M1
}
fiat typo {
  ```
  t
  ```
  until: M9
  promote: bind
}
";
        let blocks = blocks(source);

        // Act
        let errors = check(&blocks, &phases(&["M0", "M1"]));

        // Assert
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            errors[0].message,
            "unknown phase `M9` in `until:`; phases.order lists `M0`, `M1`"
        );
        let span = errors[0].span.expect("anchored");
        assert_eq!(&source[span.start..span.end], "M9");
    }

    #[test]
    fn checks_deadlines_on_every_mode() {
        // Arrange: objective, structure, and bind each name an unknown phase.
        let source = "\
objective root {
  ```
  r
  ```
  children: s
  until: X1
}
structure s {
  ```
  s
  ```
  children: b
  until: X2
}
bind b {
  ```
  b
  ```
  test: \"cargo:t\"
  until: X3
}
";
        let blocks = blocks(source);

        // Act
        let errors = check(&blocks, &phases(&["M0"]));

        // Assert
        let named: Vec<&str> = errors
            .iter()
            .map(|error| {
                let span = error.span.expect("anchored");
                &source[span.start..span.end]
            })
            .collect();
        assert_eq!(named, ["X1", "X2", "X3"]);
    }

    #[test]
    fn declared_phases_and_missing_deadlines_are_clean() {
        // Arrange
        let blocks =
            blocks("fiat a {\n  ```\n  a\n  ```\n  until: M0\n}\nfiat b {\n  ```\n  b\n  ```\n}\n");

        // Act & Assert
        assert_eq!(check(&blocks, &phases(&["M0"])), [] as [Error; 0]);
    }
}
