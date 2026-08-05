# 07: `inka eval`(established 不動点と blame)

対象: プロジェクト+レコード群+ci-export → 各ノードの established と blame。
**入力は完全にリポジトリの内容だけ**。時計・環境を読まない。

## 有効な承認の判定(キューと共有する中核関数)

```rust
pub fn valid_approvals<'a>(
    records: &'a [Record],
    target: (ViewKind, NodeId),
    current_hash: [u8; 32],
    node_until: Option<&str>,     // ノード側期限注釈
    phase_order: &PhaseOrder,     // 名前→序数
    current_phase: &str,
) -> Result<Vec<&'a Record>, VetoOrExpired>
```

判定順序:

1. target と view_hash が現在値に一致するレコードだけを対象にする(不一致 = stale で
   無効。**エラーではなく単に数えない**)
2. 同一ハッシュへの decline が1つでもあれば**拒否権**: 承認数に関係なく unsatisfied
   (blame 用に decline レコードを返す)
3. ノード側 `until:` の失効: `phase_order[current] > phase_order[until]` なら
   承認を数えない(期限切れ)。**期限後に付けられた承認も数えない**(record.phase は
   参照しない)。解除は注釈の編集=ハッシュ変更+再承認のみ。kind によらず同じ式でよい
4. 残った approve が **1件以上**(デフォルトポリシー any(1))なら satisfied

## established 不動点

DAG なのでトポ順1パス(葉から根へ):

```
for id in topo_order:
    established[id] = match mode {
        Fiat      => satisfied(fiat(id)),
        Structure => satisfied(structure(id)) && children(id).all(established),
        Bind      => satisfied(bind(id)) && ci_matches(id),   // 08 参照
        Objective => satisfied(objective(id)) && children(id).all(established),
    }
```

## blame

「未確立の最深原因」= 未確立ノードのうち、**原因が自分自身にある**ものだけを列挙する。
子のせいで未確立なだけのノードは出さない(子の側が出る)。

```rust
pub enum BlameReason {
    MissingApproval { kind: ViewKind },
    StaleApproval   { kind: ViewKind, approvers: Vec<String> },  // 旧ハッシュにしか承認がない
    Declined        { by: String, comment: String },
    UntilExpired    { until: String },
    BindNoExport,
    BindCommitMismatch { export_commit: String },
    BindInterfaceChanged,        // 承認時の界面ハッシュ ≠ export の現在値
    BindTestFailed,
}
pub struct Blame { pub node: NodeId, pub reasons: Vec<BlameReason> }
```

- 表示は「ノード ID+理由(+span)」を Diagnostic に変換して ariadne で
- exit code: clean なら 0、未確立ありなら 1(CI にそのまま置ける)

## `inka eval` の出力

- サマリ1行: `established 12/15`
- blame 一覧(上記)
- `--json` で機械可読出力(後の LSP・CI 連携の下地。serde で Blame を出すだけ)

## テスト(シナリオが本体)

手書き jsonl フィクスチャで:

1. 全承認あり → clean
2. description 変更 → 対象+直接親のみ blame(**孫は無傷**であることが最重要の検算)
3. values の値変更 → 参照している契約だけ blame
4. decline 1件 → クォーラムが揃っていても unsatisfied
5. until:M2 のノード、current を M3 に変更 → UntilExpired
6. bind: export なし / commit 不一致 / interface 変化 / fail の4種が
   それぞれの BlameReason になる
7. objective 未承認 → objective が MissingApproval で blame される
8. until 期限後に付けた承認 → カウントされず UntilExpired のまま

すべて insta スナップショットで固定。
