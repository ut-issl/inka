# inka — 確定仕様

このページの内容はすべて確定済み。ハッシュとレコードのレイアウトは、ゴールデンベクタが
入った時点で凍結される(以後追記専用。非互換の変更は v2 として並存)。
英語版(正): [spec.md](spec.md)

## DSL 構文

```
structure <id> #tag1 #tag2 {
  ```
  description(markdown 可。{$var} の値埋め込み可。フェンスは ``` 以上)
  ```
  children: <id>, <id>, ...
}

bind <id> {
  ```...```
  source: veriq:<module>.<name>   // または ci:<test-target>
}

fiat <id> {
  ```...```
  until: <phase>
  promote: bind
  reason: "<短い理由>"
}
```

- ブロック: `objective` / `structure` / `bind` / `fiat` + id + 任意のタグ + `{}`。
  インデントは構造的に無意味。
- ブロックにつきフェンス付き description をちょうど1つ。インフォ文字列は受理して予約。
  フェンス内=自然言語のみ、フェンス外=キーワードのみ。
- モード別必須プロパティ(パース時に強制): `structure`/`objective` → `children:`
  必須、`bind` → `source:` 必須、`fiat` → どちらも不可。
- 注釈: `until: <phase>` と `reason: "<短文>"` は全ブロックに書ける。
  `promote: <structure|bind>` は fiat のみ・任意(`promote:` なしの `until:` は
  単純な期限切れ)。`reason` は二重引用符の1行文字列でエスケープなし — 言語中で
  唯一の引用符トークン(description は引用符を使わない)。
- `//` 行コメント。字句段階で落ち、ハッシュされない。
- `{$` のエスケープは無い — 正確なパターン `{$識別子}` のみが補間で、それ以外は
  文字通り。
- プロパティの順序は自由。重複は error。
- タグは自由語彙。check はタグについて何も言わない(タイポ検出はポリシーセレクタ側:
  存在しないタグにマッチするセレクタが warning)。
- 名前空間: 区切りは `::`、`argument/` 配下のディレクトリから自動付与
  (`argument/power/x.inka` → `power::<id>`)。直下のファイルは名前空間なし。
  同一名前空間内の参照は接頭辞省略可。ID の中に `::` は書けない。ポリシーセレクタも
  同記法(`power::*`)。veriq の source 参照は Python の `module.function` 名のまま。
- `source:` は check では形式検査のみ。実在と pass/fail は eval の仕事。

## プロジェクト構成

- `inka.toml`: `[project] name`、`[phases] order`(全順序)+ `current`(order に
  対して検証)、`[values] files`。未知キーは拒否。
- 論証は規約読み込み: `argument/` 配下のすべての `*.inka`。
- values ファイルはフラットな TOML 文字列マップ。再帰展開なし(1パス)。ファイル間の
  キー衝突は error。未定義の `{$var}` は error。

## ハッシュ(v1)

- 展開済み description の正規化(順に): NFC、CRLF/CR→LF、行末空白除去、先頭末尾の
  空行除去、共通スペースインデントの剥ぎ取り、`\n` で連結。冪等。内部の改行と空白は
  契約の一部(ほぼ verbatim。markdown-AST 正規化はしない)。
- 注釈の正規形: キーでソートした `key:value` 文字列列(例
  `["promote:bind", "reason:awaiting the mass-properties test", "until:M3"]`。
  `reason` の値は引用符を除いた本文そのもの)。
- フレーミング: 全フィールドは u64-LE の長さ接頭辞付き。リストは u64-LE の要素数を
  前置。ドメインタグ: `inka:objective:v1` / `inka:structure:v1` /
  `inka:fiat:v1` / `inka:bind:v1`。
  - objective = tag ‖ desc ‖ annotations ‖ 子 desc 群(バイト列順ソート)
  - structure = tag ‖ desc ‖ annotations ‖ 子 desc 群(バイト列順ソート)
  - fiat = tag ‖ desc ‖ annotations
  - bind = tag ‖ desc ‖ annotations ‖ interface_hash(生32バイト)‖ 仮定リスト
    (仮定集約が入るまでは空。席があるので後から埋めてもレイアウト変更にならない)
- ハッシュ非包含: ID、タグ、コメント、`source:` の参照文字列。包含: 期限注釈。
  値展開は正規化・ハッシュの前段 — ハッシュはプレースホルダを見ない。

## 承認レコード(v1)

- フィールド: `v`(=1)、`target`(`"<kind>:<id>"`。kind ∈ objective | structure |
  bind | fiat)、`view_hash`
  (`"sha256:<hex>"`)、`label`(approve | decline)、`comment`(decline では必須)、
  `approver`、`key_id`、`timestamp`(RFC3339 UTC。監査用のみ — eval は読まない)、
  `phase`(承認時点の現在フェーズ)、`until`(承認者側留保)、`policy_hash`、`sig`
  (予約。署名導入まで空)。
- 署名対象の正規化バイト列: `sig` を除くキー昇順 compact JSON。省略可フィールドは
  `null` として出す。
- jsonl は追記専用、承認者ごとに1ファイル(`approvals/<name>.jsonl`)。不正行は
  ファイル名+行番号付きで大きく失敗。未知フィールドは拒否。

## 評価の意味論

- レコードが数えられる条件: target と view_hash が現在値に一致し、同一ハッシュへの
  decline が存在せず(decline はクォーラムに関係なく拒否権)、ノードの `until:` が
  失効していない(宣言されたフェーズ順序で比較)。**失効はハード**: 現在フェーズが
  `until:` を過ぎたら、そのノードの承認は**期限後に付けられたものも含めて**一切
  カウントしない。`until:` は全員が承認した「未来へ先置きされた decline」であり、
  解除は注釈の編集=契約変更+再承認のみ。
- established: fiat = 有効な fiat 承認 / structure = 有効な structure 承認 ∧ 全子
  established / bind = 有効な bind 承認 ∧ 証拠の照合 / objective = 有効な objective
  承認 ∧ 全子 established(ルートの文言も他の契約と同様に inka の中で合意される)。
- blame は原因が自分自身にあるノードだけを列挙(不足・stale・decline・失効・証拠
  不一致)。子経由でのみ未確立のノードは出さない。

## CI evidence provider(テストターゲット粒度)

- provider 契約 — どの証拠供給源も次を供給する: 内容アドレスの界面ハッシュ、pass が
  何を立証するかの人間可読な言明、コミットに束縛された結果のリポジトリ内容としての
  供給(eval は外部 API を呼ばない)、環境ハッシュ。
- `ci-export.json` v1: `commit`、`environment.lockfile_hash`、`targets[]`
  (`id` 例 `tests/hash_props.rs`、`interface_hash`(ファイル内容ハッシュ、CRLF→LF
  のみ)、`result`(`pass` | `fail`))。
- `source: ci:<id>` の eval 照合: export にエントリがある/現在のワークツリーの
  ファイル界面ハッシュが export と一致/bind 承認の view_hash が現在の desc+界面
  ハッシュで再計算した値と一致/result が pass。export のコミット不一致は warning
  (内容の一致は界面ハッシュが担保)。

## リポジトリ規約

- コード・フィクスチャ・設定のコメントは英語。`.md` は英語が正で `_ja.md` を併置。
  フィクスチャの契約と値も英語。
- 文書は現在の設計だけを書く — 履歴や「旧◯◯」を書かない。
- ゴールデンベクタのディレクトリは追記専用(CI で保護)。