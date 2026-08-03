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

## Decisions to settle in this issue (recorded as rustdoc, per code-first policy)

1. **Escaping rule** for a literal `{$` inside a description.
2. **Property ordering**: free order or fixed? duplicates always an error?
3. **Unreferenced tags**: silent, or warn (no policy exists yet to reference them)?
4. **ID namespaces**: flat vs dotted (`aocs.pointing_ok`). The aocs example wants
   to share a node with a power-domain argument, and concept's policy selectors
   already assume `power.*`.
5. **Unresolvable `source:` references**: format check only at `check` time
   (existence is eval's job once a veriq export is present) — confirm.

## Acceptance criteria

- [ ] `inka check` passes on `tests/fixtures/dogfood` and `tests/fixtures/aocs`.
- [ ] Broken-fixtures suite exists; all diagnostics snapshot-fixed with insta.
- [ ] The five decisions above are implemented and documented in rustdoc with a
      one-line rationale each.

## References

- inka-concept.md §2.4 (diagnostics), §2.5 (values), §12 (syntax)
- inka-implementation-roadmap.md M1
- fixtures READMEs (dogfood / aocs) — open questions feeding this issue