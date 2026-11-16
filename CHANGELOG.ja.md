# 変更履歴

inka のリリースノートです([English](CHANGELOG.md))。
各 `## v<version>` 節は `CHANGELOG.md` の同じ節の日本語版で，GitHub Release の本文には英語版を使います。
GitHub Release では改行がそのまま残るので，1 文ごとに改行してください。
リリースの手順は [RELEASING.ja.md](RELEASING.ja.md) を参照してください。

## v0.0.5

inka 自体には変更がなく，リリースワークフローによるクレートの公開を直すリリースです。

### 配布

- v0.0.4 では公開に失敗したクレートを crates.io に公開するので，`cargo install inka-cli` でもビューアが組み込まれます。
- リリースワークフローの dry run で，crates.io への公開を `cargo publish --dry-run` で予行するようになりました。

## v0.0.4

グラフビューアを `inka` に組み込み，ほかに何もダウンロードせずに `inka serve` を使えるようにするリリースです。

### コマンドライン(`inka`)

- `inka serve` はバイナリに組み込まれたグラフビューアを配信するので，`--assets` は不要になりました。
- `--assets <DIR>` は，ビューア自体の開発のために，引き続きディスク上のビューアのバンドルを配信します。
- `cargo install inka-cli` でインストールした `inka` にもビューアが組み込まれます。

### 配布

- web ビューアのバンドル `inka-web-<version>.tar.gz` は GitHub Release に添付しなくなりました。
- npm パッケージは，リリースワークフローが npm にステージし，メンテナが承認した時点で公開されます。
- クレートは crates.io に公開されませんでした。v0.0.5 で公開します。

## v0.0.3

inka 自体には変更がなく，リリースワークフローによるパッケージの公開を直すリリースです。

### 配布

- v0.0.2 では公開に失敗した npm パッケージ `@inka-project/inka-evidence` を npm に公開します。
- Open VSX への公開は，`OVSX_PAT` シークレットが設定されていないとき，トークンの入力を待たずにすぐエラーで失敗するようになりました。

## v0.0.2

`inka init` を追加し，初めて各パッケージをレジストリに公開するリリースです。

### コマンドライン(`inka`)

- `inka init [PATH]` は新しいプロジェクトの雛形として，`inka.toml`，空の `values.toml`，最初の論証を書き出します。
- 書き出したプロジェクトは，そのまま `inka check` と `inka fmt --check` を通ります。
- プロジェクト名の既定はディレクトリ名で，`--name` で変えられます。
- `--phases` は `phases.order` を設定し，その最初の項目が `phases.current` になります(既定は `M0`)。
- `inka init` はファイルを追加するだけで，既存のプロジェクトを上書きすることはありません。

### 配布

- GitHub Release に，x86_64 と aarch64 の Linux(musl)，x86_64 と aarch64 の macOS，x86_64 の Windows 向けのビルド済みバイナリを添付するようになりました。
- プラットフォームごとの VS Code 拡張，Python の wheel と sdist，npm パッケージ，`inka serve --assets` 用の web ビューアのバンドルも添付します。
- すべての成果物にビルド来歴の attestation が付き，`SHA256SUMS` に載ります。
- クレートを crates.io に，`inka-evidence` を PyPI に，`@inka-project/inka-evidence` を npm に公開します。
- VS Code 拡張を `inka-project.inka-vscode` として VS Code Marketplace と Open VSX に公開します。
- VS Code 拡張は `inka-lsp` を同梱し，`PATH` 上のものより先にそれを使います。

### 互換性を壊す変更

- npm パッケージの名前を `@ut-issl/inka-evidence` から `@inka-project/inka-evidence` に変えました。

## v0.0.1

inka の最初のリリースです。inka は，設計の論証を構造化された査読可能な主張として書くためのテキスト DSL です。
リポジトリ内のすべてのパッケージを，このバージョンでまとめてリリースします。

### コマンドライン(`inka`)

- `inka check` はプロジェクトを読み込み，論証の構造上の問題をすべて報告します。
- `inka eval` は論証を評価し，現在成り立っているか，成り立っていないならその理由を示します。
- `inka review` は承認対象を一覧し，対話的に順に確認するか，1 件の判断を記録します。
- `inka fmt` はすべての論証ファイルを正規の書式に書き直します。`--check` では書き直すファイルを一覧するだけです。
- `inka export` は論証を cytoscape の `elements` JSON として出力し，`inka serve` は読み取り専用のグラフビューアを配信します。

### エディタ対応

- `inka-lsp` は診断，ホバー，定義へ移動，参照の検索，リネーム，セマンティックトークンを提供します。
- インレイヒント，承認状態のコードレンズ，ドキュメントのフォーマットも提供します。
- `editors/vscode` の VS Code 拡張は言語を登録し，サーバーを起動します。

### テストエビデンス

- テストエビデンス v1 は，論証から引用できるテスト実行結果の JSON スキーマです。
- `inka-evidence-rust` は cargo-nextest の実行結果からこれを出力します。
- Python パッケージ `inka-evidence` はこれを出力し，pytest プラグインを含みます。
- npm パッケージ `@ut-issl/inka-evidence` はこれを出力し，Vitest アダプタを含みます。

各パッケージは crates.io・PyPI・npm・VS Code Marketplace にはまだ公開していません。
このリポジトリからビルドしてください。
