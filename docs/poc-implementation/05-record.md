# 05: 承認レコード(凍結対象)

対象: `approvals/<user>.jsonl` の読み書きと、署名対象の正規化バイト列。

## スキーマ

```rust
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub v: u32,                       // 1 固定
    pub target: String,               // "structure:pointing_ok" — "<kind>:<id>"
    pub view_hash: String,            // "sha256:<hex>"
    pub label: Label,                 // approve | decline
    pub comment: Option<String>,      // decline では必須(読み込み時に検証)
    pub approver: String,             // "shimomura" など。ミニマムでは自己申告
    pub key_id: Option<String>,       // 席のみ。ミニマムでは None
    pub timestamp: String,            // RFC3339 UTC 例 "2026-01-01T00:00:00Z"
    pub phase: String,                // 承認時点の inka.toml current
    pub until: Option<String>,        // 承認者側 until。ミニマムでは書かない
    pub policy_hash: Option<String>,  // ミニマムでは None(デフォルトポリシーのため)
    pub sig: Option<String>,          // 席のみ。ミニマムでは None
}
```

決めごと(rustdoc に理由とともに):

- `target` は `<kind>:<id>` の1フィールド。kind を分けないのは jsonl を人が grep
  しやすくするため
- `timestamp` は RFC3339 UTC 固定。**eval はこれを一切使わない**(fresh 判定は phase
  フィールド。時計非依存の原則)。記録は監査用
- 未知フィールドは**拒否**(`deny_unknown_fields`)— スキーマの静かな divergence を防ぐ

## 署名対象の正規化バイト列

```rust
pub fn canonical_signing_bytes(r: &Record) -> Vec<u8>
```

- `sig` を除いた全フィールドを **キー昇順・compact(空白なし)の JSON** にする
- 実装: `serde_json::to_value(r)` → Object から `"sig"` を remove → `to_vec`。
  serde_json の Map はデフォルト(`preserve_order` feature **無効**)で BTreeMap
  なのでキー順が保証される。**preserve_order を有効にしないこと**を rustdoc に明記
  (依存クレートの feature 統合で紛れ込みやすい)
- `None` のフィールドは `null` として**出す**(欠落と null を区別しない=表現を1つに)
- ミニマムでは署名しないが、ゴールデンベクタ(record → 期待バイト列)だけは
  `tests/golden/record/` に作って凍結する

## jsonl の読み書き

- 読み: `approvals/*.jsonl` を全部、1行ずつ `serde_json::from_str::<Record>`。
  **不正行はファイル名+行番号付きの error で全体を fail**(黙って読み飛ばすと
  decline が消える)
- 書き: 追記のみ。1レコード = 1行(compact)。ファイルは approver ごと
  (`approvals/<approver>.jsonl`)
- `v != 1` は error("未知のレコードバージョン")

## テスト

- ラウンドトリップ: parse → serialize → parse が恒等
- canonical bytes のゴールデンベクタ(全フィールドあり / Option が None の2件以上)
- 不正行(壊れた JSON・未知フィールド・decline でコメントなし)が行番号付きで
  fail する
