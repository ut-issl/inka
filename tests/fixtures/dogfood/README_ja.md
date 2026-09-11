# dogfood: inka 自身の設計論証

inka 自身の設計主張を inka DSL で書いたもの(ut-issl/inka#6)。
標準サンプルプロジェクトであり、PoC([docs/ja/design/poc-plan.md](../../docs/ja/design/poc-plan.md))の主フィクスチャ。
ゴールのループ — 書く → check → approve → eval → 編集 → stale → 再承認 — は
このプロジェクトに対して回す。

英語版(正): [README.md](README.md)

## ファイル構成

| ファイル | 内容 |
| --- | --- |
| `inka.toml` | プロジェクト定義: フェーズの全順序と現在値、値ファイルの指定 |
| `values.toml` | description の `{$var}` から参照される値 |
| `argument/` | 論証本体。規約で配下の `*.inka` がすべて読まれる |
| `argument/objective.inka` | ルート: 4本の枝への分解 |
| `argument/editing.inka` | 枝1: 編集コストが界面の差分に比例する(§3, §11) |
| `argument/approval.inka` | 枝2: 承認が儀式化しない(§4, §5, §7) |
| `argument/audit.inka` | 枝3: 記録の監査可能性(§9)。`{$sig_scheme}` の値埋め込み例を含む |
| `argument/truth-validity.inka` | 枝4: 真実性と妥当性の分離(§6) |
| `argument/assumptions.inka` | 環境仮定(ただの fiat 命題) |

## 使っている DSL 機能

複数ファイル分割と跨ぎ参照、DAG 共有(`local_view_hash_sound` は3親)、期限注釈
(`until:` / `promote:`)、markdown を含む description(`labels_cover_reality`)、
値埋め込み(`approver_unforgeable`)。

最初の CI bind は `normalization_idempotent` です。
選択した生成入力に対する正規化の冪等性に主張を限定し、`hash_props.rs` の
関数本文と実行結果を証拠にします。広い主張 `local_view_hash_sound` は fiat のままです。

[dogfood workflow](../../../.github/workflows/dogfood-evidence.yaml) が証拠元で
ファイル・関数の本文を記録して8件の proptest を実行し、公式 Artifact Actions で
配送・取り込みを行います。取り込む `evidence/ci.json` は Git の管理外です。
証拠がなければ missing evidence、pass の証拠があっても未承認なら未成立です。
workflow は人間の承認を生成しません。

CLI 結合テストは一時コピーに明示的な合成承認を置き、実テストからの証拠生成・
成立・変更による失効・再承認と、不成立結果・入力エラーを確認します。
他の機械検証に昇格予定の主張は、期限付き fiat として残しています。

## 設計の正はどこにあるか

この README は設計を再述しない。参照先は [docs/ja/design/](../../docs/ja/design/):

- [concept.md](../../docs/ja/design/concept.md) — inka とは何か
- [spec.md](../../docs/ja/design/spec.md) — 確定仕様
- [poc-plan.md](../../docs/ja/design/poc-plan.md) /
  [full-plan.md](../../docs/ja/design/full-plan.md) — 実装方針
- [open-questions.md](../../docs/ja/design/open-questions.md) — 未決事項
