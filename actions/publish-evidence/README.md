# Publish inka evidence

Linux composite Action that validates a generated v2 bundle with the source
runner's inka CLI and transports it using the official upload-artifact Action.
It does not select or execute tests, build language environments, or approve binds.

After your source workflow's preparation, tests and finish steps:

```yaml
- name: Publish evidence from this execution
  if: ${{ always() && steps.finish.outcome != 'skipped' }}
  uses: ./inka/actions/publish-evidence
  with:
    bundle: .evidence/run-123-1/evidence.json
    inka: /path/to/inka
    name: inka-evidence-${{ github.run_id }}-${{ github.run_attempt }}-linux
```

`./inka` is a checkout of the inka tool repository at a reviewed commit. For
remote usage, pin `ut-issl/inka/actions/publish-evidence@<reviewed-commit>`.
The CLI must already be available, as it is also needed by the producer steps.
Initial source distribution can build it with `cargo build -p inka-cli --locked`;
binary release installation can be added separately.

Use fresh producer output paths for every attempt. Preserve the test command's
failure status while still running finish/publish. A valid fail/skip/missing
result bundle is publishable; malformed or missing input makes this Action fail.
Do not make tests green by ignoring the test command's exit status. No evidence
is produced if preparation failed. The artifact name is mandatory: give matrix
entries distinct names and select an explicit run/attempt when importing.

The Action copies exactly one file into a new runner-temporary directory,
validates that copy, then uploads only `evidence.json`. It publishes the artifact
ID and digest returned by upload-artifact. Existing artifacts are not overwritten.
The Action does not sign the producer's claims or establish upstream provenance;
the receiving workflow must trust and explicitly select the source/run/artifact.
Bundle metadata and optional lockfile/environment records are not proof that all
possible execution dependencies were frozen (#39).

Run local validation tests after building inka:
`python3 -m unittest discover -s actions/publish-evidence/tests -v`.
The artifact service itself is exercised by the dogfood workflow (#75).
