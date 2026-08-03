# syntax: lexer, parser, and `inka check`

- **Labels:** enhancement
- **Milestone:** M1
- **Depends on:** #6 (dogfood argument; the syntax is now exercised by two example
  projects: `tests/fixtures/dogfood` and `tests/fixtures/aocs`)

## Motivation

The first real command. Writing the parser is also the final validation of the
syntax: the remaining syntax-level open questions (escaping, fence details, ID
namespaces) can only be settled by implementing them. After this issue, both
example arguments are machine-checked, and every future syntax discussion has a
running referee.

## Scope

**Lexer**
- `//` line comments (dropped; never part of contracts)
- fences: variable-length (```` ``` ```` or longer), CommonMark-style containment;
  info string accepted and preserved (reserved for future use)
- `{$var}` recognition inside fences
- identifiers, `#tags`, block keywords, property keywords, `{` `}`, spans

**Parser (hand-written recursive descent)**
- block = mode keyword (`objective` / `structure` / `bind` / `fiat`) + id + tags
  + `{ ... }`
- exactly one fenced description per block
- mode-required properties as parse-level rules: `structure` → `children:`
  required; `bind` → `source:` required; `fiat` → neither allowed;
  `until:` / `promote:` annotations
- error recovery: skip to the closing `}` so one broken block does not cascade

**Project loading**
- `inka.toml` (`[project]`, `[phases] order/current`, `[values] files`)
- argument read by convention: all `*.inka` under `argument/` (recursive)
- `{$var}` interpolation from the values TOML — happens before anything else
  consumes descriptions (hashing later binds to the rendered text)

**Graph**
- ID interning (`String` → `u32`), adjacency map, toposort, reachability

**Diagnostics (`ariadne` or `miette`, span-carrying)**
- error: undefined child reference, duplicate ID, cycle (print the path),
  undefined `{$var}`, missing/duplicate description fence, unknown property,
  mode/property mismatch
- warning: proposition unreachable from the objective
- exit non-zero on any error

**CLI + tests**
- `clap` subcommand skeleton; `inka check [path]`
- insta snapshots: both example projects pass; a broken-fixtures suite triggers
  each diagnostic exactly once

## Settled rules to implement (rationale recorded as rustdoc, per code-first policy)

1. **No escape for `{$` in v1.** Only the exact pattern `{$identifier}` is treated
   as interpolation; any other `{` or `$` is always literal. An escape notation is
   deferred until a real need to write a literal `{$var}` in a contract appears
   (YAGNI). Typos are caught by the undefined-variable error.
2. **Property order is free. Duplicates are always an error.**
3. **Tags are free vocabulary.** `check` (M1) says nothing about tags. Typo
   detection arrives with policies (M3) as a warning on the policy side — a
   selector referencing a tag that appears nowhere selects the empty set and is
   probably a typo. Unreferenced tags on the argument side are legitimate
   forward-looking annotations.
4. **Namespaces are assigned by directory structure.** The namespace is the
   directory path relative to `argument/`, joined with `::`
   (`eol_capacity_ok` in `argument/power/eol.inka` is `power::eol_capacity_ok`).
   Files directly under `argument/` have no namespace. `::` cannot be written in
   an ID itself — namespaces arise only from location. References within the same
   namespace may omit the prefix; crossing namespaces requires the full path.
   Since IDs are excluded from hashes, moving a file is a bulk rename and
   invalidates no approvals. Policy selectors use the same notation (`power::*`).
   `source:` references keep Python's dotted names (`veriq:aocs.verify_detumble`)
   since they name veriq-side objects.
5. **`source:` gets a format check only** at `check` time; existence is eval's
   job once a veriq export is present.

## Acceptance criteria

- [ ] `inka check` passes on `tests/fixtures/dogfood` and `tests/fixtures/aocs`.
- [ ] Broken-fixtures suite exists; all diagnostics snapshot-fixed with insta.
- [ ] The five rules above are implemented and documented in rustdoc with a
      one-line rationale each.

## References

- inka-concept.md §2.4 (diagnostics), §2.5 (values), §12 (syntax)
- inka-implementation-roadmap.md M1
- fixtures READMEs (dogfood / aocs) — open questions feeding this issue