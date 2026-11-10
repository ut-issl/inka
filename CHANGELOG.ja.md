# 変更履歴

inka のリリースノートです([English](CHANGELOG.md))。
各 `## v<version>` 節は `CHANGELOG.md` の同じ節の日本語版で，GitHub Release の本文には英語版を使います。
GitHub Release では改行がそのまま残るので，1 文ごとに改行してください。
リリースの手順は [RELEASING.ja.md](RELEASING.ja.md) を参照してください。

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
