# inka ミニマム実装スコープ

inka のシステムを一人で体験できる最小の実装範囲を定める。実装順はここでは定めない。
ミニマム達成後の拡張(LSP・署名・veriq 連携・LLM lint・import 等)は
inka-implementation-roadmap.md を参照。

## ゴール

次のループが dogfood プロジェクト(`tests/fixtures/dogfood`)で一周すること:

1. 論証を書く → `inka check` が通る
2. 自分で `inka approve` する → `inka eval` が clean になる
3. 契約(description / values)を編集する → 該当承認だけが stale になり blame に出る
4. 再承認する → clean に戻る
5. **CI bind が1本 established になる**: `local_view_hash_sound` を inka 自身の
   proptest(テストファイル)に bind し、テストファイルを編集すると bind 承認が
   剥がれ、実装だけの変更では剥がれないことを確認する

5 が bind(真実性は機械・妥当性は人間)のコンセプト証明にあたる。

## 機能一覧

### 1. 読み込み

- **lexer + 再帰下降パーサ**(現行構文): `objective` / `structure` / `bind` / `fiat`
  ブロック、`{}`、フェンス付き description(可変長フェンス、インフォ文字列は保持)、
  `children:` / `source:` / `until:` / `promote:`、`//` コメント、タグ受理、span、
  `}` までのエラー回復。確定済みの構文規則(issue-drafts/06)に従う
- **inka.toml**: `[project]`、`[phases] order/current`、`[values] files`
- **argument/ 規約読み込み**: 配下の `*.inka` をすべて読む(ミニマムではフラット
  構成のみ対象。名前空間の解決は範囲外)
- **`{$var}` 展開**: values TOML から解決。未定義変数は error。展開は正規化・
  ハッシュより前段

### 2. 検査 — `inka check`

- グラフ構築(ID interning、隣接マップ、トポソート、到達性)
- error: 未定義の子参照、重複 ID、閉路(経路表示)、未定義 `{$var}`、
  description フェンスの欠落/重複、未知のプロパティ、モードとプロパティの不整合
- warning: objective から到達しない命題
- error があれば非ゼロ終了

### 3. ハッシュ

- 正規化(NFC、CRLF→LF、行末空白・共通インデント・先頭末尾空行の除去、内部は
  verbatim)
- view_hash 3種(ドメイン分離タグ `inka:structure:v1` / `inka:fiat:v1` /
  `inka:bind:v1`):
  - structure: desc + 期限注釈 + 子 desc 集合(ソート・長さ接頭辞)
  - fiat: desc + 期限注釈
  - **bind(ミニマム定義)**: desc + evidence の界面ハッシュ(ci-export の
    `interface_hash`)。総仮定集合は空で開始。言明は bind 側 description が兼ねる
- ゴールデンベクタの最小セット(本物の承認を記録し始める前にハッシュを凍結する)

### 4. 承認レコード

- jsonl スキーマ(issue-drafts/03): `target, view_hash, label, comment, approver,
  key_id, timestamp, phase, until, policy_hash, sig, v`。`sig` は席のみ確保して空
- `approvals/<user>.jsonl` の読み書き(append-only。不正行はファイル名・行番号付きで
  fail)
- 署名対象の正規化バイト列(キーソート compact JSON、sig 除外)の関数だけは用意
  (署名の実装はしない)

### 5. 承認セッション — `inka approve`(CLI、TUI なし)

- キュー表示: 未承認または stale の対象一覧(view_hash は現在のソースから計算)
- 対象ごとに approve / decline(コメント必須)を選び、`--as <approver>` の名義で
  レコードを追記
- 承認対象の契約(展開済み description と子の description)を表示する。
  前回承認時との diff 表示は範囲外

### 6. ポリシー(組み込みデフォルトのみ)

- ポリシー言語(パーサ・セレクタ・述語)は実装しない
- 組み込み規則: **すべての承認対象に any(1)** — 誰か1人の有効な承認で満たされる
- decline の拒否権はこの規則の上でも有効

### 7. 評価 — `inka eval`

- established 不動点(トポ順1パス):
  - fiat: 有効な fiat 承認がある
  - structure: 有効な structure 承認 ∧ 全子 established
  - bind: 有効な bind 承認 ∧ CI 結果の照合が pass(§8)
- stale 検出(レコードの view_hash ≠ 現在の view_hash)
- decline 拒否権(同一ハッシュへの有効な decline があれば unsatisfied)
- ノード側 `until:` の失効判定(inka.toml のフェーズ順序で比較)。承認者側 until は
  範囲外
- blame 出力: 未確立の最深原因ノードの列挙(不足承認・stale・until 失効・bind の
  照合失敗の別を表示)

### 8. CI bind provider(テストターゲット粒度)

- `source: ci:<テストターゲット>` の受理(例 `ci:tests/hash_props.rs`)。
  対象は Rust の integration test ターゲット(tests/ 直下の1ファイル=1ターゲット)
- **界面ハッシュ = テストファイル内容のハッシュ**(改行正規化程度。AST 正規化は
  しない)。ファイル内のあらゆる編集で bind が剥がれる — 粗いが健全側に倒れる
- `ci-export.json` の生成(CI 上のスクリプト。数十行想定):

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

- GitHub Actions ワークフロー: `cargo test --test <name>` をターゲットごとに実行し、
  export を生成してリポジトリに書き戻す(eval は外部 API を呼ばず、このファイルを
  読むだけ — 純関数性の維持)
- eval 側の照合: bind 承認の view_hash が現在の `interface_hash` と整合、
  export の `commit` が評価対象コミットと一致、`result == pass`。
  いずれか欠ければ established にならず、blame に理由が出る

## テスト(ミニマム自身の検証)

- フィクスチャ: dogfood(一周の主対象)+ aocs(パーサ・checkの負荷試験。bind は
  export が無いので未確立のままで正しい)
- 壊したフィクスチャ群で check の各診断を発火させ、insta スナップショットで固定
- ハッシュ: ゴールデンベクタ+ proptest(インデント・行末空白・CRLF で不変、
  内部改行・1文字変更・子の並べ替え(structure のみ不変)の性質)
- シナリオテスト: 「approve → description 変更 → stale」「approve → values 変更 →
  参照契約のみ stale」「bind: テストファイル変更 → bind 失効」「decline → unsatisfied」

## ミニマムに含めないもの

署名・アンカリング / veriq 連携 / ポリシー言語 / TUI / LSP / what-if 群
(`--forecast` 等) / 可視化 / import / LLM lint / 名前空間の解決 / 承認者側 `until:` /
承認時の diff 表示 / closure hash(issue-drafts/08)

## モジュール構成の目安(単一クレート)

```
src/
  syntax/    // lexer, parser, AST, span
  model/     // グラフ、ID interning
  hash/      // 正規化と view_hash(凍結対象)
  record/    // 承認レコードの読み書き(凍結対象)
  eval/      // established 不動点・blame・デフォルトポリシー
  provider/  // ci-export.json の読み込みと照合
  cli/       // check / hash / approve / eval
tests/
  golden/    // ハッシュ・レコードのベクタ(追記専用)
  fixtures/  // dogfood, aocs, 壊したバリアント
ci/
  export.sh  // ci-export.json 生成(GitHub Actions から呼ぶ)
```