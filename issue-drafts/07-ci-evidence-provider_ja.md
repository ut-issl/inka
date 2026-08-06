# bind: CI(テストターゲット)evidence provider の実装

- **ラベル:** enhancement
- **マイルストーン:** M8 周辺 — veriq アダプタより**先**に実装する有力候補(最初の実
  bind provider。仮定の集約が不要なため veriq より単純)。
- **依存:** bind をスタブにした `inka eval`(07-eval)、`inka check`(03/06)。
- **実装ガイド:** `docs/poc-implementation/08-ci-provider.md`。

## 動機

DSL は既に2つの evidence プロパティ——`veriq:`(Python calc 参照)と `ci:`(テスト
ターゲット)——を区別する。その記法・provider 契約・`ci-export.json` v1 形式・eval 側
照合は `spec.md` の *CI evidence provider* で確定済み。欠けているのは `ci:` provider の
**実装**である。

`ci:` はテストを機械証拠として立てる: 人間の bind 承認が「このテストはこの命題の証拠
として適切」を表明し、CI が真実性の側(このコミットで pass するか)を供給する。
ドッグフーディング上の見返り: 現在 `fiat until:M2 promote:bind` の命題(例
`local_view_hash_sound`)を、**inka 自身の CI で走る本物の proptest に bind** できる。

## spec で既に確定していること(再決定しない)

- **粒度:** テストターゲット(テストファイル1つ)。`ci: "tests/hash_props.rs"` で参照。
- **provider 契約:** 内容アドレスの interface hash、pass が何を立証するかの人間可読な
  言明、コミットに束縛された結果を「リポジトリの内容」として届けること(eval は外部
  API を呼ばない)、環境ハッシュ。
- **`ci-export.json` v1:** `commit`、`environment.lockfile_hash`、`targets[]`
  = `{ id, interface_hash, result }`。`interface_hash` はテストファイルの内容ハッシュ
  (CRLF→LF 正規化のみ)、`result` は `pass` | `fail`。
- **eval 照合:** export に `<id>` のエントリがある/現在のワークツリーのファイルの
  interface hash が export と一致/bind 承認の `view_hash` が現在の description +
  interface hash で再計算した値と一致/`result` が pass。コミット不一致は warning
  (内容の一致は interface hash が担保)。

## スコープ(実装)

- **`inka ci-export` サブコマンド** — 各ターゲットの `interface_hash`(ファイル内容、
  CRLF→LF)を計算し、現在のコミットの `ci-export.json` を書く。これが provider の
  「結果をリポジトリ内容として届ける」部分。
- **GitHub Actions ジョブ** — テストスイートを実行し、対象コミットごとに export を
  コミット(または保存)する。
- **eval 側 `ci:` 照合** — 上記の照合を実装。陳腐化・不一致の export は bind を単に
  established にしない(明確な blame 付き)。
- **dogfood 配線** — dogfood の命題1つ(例 `local_view_hash_sound`)を `fiat` から
  `bind { ci: "..." }` に切り替え、本物の proptest に対して end-to-end で established に。

## export 例(`ci-export.json` v1)

```json
{
  "commit": "abc123...",
  "environment": { "lockfile_hash": "sha256:..." },
  "targets": [
    {
      "id": "tests/hash_props.rs",
      "interface_hash": "sha256:...",
      "result": "pass"
    }
  ]
}
```

人間可読な言明は bind ノードの description が担い、export は同一性+結果のみを持つ
(ターゲットごとの statement フィールドは持たない)。

## スコープ外

- **有効内容(closure)の変更検出** — ターゲットファイル自体の内容ハッシュが一致した
  ままヘルパが変わるケース。意図的に別 Issue(ドラフト08)へ分離: `veriq:` と `ci:` に
  等しく適用され、bind の束縛先には**入れてはならない**(失効爆発のリスク)。

## 完全性の強化パス(後日)

- GitHub OIDC / Sigstore(`actions/attest`)による CI 署名 attestation。
  `ci:github-actions` を非人間 attestor(`llm:<model>` の前例)と同格にする。初版では
  スコープ外。

## 受け入れ条件

- [ ] `inka ci-export` が実ターゲットに対し spec 形式の `ci-export.json` を生成する。
- [ ] GitHub Actions ジョブが inka 自身の CI で export を生成する。
- [ ] eval 側 `ci:` 照合を実装し、陳腐化・不一致に blame を出す。
- [ ] dogfood の命題1つ(例 `local_view_hash_sound`)が本物の proptest に bind され、
      end-to-end で established になる。
- [ ] bind されたテストファイルの内容を編集すると bind 承認が失効する。

## 参照

- `docs/en/design/spec.md` → *CI evidence provider (test-target granularity)*
- `docs/en/design/poc-plan.md`(CI evidence provider の項)
- `docs/poc-implementation/08-ci-provider.md`(実装ガイド)