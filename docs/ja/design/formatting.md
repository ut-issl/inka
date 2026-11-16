# inka — フォーマット

`inka fmt` と言語サーバのフォーマット要求が出力する `.inka` ファイルの正規レイアウト。
ファイルの意味は[確定仕様](spec.md)が定め、このページはレイアウトだけを定める。
英語版(正): [English](https://www.space.t.u-tokyo.ac.jp/inka/design/formatting/)

## 例

````text
// File comment.

// Comment on the block.
structure pointing_ok #mission #aocs {
    ```
    In steady-state observation mode, the pointing control error is at most {$pointing_accuracy}
    ```
    // Comment on the children property.
    children:
        // Comment on this child.
        attitude_knowledge_ok,
        control_law_ok,
        mass_properties_valid
}

fiat mass_properties_valid {
    ```
    The mass properties used by the control design match the flight model
    ```
    // Awaiting the mass-properties test.
    until: M3
    promote: bind
}

structure single {
    ```
    ...
    ```
    children: only_child
}
````

## 保証

- **意味を保つ。** フォーマットはどのビューハッシュも変えず、フォーマット後のファイルは
  ソース位置とフェンス内のインデントを除いて同じ構文木にパースされる。
- **冪等。** フォーマット済みのファイルをフォーマットしても何も変わらない。
- **エラーはフォーマットしない。** パースエラーのあるファイルは触らずに報告する。コメントが
  パースエラーの原因になることはない。

## レイアウト

- **ファイル。** UTF-8、改行は LF、行末空白なし、末尾の改行はちょうど1つ。ファイルコメントを
  先頭に置き、空行を1つ挟む。ブロックはソース順のまま、ちょうど1つの空行で区切る。
- **インデント。** 1段あたり空白4つ。タブは使わない。ブロックヘッダとそのコメントは0段、
  ブロックの中身は1段、複数行の `children:` リストの要素は2段。
- **ブロックヘッダ。** `<mode> <id> #tag ... {` を空白1つで区切る。タグはソース順のまま。
- **ブロック本体。** description、次に `children:` / `test:` / `calc:`、次に `until:`、
  次に `promote:`。1行に1プロパティで、`key: value` と書く。ブロック内に空行は入れない。
- **description。** フェンスは1段に置く。開きフェンスは長さとインフォ文字列を保ち、閉じフェンスは
  開きフェンスと同じ長さにする。本文は共通の先頭空白インデントを除いたうえで、空でない各行を1段に
  再インデントする。空行は空にする。これによってビューハッシュが変わる場合 — フェンスに複数行の値が
  展開されると起こりうる — と、本文が未定義の値を参照していてビューハッシュを計算できない場合は、
  本文を書かれたままバイト単位で残す。
- **children。** コメントの無い子が1つだけなら、プロパティと同じ行に書く: `children: only_child`。
  それ以外では `children:` を単独行にし、各要素を2段の独立した行に書いて行末の `,` で区切る。
  最後の要素の後に `,` は付けない。要素はソース順のまま。
- **コメント。** すべてのコメントは付着先の直前の独立した行に、付着先と同じ段で書く: ブロックへの
  コメントはブロックヘッダの上、プロパティへのコメントはそのプロパティの上、子要素へのコメントは
  その要素の上、ファイルコメントはファイルの先頭。本文の各行は `// <行>`、空行なら `//` になる。
  したがって行末コメントとぶら下がりコメントは付着先の上に移動する。本文と付着先は変わらない。
- **行幅。** 折り返さない。

## コマンド

- `inka fmt [PATH]` は `PATH`(既定はカレントディレクトリ)のプロジェクトのすべての
  `*.inka` ファイルをその場で書き換える。
- `inka fmt --check [PATH]` は何も書き込まず、変更されるファイルを列挙し、1つでもあれば
  非ゼロで終了する。
- どちらもプロジェクトの manifest と values を読む。フェンスを再インデントしてよいかがそれで
  決まるためで、読み込めなければ何もフォーマットしない。パースエラーのあるファイルは `inka check`
  と同じ形で報告して触らず、他のファイルは通常どおりフォーマット(または列挙)したうえで、非ゼロで
  終了する。
- 言語サーバは開いている文書に対する `textDocument/formatting` に同じ出力で応答する。
