# Instructions for coding agents

`CLAUDE.md` is a symbolic link to this file.

## Language

- Reply to the person you are working with in Japanese, including the final summary of a task. Do not switch to English
  because the work before the reply was in English.
- Write everything that goes into the repository or onto GitHub in English: code, comments, commit messages, and the
  titles and descriptions of pull requests.
- The `*.ja.md` files are the Japanese counterparts of the English documents next to them. Change both in the same pull
  request.

## Pull requests

- Name the branch `<type>/<short-description>`, such as `feat/review-colour-output`, with the type of the title below.
  Put the issue number first when there is one, as in `feat/58-dotted-value-paths`. A release is prepared on
  `release/v<version>`.
- Write the title as a [Conventional Commit](https://www.conventionalcommits.org): `<type>(<scope>): <summary>`, such as
  `feat(cli): colour the review output`. A pull request is squashed when it is merged, so its title becomes the subject
  of the commit on `main`, and `scripts/release.py prepare` drafts the release notes from those subjects.
- End the title with the issue the pull request closes, as in `fix(core): keep the order (#58)`.
- Fill in every section of `.github/PULL_REQUEST_TEMPLATE.md` in the description.
- Do not open a pull request that is known to fail its checks. Run the checks that cover the change before pushing.
  For a release, rewrite the drafted `CHANGELOG.md` and `CHANGELOG.ja.md` sections first:
  `python3 scripts/release.py check` fails while a draft is left.
- `scripts/release.py prepare` commits and pushes nothing. Follow [RELEASING.md](RELEASING.md) for the rest of a
  release.
