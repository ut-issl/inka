# inka

Structured argumentation with accountable approvals. Text-based, LSP-powered, built for iterative development.

## Pre-commit Setup

This repository uses [prek](https://prek.j178.dev), a faster drop-in replacement for [pre-commit](https://pre-commit.com).

```console
prek install --hook-type pre-commit --hook-type pre-push
```

If you prefer `pre-commit`, substitute `pre-commit` for `prek` in the command above.
