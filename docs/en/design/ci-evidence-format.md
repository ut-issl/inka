# Test evidence bundle v2

The [integration design](test-evidence-integration.md) defines automatic catalog publication. The wire format
is independent of report collection. Versioned digest identifiers and frozen vectors remain stable across
terminology changes.

This wire contract implements the [evidence semantics](ci-evidence.md).
Japanese translation: [日本語版](../../ja/design/ci-evidence-format.md).
The JSON Schema is [ci-evidence-v2.schema.json](../../../schemas/ci-evidence-v2.schema.json).
Shared [conformance vectors](../../../tests/golden/evidence/v2/README.md) are
append-only. This format uses v2; the file-hash-only v1 export is not accepted
as an implicit source of v2 evidence.

## Bundle

One bundle describes one source revision and one execution attempt. It contains
`v: 2`, `source: {id, revision}`, `execution: {id, attempt}`, and `targets`.
`attempt` is a positive u32. Optional `environment` is an object of descriptive
JSON metadata and has no initial effect on establishment.

Each target has:

| Field | Meaning |
| --- | --- |
| `id` | Opaque identity unique within this source |
| `kind` | `file` or `function` |
| `profile` | Versioned extraction rule, such as `file-source:v1` |
| `content` | Nonempty UTF-8 reviewable source content |
| `interface_hash` | Lowercase `sha256:` plus 64 hexadecimal digits |
| `required` | Nonempty, unique set of required execution IDs |
| `results` | Records `{case, result}` for required execution IDs |

`result` is `pass`, `fail`, `skip` or `cancelled`. An absent result means a
required execution has not been reported. Case IDs are opaque strings that
include an environment discriminator when multiple environments are required,
for example `linux/normalization_is_idempotent`. Producers must declare that
selection independently of which results passed.

All identity/revision/profile/case strings must contain non-whitespace content.
Fields other than arbitrary members of `environment` are closed: reject unknown
fields and duplicate object keys. Target IDs, required IDs and result case IDs
must be unique in their respective scopes. Results for undeclared cases are
invalid. Empty `targets` is permitted; it cannot establish a referenced target.

The schema checks structure. Consumers additionally enforce uniqueness by ID,
result membership, version support and recomputed interface hashes. A structural
or hash inconsistency aborts evaluation; valid missing/non-passing evidence is
bind blame. Missing bundle files are missing evidence; unreadable or malformed
existing files are input errors.

## Automatic target identity

Automatic collection derives identity from source location and test names, never
line numbers, collection order, checkout directory, revision or execution ID.
The source ID is a stable configured identity; GitHub integrations can use
`github:owner/repository`. It must match the receiving manifest.

A file target ID is its repository-relative path, with `/` separators and no
leading slash, `.` or `..` components. A function target appends `::` followed
by its qualified declaration names (module, class, nested suite and test name
as applicable), separated by `::`. Names must identify the extracted declaration,
not an individual parameterized invocation.

Encode each path/name component from UTF-8 bytes: preserve ASCII letters, digits,
`-`, `.`, `_` and `~`; encode other bytes as `%HH` with uppercase hexadecimal.
Only structural path separators and qualification separators remain unescaped.
Do not case-fold or Unicode-normalize names. Thus a literal `::` in a test name
cannot collide with qualification. The consumer still treats IDs as opaque;
this convention does not invalidate explicitly named v2 targets.

| Target | Automatic ID |
| --- | --- |
| File | `tests/test_math.py` |
| Function | `tests/test_math.py::test_add` |
| Method | `tests/test_math.py::TestMath::test_add` |
| Test name containing a separator | `tests/math.test.ts::adds%3A%3Avalues` |

All collected parameterized cases of one declaration belong to one function
target. Its required set contains their distinct runner case identities and
execution-environment distinctions. A file target requires all collected cases
associated with that file. Case identities must be stable across reordering;
collectors preserve meaningful runner parameter IDs rather than inventing
position-based IDs. The framework integration defines its case encoding and
runner scope. Ambiguous declaration mappings or duplicate case identities are
errors, not permission to merge or silently omit tests.

Adding an unrelated target leaves existing target hashes unchanged. Adding or
removing cases of a function changes its required set and requires reapproval;
adding a test in a file also changes that file target. Moving a file or renaming
a declaration changes identity and requires reapproval even with identical
content. A parameter ID change likewise changes the required set. A generated
runner ID that varies across runs therefore cannot promise stable approval.

## Interface digest

Define `field(s)` as u64 little-endian UTF-8 byte length followed by those bytes.
Compute SHA-256 over this exact concatenation:

```text
field("inka:ci-interface:v2")
field(source.id)
field(target.id)
field(target.kind)
field(target.profile)
field(target.content with CRLF replaced by LF)
u64-LE(count of required IDs)
field(required ID 1) ... field(required ID N)
```

Required IDs are sorted by UTF-8 bytes. Their input order is not significant;
duplicates are invalid. Including the required set means that weakening test
selection requires fresh approval even when the source body is unchanged.

Source identity, target identity, kind, profile, source content and the required
set affect approval. Revision, run/attempt, results, and environment metadata do
not. The resulting raw 32-byte digest enters the existing bind v1 interface slot.
No existing bind/record golden vectors change.

## Local references

The manifest accepts an optional map of named evidence sources:

```toml
[evidence.sources.upstream]
id = "github:example/source"
file = "evidence/upstream.json"
```

An alias uses the DSL identifier grammar. `file` is relative to the manifest
directory; it names imported data, not executable source. Configure one selected
bundle per source identity; duplicate source identities under different aliases
are rejected to avoid competing imported revisions. The bundle's `source.id`
must match the configured identity.

A bind selects `<alias>/<target-id>`:

````text
bind arithmetic {
  ```
  The arithmetic property is checked by the selected test
  ```
  test: "upstream/tests/example.rs::works"
}
````

Split at the first slash; everything after it is the opaque target ID, including
any further slashes. An invalid reference, unknown alias, missing imported bundle
or missing target leaves that bind unestablished with a specific blame reason.
Structural `check` continues to treat the reference as opaque.

The local alias is routing configuration, not the source identity included in
the hash. Alias-only changes can preserve approval; source/target changes cannot.
Inka's own HEAD is never compared with the evidence source revision. The producer
and import procedure select the revision/attempt; eval uses only that selection.

## Result aggregation

After validating the bundle, evaluate each target's required cases. Report all
non-passing/missing cases. A target passes only if every required case has exactly
one `pass` result. Do not infer success from an empty result array or the overall
workflow status, and do not combine results from different attempts implicitly.

Closure metadata and environment policies require later explicit extensions.
Unknown future fields are rejected rather than silently losing their semantics.
