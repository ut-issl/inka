# ミニマム実装ガイド

PoC([../en/design/poc-plan.md](../en/design/poc-plan.md))のスコープを自力実装するための
ガイド。各ファイルが1機能に対応する。コードの正解を書く場所ではなく、型の形・
アルゴリズム・凍結すべき決定・落とし穴を先に固めておく場所。

| ファイル | 内容 |
|---|---|
| [01-syntax.md](01-syntax.md) | lexer・AST・再帰下降パーサ・エラー回復 |
| [02-project.md](02-project.md) | inka.toml・argument/ 規約・`{$var}` 展開 |
| [03-graph-check.md](03-graph-check.md) | interning・グラフ・診断・`inka check` |
| [04-hash.md](04-hash.md) | 正規化・view_hash 3種・ゴールデンベクタ(凍結対象) |
| [05-record.md](05-record.md) | 承認レコード・正規化バイト列・jsonl(凍結対象) |
| [06-approve-cli.md](06-approve-cli.md) | キュー導出・`inka approve` |
| [07-eval.md](07-eval.md) | established 不動点・stale・blame・`inka eval` |
| [08-ci-provider.md](08-ci-provider.md) | ci-export.json・照合・GitHub Actions |

## 共通方針

- **クレート**: `toml` / `serde` + `serde_json` / `sha2` / `unicode-normalization` /
  `clap`(derive)/ `ariadne`(診断表示)/ `insta` + `proptest`(テスト)。
  lexer は手書き(logos は不要: フェンスと `{$var}` の文脈依存が手書きの方が素直)。
- **純関数の束で書く**: `parse → load → graph → hash → eval` はすべて
  「入力バイト列 → 値」。I/O は cli/ に隔離する。eval に時計・環境変数・ネットワークを
  持ち込まない(timestamp を使うのは approve の記録時のみ)。
- **span**: `struct Span { file: FileId, start: u32, end: u32 }`(バイトオフセット)。
  AST の主要要素は `Spanned<T> { value: T, span: Span }` で包む。
- **診断型は自前で1つ**:
  ```rust
  struct Diagnostic {
      severity: Severity,          // Error | Warning
      code: &'static str,          // "undefined-reference" など kebab-case
      message: String,
      labels: Vec<(Span, String)>, // ariadne のラベルにそのまま変換
  }
  ```
  check も eval の blame も最終的にこれを吐く。ariadne への変換は cli/ 側の1関数。
- **凍結対象**(04, 05)は実装より先にゴールデンベクタを書き、CI で
  `tests/golden/` を追記専用として守る。
- **rustdoc に理由を書く**: 確定済みの設計判断(構文規則、正規化、フレーミング)は
  該当モジュールの rustdoc に1行ずつ理由を残す(仕様書は作らない)。

## 推奨する着手順

依存が薄い順: **04(hash: 文字列→値の純関数でパーサ不要)→ 01 → 02 → 03(ここで
`inka check` が動く)→ 05 → 07 → 06(ここで一周)→ 08(bind の PoC)**。
04 を最初にやるのは、凍結対象を最初に固めるため+最小の Rust ウォームアップになるため。