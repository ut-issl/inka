---
name: repo-setup
description: >-
  Interactive initial setup of a repository newly created from this template.
  Use when the user asks to set up, initialize, or bootstrap the repository right after creating it from the template,
  or proactively when the repository is still in its template state:
  README.md still carries the template's setup instructions and .github/CODEOWNERS has no active owner line.
  Do not use for routine maintenance of an already-configured repository.
---

# Initial Repository Setup

Guide the user through the initial setup of a repository created from this template.

First check that the repository is actually still in its template state:
`.github/CODEOWNERS` contains only the commented `# * @your-username` placeholder (no active owner line),
and `README.md` still carries the template's setup instructions.
If setup appears to be done already, say so and stop instead of redoing it.
If this skill was triggered proactively rather than by an explicit request,
confirm with the user that they want to run the initial setup now before touching anything.

Follow the steps below in order, one step at a time.
Before each step, briefly explain what it does, then ask the user how to proceed.
Never enable an opt-in feature without an explicit yes from the user; if the user declines a step, skip it and move on.
Converse in the language the user writes in, but keep all edits (comments, commit messages, etc.) in English.

## 1. Install pre-commit hooks

Ask the user whether to install the git hooks now.
If yes, run:

```console
prek install --hook-type pre-commit --hook-type pre-push
```

If `prek` is not on PATH, run the same command via uv instead: `uvx prek install ...`.
If `uv` is not available either, ask the user to install
[uv](https://docs.astral.sh/uv/) or [prek](https://prek.j178.dev) and retry once they have;
if they prefer not to, skip this step and continue.

## 2. Configure CODEOWNERS (required)

Ask the user who owns this repository: a real user (`@person`) or a team (`@ut-issl/<team-slug>`).

Then edit `.github/CODEOWNERS`:

- Replace the commented `# * @your-username` line with `* <owner>` using the user's answer.
- Remove the setup instruction comments, keeping only the comment that describes the default owner line.

Finally, ask whether the user wants path-specific overrides (e.g. `.github/ @infra-manager`) and append any they provide.

## 3. Enable additional CI jobs (opt-in)

The following jobs are commented out in `.github/workflows/ci.yaml`.
Ask the user whether to enable each one — one question per job,
or a single multi-select question if the interface supports it:

- `validate-renovate-config` — validate the Renovate config
- `lint-markdown` — lint Markdown files
- `lint-json5` — lint JSON5 files
- `lint-toml` — lint and format TOML files
- `lint-yaml` — lint YAML files
- `check-prek` — run the pre-commit hooks via prek (always runs, not file-gated)
- `check-typos` — check for typos

To enable a job, uncomment its entire block in `ci.yaml`
(remove the leading `#` and the space after it from every line, preserving indentation).
Leave `lint-commit-messages` alone here; it belongs to the next step.

## 4. Enforce Conventional Commits with Commitizen (opt-in)

Explain: this enforces [Conventional Commits](https://www.conventionalcommits.org) on commit messages and PR titles.
Linting the PR title is especially useful with squash merging, since the PR title becomes the squashed commit subject.

If the user opts in, uncomment all of the following blocks:

- the `lint-commit-messages` job in `.github/workflows/ci.yaml`
- the `lint-pr-title` job in `.github/workflows/manage-pull-requests.yaml`
- the `commitizen` repo block in `.pre-commit-config.yaml`

The `commitizen` hook runs at the `commit-msg` stage, which step 1 does not install.
Install it additionally, with the same fallback rules as step 1:

```console
prek install --hook-type commit-msg
```

## 5. Enable Renovate (opt-in)

Explain: Renovate is preconfigured in `.github/renovate.json5` to track Action SHAs,
tool versions pinned in `ci.yaml`, and pre-commit hooks.

If the user opts in:

- Delete the `enabled: false,` line (including its trailing comment) from `.github/renovate.json5`.
- Remind the user that the Renovate GitHub App must be installed for this repository to take effect.

## 6. Clean up template scaffolding (opt-in)

Ask whether to remove the template-only material now that setup is complete.
If yes:

- Ask for the project name and a one-line description.
- Rewrite `README.md`: set the title and description to the answers,
  keep the pre-commit setup instructions (future contributors need them),
  and remove the CODEOWNERS, agent-assisted setup, and opt-in feature sections (they are now settled).
- Ask whether the project should keep a Japanese README.
  If yes, apply the same rewrite to `README.ja.md`; if no, delete it.
- Delete this skill: remove the `.agents/skills/repo-setup/` directory and the `.claude/skills/repo-setup` symlink,
  and remove `.agents/` and `.claude/` entirely if they are empty afterwards.

## 7. Wrap up

Show a summary of everything that was changed or skipped.
Offer to run `prek run --all-files` to verify the edited files pass the hooks.
Leave all changes uncommitted; committing and pushing are up to the user unless explicitly requested.
