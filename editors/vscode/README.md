# inka VS Code syntax highlighting

`.inka` ファイルの最小シンタックスハイライト(TextMate 文法のみ、LSP なし)。

対応: ブロックキーワード(`objective` / `structure` / `bind` / `fiat`)、命題 ID、タグ(`#safety`)、
プロパティ(`children:` / `source:` / `until:` / `promote:`)、`veriq:` 参照、
フェンス付き description(markdown 埋め込み+ `{$var}` の強調)、`//` コメント。

## インストール(開発用)

拡張ディレクトリへ symlink して VS Code をリロードする:

```sh
# WSL / リモートの場合
ln -s "$(pwd)/editors/vscode" ~/.vscode-server/extensions/inka.inka-syntax-0.0.1

# ローカルの場合
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/inka.inka-syntax-0.0.1
```

その後、コマンドパレットで **Developer: Reload Window**。

## 設計方針

この TextMate 文法は**意図的に簡素なベースレイヤ**である。フェンス内の装飾は
`{$var}`・`**太字**`・`` `インラインコード` ``・箇条書き記号のみで、markdown 文法の
埋め込みはしない——埋め込み文法が開くブロック領域(リスト・コードフェンス等)は
外側のフェンス終端を覆い隠し、インデント量に依存する壊れ方をするため。

description 内の本格的な markdown ハイライトは、ロードマップ M6 の `inka lsp` が
semantic tokens で供給する: サーバ側の本物のパーサが description 範囲を確定し、
その範囲だけを CommonMark パーサにかけてトークン化する方式。TextMate 側は
一次描画と、LSP が存在しない表示面(GitHub 等)のための恒久的なフォールバックとして
このまま残す。
