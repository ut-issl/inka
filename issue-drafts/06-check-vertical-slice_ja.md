# syntax: lexer・パーサ・`inka check`

- **ラベル:** enhancement
- **マイルストーン:** M1
- **依存:** #6(ドッグフード論証。構文は現在2つのサンプル
  `tests/fixtures/dogfood` と `tests/fixtures/aocs` で荷重済み)

## 動機

最初の本物のコマンド。パーサを書くことは構文の最終検証でもある: 残っている構文レベルの
未決事項(エスケープ、フェンスの細部、ID 名前空間)は実装によってしか確定しない。
この Issue が終わると2つのサンプル論証が機械検査され、以後の構文議論には動く審判がつく。

## スコープ

**Lexer**
- `//` 行コメント(除去。契約には決して入らない)
- フェンス: 可変長(``` またはそれ以上)、CommonMark 流の包含規則。
  インフォ文字列は受理して保持(将来の拡張用に予約)
- フェンス内の `{$var}` 認識
- 識別子、`#タグ`、ブロックキーワード、プロパティキーワード、`{` `}`、span

**パーサ(手書き再帰下降)**
- ブロック = モードキーワード(`objective` / `structure` / `bind` / `fiat`)+ id
  + タグ + `{ ... }`
- ブロックにつきフェンス付き description をちょうど1つ
- モード別必須プロパティをパース時規則に: `structure` → `children:` 必須、
  `bind` → `source:` 必須、`fiat` → どちらも不可。`until:` / `promote:` 注釈
- エラー回復: `}` まで読み飛ばし、1つの壊れたブロックが連鎖しない

**プロジェクト読み込み**
- `inka.toml`(`[project]`、`[phases] order/current`、`[values] files`)
- 論証は規約読み込み: `argument/` 以下の `*.inka` すべて(再帰)
- values TOML からの `{$var}` 展開 — description を消費する何よりも前段で行う
  (後のハッシュは展開後テキストに束縛される)

**グラフ**
- ID interning(`String` → `u32`)、隣接マップ、トポソート、到達性

**診断(`ariadne` or `miette`、span 付き)**
- error: 未定義の子参照、重複 ID、閉路(経路を表示)、未定義 `{$var}`、
  description フェンスの欠落/重複、未知のプロパティ、モードとプロパティの不整合
- warning: objective から到達しない命題
- error があれば非ゼロ終了

**CLI + テスト**
- `clap` サブコマンド骨格、`inka check [path]`
- insta スナップショット: 2つのサンプルが pass。壊したフィクスチャ群で
  各診断をちょうど1回ずつ発火させる

## 実装で従う確定規則(コードファースト方針に従い、理由を rustdoc に記録)

1. **`{$` のエスケープは v1 では設けない。** 補間として特別扱いされるのは正確なパターン
   `{$識別子}` のみで、それ以外の `{` `$` は常に文字通り。文字通りの `{$var}` を契約に
   書きたい場面が実際に出るまでエスケープ記法は追加しない(YAGNI)。誤記は未定義変数
   error が拾う。
2. **プロパティの順序は自由。重複は常に error。**
3. **タグは自由語彙。** check(M1)はタグについて何も言わない。タイポ検出は M3 の
   ポリシー導入時に「存在しないタグを参照するセレクタ(空集合を選ぶ)」への warning
   としてポリシー側に付ける(論証側の未参照タグは将来のための注記として正当)。
4. **名前空間はディレクトリ構造から自動付与。** `argument/` からの相対ディレクトリ
   パスを `::` で連結したものが名前空間(`argument/power/eol.inka` 内の
   `eol_capacity_ok` は `power::eol_capacity_ok`)。`argument/` 直下は名前空間なし。
   ID 自体に `::` は書けない(名前空間は場所からのみ生じる)。参照は同一名前空間内
   なら接頭辞省略可、跨ぐ場合はフルパス。ID はハッシュ非包含なので、ファイル移動=
   一括 rename であり承認は失効しない。ポリシーセレクタも同じ記法(`power::*`)。
   なお `source:` の veriq 参照(`veriq:aocs.verify_detumble`)は Python 側の
   module.function 名なのでドットのまま。
5. **`source:` は check では形式検査のみ。** 実在検証は veriq export を読む eval の仕事。

## 受け入れ条件

- [ ] `inka check` が `tests/fixtures/dogfood` と `tests/fixtures/aocs` を pass する。
- [ ] 壊したフィクスチャ群が存在し、全診断が insta で固定されている。
- [ ] 上記5規則が実装され、各1行の理由とともに rustdoc に記録されている。

## 参照

- inka-concept.md §2.4(診断)、§2.5(values)、§12(構文)
- inka-implementation-roadmap.md M1
- 各フィクスチャの README(dogfood / aocs)— 未決事項の出所