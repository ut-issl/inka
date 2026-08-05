# 01: lexer・AST・パーサ

対象: `argument/*.inka` のテキスト → span 付き AST。確定済みの構文規則
([../en/design/spec.md](../en/design/spec.md))に従う。

## AST(実装済み — `crate/inka-core/src/ast.rs` が正)

AST は実装済みで、このガイドではなく**コードと rustdoc が正**である。要点:

- モードごとの構造体(`ObjectiveBlock` / `StructureBlock` / `BindBlock` /
  `FiatBlock`)を `Block` enum で束ね、不正な組合せ(fiat に children、structure に
  source、objective の `promote: bind` 等)を**型で表現不能**にする。
- 注釈は `until:` をアンカーとする `Deadline { until, reason }`。promote を書ける
  モードだけが対応する型を持つ(`FiatDeadline` = structure|bind、
  `ObjectiveDeadline` = structure のみ)。`reason:` / `promote:` が `until:` なしで
  現れないことも型が保証する。
- 診断が指しうる要素は `Spanned<T>` で包む。FileId / SourceMap / Span / Spanned は
  `source.rs` にある。

パーサの仕事は「テキストをこの型に入れること」であり、型に入らないもの
(モードとプロパティの不整合、不正な promote 先)はその場で診断になる。

## lexer

行指向で書くと簡単。1パスで次のトークン列にする:

- `//` 以降を行末まで捨てる(**ただしフェンス内は除く** — 先にフェンス判定)
- フェンス開始: `^\s*(`{3,})(\S*)\s*$` にマッチしたら、**同じ長さ以上のフェンスのみ**を
  終端として raw ブロックを丸ごと1トークン `FenceBlock { raw, info, span }` に切り出す。
  この間 `//` も `{}` も一切解釈しない(フェンス内=自然言語の世界、が規則の実装)
- それ以外の行: 識別子 / キーワード(objective, structure, bind, fiat)/
  `#tag` / `{` / `}` / `children:` などの `ident:` 形 / `,` / 引用符付き文字列
  (`"..."`。1行・エスケープなし。`reason:` の値専用)/ `::`(将来用に予約)
- フェンス外に裸のテキストは存在できない構文なので、予約語は常にキーワードとして
  扱ってよい(文との衝突を考えなくてよい)

span はバイトオフセット。フェンス内の `{$var}` は lexer では切らず、raw のまま渡す
(展開は 02、エラー位置は `Description.span + 内部オフセット` で計算)。

## パーサ(再帰下降)

```
file      := block*
block     := mode_kw ident tag* '{' entry* '}'
entry     := FenceBlock
           | 'children' ':' ident (',' ident)*
           | 'source' ':' source_ref
           | 'until' ':' ident
           | 'promote' ':' ident
           | 'reason' ':' STRING
```

- **description はブロックにつきちょうど1つ**(0個・2個は error)
- プロパティは**順不同、重複は error**(確定規則2)
- 未知の `ident:` は error "unknown-property"
- モード別の必須/禁止チェックはブロックを閉じた時点で行う

## エラー回復

ブロック単位で回復する。entry のパースに失敗したら診断を積み、**次の `}` または次の
モードキーワードまでスキップ**して続行。フェンストークンは lexer 段階で対になっている
ので、フェンス内の `}` に釣られる事故は起きない(これが lexer でフェンスを丸ごと
切り出す理由)。

## テスト

- dogfood / aocs の全ファイルがパースでき、AST の insta スナップショットが安定する
- 壊しフィクスチャ: description なし、description 2個、fiat に children、bind に
  source なし、structure に `promote:`、objective に `promote: bind`、`until:` なしの
  `reason:`、`promote: fiat` など不正な値、閉じない引用符、未知プロパティ、
  重複プロパティ、閉じ忘れ `}`(→次ブロックが生きていることまで確認 = 回復のテスト)
- フェンス: より長いフェンス(````)の中に ``` を含むケース、インフォ文字列付き、
  CRLF 入力