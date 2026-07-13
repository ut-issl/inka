# repository-template

ISSL organization 向けの汎用リポジトリテンプレートです。

## エージェントによるセットアップ支援

[Agent Skills](https://agentskills.io) に対応した coding agent（Codex や Claude Code など）を使っている場合，
`repo-setup` skill が以下のセットアップ作業全体を対話的に案内します。
Codex では `$repo-setup in Japanese`，Claude Code では `/repo-setup in Japanese` で呼び出せます。

## 【必須】CODEOWNERS の設定

このテンプレートからリポジトリを作成したら，まず [.github/CODEOWNERS](.github/CODEOWNERS) を更新してください。

- 既定の `*` 行のコメントを外し，`@your-username` を実際のオーナー（実在するユーザー `@person` またはチーム `@ut-issl/<team-slug>`）に置き換える
- 必要に応じて，以下の例のようにパスごとの上書きを追加する

  ```text
  .github/        @infra-manager
  docs/           @docs-manager
  ```

詳しい構文は [GitHub ドキュメント](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-code-owners)
を参照してください。

## pre-commit のセットアップ

このテンプレートでは，[pre-commit](https://pre-commit.com) と互換性のある [prek](https://prek.j178.dev) を推奨しています。

```console
prek install --hook-type pre-commit --hook-type pre-push
```

`pre-commit` を使う場合は，上のコマンドの `prek` を `pre-commit` に置き換えてください。

## 任意で有効化できる機能

デフォルトでは，PR 上で実行されるのは `lint-gh-actions`（GitHub Actions workflow の lint）だけです。
その他の機能は無効化されているため，必要に応じて以下から有効化してください。

### 追加の CI ジョブ

以下のジョブは [ci.yaml](.github/workflows/ci.yaml) でコメントアウトされています。
対応するブロックのコメントを外すと有効になります。
各ジョブは関連するファイルが変更されたときにのみ実行されます。

- [`validate-renovate-config`](.github/workflows/ci.yaml#L119-L144)：Renovate 設定の検証
- [`lint-markdown`](.github/workflows/ci.yaml#L146-L192)：Markdown ファイルの lint
- [`lint-json5`](.github/workflows/ci.yaml#L194-L231)：JSON5 ファイルの lint
- [`lint-toml`](.github/workflows/ci.yaml#L233-L307)：TOML ファイルの lint と format
- [`lint-yaml`](.github/workflows/ci.yaml#L309-L344)：YAML ファイルの lint
- [`check-prek`](.github/workflows/ci.yaml#L346-L361)：prek による pre-commit フックの実行（ファイル変更に関わらず常に実行）
- [`check-typos`](.github/workflows/ci.yaml#L363-L389)：タイポの検出

### Conventional Commits の強制（Commitizen）

[commitizen](https://github.com/commitizen-tools/commitizen) を使って，コミットメッセージと PR タイトルに
[Conventional Commits](https://www.conventionalcommits.org) を適用します。
有効化すると，すべてのコミットと PR タイトルが仕様に従っている必要があります。
PR タイトルの lint は，squash merge を使う場合に特に有用です
（デフォルトでは，PR タイトルが squash 後のコミットの subject になるため）。

以下の 3 つのブロックのコメントを外してください。

- [`lint-commit-messages`（ci.yaml 内）](.github/workflows/ci.yaml#L391-L423)
- [`lint-pr-title`（manage-pull-requests.yaml 内）](.github/workflows/manage-pull-requests.yaml#L28-L44)
- [`commitizen` の repo ブロック（.pre-commit-config.yaml 内）](.pre-commit-config.yaml#L55-L61)

`commitizen` hook は `commit-msg` stage で実行されますが，
この stage は前述の pre-commit セットアップのコマンドではインストールされません。
次のコマンドで追加インストールしてください。

```console
prek install --hook-type commit-msg
```

Conventional Commits に従ったコミットメッセージを対話的に作成するには，次のコマンドを使います。

```console
uv tool install commitizen
cz commit
```

### Renovate

[Renovate](https://docs.renovatebot.com) は [.github/renovate.json5](.github/renovate.json5) で事前設定されており，
Action の SHA，[ci.yaml](.github/workflows/ci.yaml) 内で固定しているツールバージョン，pre-commit フックを追跡するようになっています。
有効化するには，[renovate.json5#L3](.github/renovate.json5#L3) の `enabled: false,` 行を削除する
（または `true` に変更する）とともに，Renovate GitHub App がリポジトリにインストールされていることを確認してください。
