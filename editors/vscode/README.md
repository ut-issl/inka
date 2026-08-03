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

LSP(診断・hover・rename 等)はロードマップ M6 で `inka lsp` として別途実装する。