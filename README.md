# repository-template

General-purpose repository template for the ISSL organization.

## Agent-Assisted Setup

If you use a coding agent that supports [Agent Skills](https://agentskills.io) (e.g. Codex or Claude Code),
the `repo-setup` skill walks you through all of the setup below interactively:
invoke it with `$repo-setup` in Codex or `/repo-setup` in Claude Code.

## [Required] Configure CODEOWNERS

You must update [.github/CODEOWNERS](.github/CODEOWNERS) right after creating a repository from this template.

- Uncomment the default `*` line and replace `@your-username` with the actual owner —
  a real user (`@person`) or a team (`@ut-issl/<team-slug>`).
- Add path-specific overrides as needed, for example:

  ```text
  .github/        @infra-manager
  docs/           @docs-manager
  ```

See the [GitHub docs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners)
for the full syntax.

## Pre-commit Setup

This template uses [prek](https://prek.j178.dev), a faster drop-in replacement for [pre-commit](https://pre-commit.com).

```console
prek install --hook-type pre-commit --hook-type pre-push
```

If you prefer `pre-commit`, substitute `pre-commit` for `prek` in the command above.

## Opt-in Features

By default only `lint-gh-actions` (GitHub Actions workflow lint) runs on PRs; everything else is disabled.
Enable any of the below if you want them.

### Additional CI Jobs

The following jobs are commented out in [ci.yaml](.github/workflows/ci.yaml).
Uncomment the corresponding block to enable each one.
Each job runs only when the relevant files change.

- [`validate-renovate-config`](.github/workflows/ci.yaml#L119-L144) — validate the Renovate config
- [`lint-markdown`](.github/workflows/ci.yaml#L146-L192) — lint Markdown files
- [`lint-json5`](.github/workflows/ci.yaml#L194-L231) — lint JSON5 files
- [`lint-toml`](.github/workflows/ci.yaml#L233-L307) — lint and format TOML files
- [`lint-yaml`](.github/workflows/ci.yaml#L309-L344) — lint YAML files
- [`check-prek`](.github/workflows/ci.yaml#L346-L361) — run the pre-commit hooks via prek (always runs, not file-gated)
- [`check-typos`](.github/workflows/ci.yaml#L363-L389) — check for typos

### Conventional Commits Enforcement (Commitizen)

Enforces [Conventional Commits](https://www.conventionalcommits.org) on commit messages and PR titles
via [commitizen](https://github.com/commitizen-tools/commitizen).
Once enabled, all commits and PR titles must follow the spec.
Linting the PR title is especially useful with squash merging,
since the PR title becomes the subject of the squashed commit by default.

Uncomment all three blocks:

- [`lint-commit-messages` in ci.yaml](.github/workflows/ci.yaml#L391-L423)
- [`lint-pr-title` in manage-pull-requests.yaml](.github/workflows/manage-pull-requests.yaml#L28-L44)
- [the `commitizen` repo block in .pre-commit-config.yaml](.pre-commit-config.yaml#L55-L61)

The `commitizen` hook runs at the `commit-msg` stage,
which the install command in the pre-commit setup section above does not cover.
Install it additionally:

```console
prek install --hook-type commit-msg
```

To author Conventional Commits interactively:

```console
uv tool install commitizen
cz commit
```

### Renovate

[Renovate](https://docs.renovatebot.com) is preconfigured in [.github/renovate.json5](.github/renovate.json5)
to track Action SHAs, pinned tool versions inside [ci.yaml](.github/workflows/ci.yaml), and pre-commit hooks.
To opt in, remove the `enabled: false,` line at [renovate.json5#L3](.github/renovate.json5#L3)
(or change it to `true`) and make sure the Renovate GitHub App is installed for the repository.
