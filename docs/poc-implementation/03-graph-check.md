# 03: グラフ構築と `inka check`

対象: ブロック群 → グラフ+構造診断。ここまでで最初のコマンドが動く。

## ID interning とグラフ

```rust
pub struct NodeId(u32);

pub struct Graph {
    names: Vec<String>,                  // NodeId -> 名前
    index: HashMap<String, NodeId>,      // 名前 -> NodeId
    nodes: Vec<Node>,                    // NodeId で引く(mode, rendered desc, 注釈...)
    children: Vec<Vec<NodeId>>,          // 隣接(親→子)
    parents: Vec<Vec<NodeId>>,           // 逆辺(blame と stale の親遡りで使う)
}
```

- 構築は2パス: ①全ブロックを登録(ここで**重複 ID を error**)②children を解決
  (ここで**未定義参照を error**。span はブロック側の `Spanned<String>` から)
- ID 順(名前のソート順)で NodeId を振ると、以後の全出力が入力ファイル順に
  依存しなくなる(スナップショットが安定する)

## トポソートと閉路

- Kahn 法(入次数カウント)。処理できないノードが残ったら閉路
- **閉路の経路表示**: 残ったノード集合内で DFS し、訪問スタックに再訪した点から
  スタックを巻き戻して経路を復元 → `a → b → c → a` の形で診断に載せる

## objective の扱い

- `objective` ブロックがちょうど1つあることを check(0個・2個は error)
- 到達性: objective から children を辿って到達可能集合を作り、圏外のノードに
  warning "unreachable"

## 診断一覧(コードを固定しておく)

| code | severity | 発生箇所 |
|---|---|---|
| duplicate-id | error | 登録パス |
| undefined-reference | error | 解決パス |
| cycle | error | トポソート |
| no-objective / multiple-objectives | error | 登録パス |
| unreachable | warning | 到達性 |
| undefined-variable | error | 02 の展開 |
| missing-description / duplicate-description | error | 01 のパーサ |
| unknown-property / duplicate-property / mode-property-mismatch | error | 01 のパーサ |

check は 01〜03 の全診断を集めて表示するだけの薄いコマンドにする
(`fn check(project: &Project) -> Vec<Diagnostic>` が本体で、cli は表示と exit code)。

## CLI

- `inka check [path]`(path 省略時はカレント)。inka.toml を探して読み、
  argument/ を読み、展開し、グラフを作り、診断を ariadne で表示
- error が1つでもあれば exit code 1。warning のみなら 0

## テスト

- dogfood / aocs が warning ゼロで通る
- 壊しフィクスチャ1診断1ファイルで、全 code を1回ずつ発火。出力(ariadne でなく
  Diagnostic の Debug/JSON)を insta で固定 — 表示の揺れとロジックの揺れを分離する