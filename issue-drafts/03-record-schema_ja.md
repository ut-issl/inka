# record: 承認レコードスキーマ(v1)をコードで — serde 構造体 + 署名対象の正規化バイト列

- **ラベル:** enhancement
- **マイルストーン:** M0(仕様凍結 — コードファーストに再解釈)
- **依存:** なし(01 / 02 と並行して進められる)

## 動機

ロードマップのもう一つの凍結対象(`spec/record.md`)にも同じ「コードが仕様」方式を
適用する。凍結物は serde 構造体の定義、署名対象の正規化バイト列を作る関数、および
それらのゴールデンベクタである。署名そのものの実装は M7 まで行わないが、`sig`
フィールドの席と、署名される正確なバイト列を**今**凍結しておくことで、後から署名を
足してもフォーマットが壊れない。

## この Issue で確定させる設計判断

各判断は rustdoc に1行の理由とともに記録する:

1. **フィールド集合**: `target, view_hash, label, comment, approver, key_id,
   timestamp, phase, until, policy_hash, sig, v`。どのフィールドが省略可能か
   (`until`、approve 時の `comment`)、バージョンフィールド `v` の意味。
2. **署名対象の正規化バイト列**: `sig` を除き、キーをソートした compact JSON
   表現。このバイト列が M7 で署名される対象であり、`policy_hash` / `phase` /
   `until` による改竄防止(concept §9.1)の前提になる。
3. **jsonl の規約**: append-only。1行 = 1つの自己完結した attestation。
   マージコンフリクト回避のためファイルは人ごとに分割してよい
   (`approvals/<user>.jsonl`)。
4. **読み込み時のエラー処理**: 不正な行はファイル名・行番号付きで大きく失敗させる —
   決して黙って読み飛ばさない(読み飛ばされた行が decline を隠しうる)。

## タスク

- [ ] serde 構造体 + jsonl の読み書き。
- [ ] 純関数としての `canonical_signing_bytes(record) -> Vec<u8>`。
- [ ] 正規化バイト列のゴールデンベクタ(レコード → 正確なバイト列)を
    `tests/golden/` にコミット。
- [ ] 上記の全判断の rustdoc。

## 受け入れ条件

- [ ] ラウンドトリップテスト: parse → serialize → parse が恒等になる。
- [ ] 署名対象の正規化バイト列のゴールデンベクタが通る。
- [ ] 未知のフィールド/欠落フィールドを持つレコードが明確なエラーで拒否される。

## 参照

- inka-concept.md §9.1、§9.2
- inka-implementation-roadmap.md §2(spec/record.md をコードファーストに翻案)
