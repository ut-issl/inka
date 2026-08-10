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

機械検証に昇格予定の主張はすべて `fiat` ブロック+`until: <マイルストーン>` /
`promote: bind` — concept §2.1 の fiat→bind ライフサイクルの可視化。最初の本物の
bind は CI provider(対象は `local_view_hash_sound`)で入る。

## 設計の正はどこにあるか

この README は設計を再述しない。参照先は [docs/ja/design/](../../docs/ja/design/):

- [concept.md](../../docs/ja/design/concept.md) — inka とは何か
- [spec.md](../../docs/ja/design/spec.md) — 確定仕様
- [poc-plan.md](../../docs/ja/design/poc-plan.md) /
  [full-plan.md](../../docs/ja/design/full-plan.md) — 実装方針
- [open-questions.md](../../docs/ja/design/open-questions.md) — 未決事項
