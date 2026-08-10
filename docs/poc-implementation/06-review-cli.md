# 06: `inka review`(CLI)

対象: キュー導出と承認レコードの追記。TUI は作らない。

コマンド名は行為(レビュー)を指す。approve / decline は対象ごとに選ぶ**結果**であり、
フラグとレコードの語彙はレコードスキーマ(05)の `approve | decline` に揃える。

## キュー導出

デフォルトポリシー(全対象 any(1))の下で「承認対象なのに有効な承認が無い、または
stale」な局所ビューの列挙。

```rust
pub struct QueueItem {
    pub target: (ViewKind, NodeId),
    pub current_hash: [u8; 32],
    pub state: QueueState,   // Missing | Stale | Declined
}
```

- 承認対象の全列挙: objective → objective(N)、structure ノード → structure(N)、
  fiat ノード → fiat(N)、bind ノード → bind(N)
- 各対象について現在の view_hash を計算し、レコード群から該当 target のものを引く:
  - 現在ハッシュに一致する approve があれば **キューに出さない**
  - 一致する approve がなく、過去ハッシュへの approve だけ → Stale
  - 一つも無い → Missing
  - 現在ハッシュに decline がある → Declined(情報として出す。解消は編集か撤回)
- **07 の「有効な承認」判定と同じ関数を使う**こと。キューと eval で判定がズレると
  「eval は文句を言うのにキューに出ない」が起きる

## CLI の形

```
inka review --as shimomura                    # キューを対話的に処理
inka review --as shimomura --list             # キュー表示のみ
inka review --as shimomura <id> --approve     # 単一対象を非対話で承認
inka review --as shimomura <id> --decline --comment "..."   # 非対話 decline
```

- `--approve` / `--decline` は**非対話の単一対象**専用。対話モードはフラグ無しで
  開き、対象ごとに結果を選ぶ。
- `--decline` は `--comment` 必須。引数パーサの段階で強制する(対話モードの
  コメント必須入力と同じ規律を型で持つ)。

対話ループ(1対象ずつ):

1. 表示: 対象の種別と ID / **展開済み description** / structure なら子の
   description 一覧(これが局所ビューのすべて。それ以上は見せない)
2. 入力: `a`(approve)/ `d`(decline: コメント入力必須)/ `s`(skip)/ `q`(quit)
3. a/d でレコードを構築(view_hash は表示に使った current_hash、phase は
   inka.toml の current、timestamp は現在時刻の RFC3339)して
   `approvals/<approver>.jsonl` に追記

## 実装メモ

- レコード追記は「1行を組み立てて append」だけにする。既存行の書き換え・削除の
  コードパスを**作らない**(append-only を関数構成で守る)
- `--as` 未指定は error(環境変数フォールバックは後で足せる。まず明示のみ)
- 標準入力が TTY でない場合(パイプ)は対話モードを拒否して `--list` か
  単一対象+`--approve`/`--decline` の非対話形を促す
- timestamp の取得(now)は cli/ 層に置き、キュー導出・レコード構築の純関数には
  引数で渡す(eval の時計非依存と同じ規律)

## テスト

- シナリオ: 全対象 Missing → 全部 approve → キューが空
- description 編集 → その対象と直接親だけが Stale でキューに戻る
- decline 済み対象がキューで Declined と表示される
- 対話部分は薄く保ち、キュー導出(純関数)を insta で固定するのを主戦場にする
