# 01: lexer・AST・パーサ

対象: `argument/*.inka` のテキスト → span 付き AST。確定済みの構文規則
(issue-drafts/06 の5規則)に従う。

## AST(先に型を固める)

```rust
pub enum Mode { Objective, Structure, Bind, Fiat }

pub struct Block {
    pub mode: Mode,
    pub id: Spanned<String>,
    pub tags: Vec<Spanned<String>>,          // "#safety" の '#' を除いた形で保持
    pub description: Description,
    pub children: Vec<Spanned<String>>,      // structure/objective のみ
    pub source: Option<Spanned<SourceRef>>,  // bind のみ
    pub until: Option<Spanned<String>>,      // フェーズ名
    pub promote: Option<Spanned<String>>,    // "bind" 等
    pub span: Span,
}

pub struct Description {
    pub raw: String,            // フェンス内の生テキスト(展開・正規化前)
    pub info: Option<String>,   // フェンスのインフォ文字列(保持のみ)
    pub span: Span,             // フェンス内側の範囲({$var} の span 逆算に使う)
}

pub enum SourceRef { Veriq(String), Ci(String) }   // "veriq:aocs.verify_x" / "ci:tests/x.rs"
```

モード別の必須/禁止(structure→children 必須、bind→source 必須、fiat→どちらも禁止、
objective→children 必須)は**パーサの検査**として実装し、AST に不正な組合せを持ち込まない。

## lexer

行指向で書くと簡単。1パスで次のトークン列にする:

- `//` 以降を行末まで捨てる(**ただしフェンス内は除く** — 先にフェンス判定)
- フェンス開始: `^\s*(`{3,})(\S*)\s*$` にマッチしたら、**同じ長さ以上のフェンスのみ**を
  終端として raw ブロックを丸ごと1トークン `FenceBlock { raw, info, span }` に切り出す。
  この間 `//` も `{}` も一切解釈しない(フェンス内=自然言語の世界、が規則の実装)
- それ以外の行: 識別子 / キーワード(objective, structure, bind, fiat)/
  `#tag` / `{` / `}` / `children:` などの `ident:` 形 / `,` / `::`(将来用に予約)
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
  source なし、未知プロパティ、重複プロパティ、閉じ忘れ `}`(→次ブロックが生きて
  いることまで確認 = 回復のテスト)
- フェンス: より長いフェンス(````)の中に ``` を含むケース、インフォ文字列付き、
  CRLF 入力