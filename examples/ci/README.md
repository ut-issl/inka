# Explicit cross-repository evidence import

Copy [receive-evidence.yaml](receive-evidence.yaml) into the receiving repository's
`.github/workflows/`. Its root is an inka project; its source can be any separate
repository using the producer IF and publish Action. Review and update the pinned
inka tool commit when adopting this example.

Configure the receiver:

```toml
[evidence.sources.upstream]
id = "github:example/source"
file = "evidence/upstream.json"
```

````inka
bind tested {
  ```
  The selected test cases support this explicitly scoped claim
  ```
  ci: "upstream/selected-target"
}
````

The source supplies the selection plan, source revision, execution ID/attempt,
test environment and generated bundle. The receiver does not check out, install
or execute that source. It checks out its own argument and the inka tool only.

Create `SOURCE_ACTIONS_READ_TOKEN` in the receiving repository with read access
to Actions artifacts in the chosen source repository (for example a fine-grained
token restricted to that repository). A receiver's ordinary `GITHUB_TOKEN` does
not automatically authorize reading a different private repository. Do not pass
the token to inka; the official download Action alone handles that access.

Dispatch with the exact source `owner/repo`, run ID and artifact ID returned by
the publish Action. Choose the artifact from the intended attempt and matrix
entry; the example intentionally does not search for the latest successful run,
merge unrelated artifacts or mix retries. Read the bundle's source revision and
execution metadata as part of selecting/reviewing it. Selecting a workflow from
an untrusted fork does not make its claims trustworthy.

The manifest's source ID must match the bundle. Unknown targets or missing
evidence leave the bind unestablished; malformed evidence fails the command.
A passing result still needs an approval of the current bind view. Changed
contract, identity or required-case selection invalidates the old approval.

The downloaded version is installed only in that workflow's workspace. Updating
a checked-in artifact, opening a PR with the import or following upstream updates
can be added independently; this example neither commits files nor grants
approvals. A new upstream revision alone cannot affect an offline evaluation of
an already imported bundle.

Local integration coverage uses separate producer and receiver directories,
generates a real bundle through prepare/finish, imports it, evaluates it with
synthetic test approvals, then checks nonpass results and reapproval after a
changed source contract. Live cross-repository deployment requires a configured
receiving repository and token; local tests do not claim to exercise GitHub's
artifact service. The dogfood workflow covers upload/download inside this repo.
