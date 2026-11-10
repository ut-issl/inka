# inka のリリース手順

この文書は inka のリリースの作り方を説明します([English](RELEASING.md))。

## バージョンの付け方

- リポジトリ内のすべてのパッケージは同じバージョンを持ちます。対象は Cargo ワークスペース(全クレート)，Python
  プロジェクト(`pyproject.toml` と `crate/inka-evidence-py`)，npm パッケージ(`web`，`editors/vscode`，
  `crate/inka-evidence-wasm`)です。基準は `Cargo.toml` の `[workspace.package]` の version です。
- バージョンは [セマンティックバージョニング](https://semver.org/lang/ja/) に従う `MAJOR.MINOR.PATCH` で，プレリリースや
  ビルドの接尾辞は付けません。メジャーバージョンが `0` の間は，互換性を壊すリリースでマイナーバージョンを，それ以外の
  リリースでパッチバージョンを上げます。
- 形式のバージョン(`.inka` 言語，テストエビデンス v1，承認ハッシュ v1)はこれとは別に管理し，形式が変わるときだけ
  上げます。形式を変えたリリースでは，そのことをリリースノートに書きます。
- manifest や lockfile のどれかがワークスペースのバージョンと食い違うと，CI(`repository-check.yaml` の
  `Check package versions agree`)が失敗します。

リリースでは [GitHub Release](https://github.com/ut-issl/inka/releases) とそのタグ `v<version>` を作ります。crates.io・
PyPI・npm・VS Code Marketplace へはまだ公開しません。

## リリースの手順

### 1. リリース用のプルリクエストを用意する

`main` から新しいブランチを切り，次の作業をします。

1. すべてのパッケージのバージョンを上げます。

   ```console
   python3 scripts/release.py bump 0.1.0
   ```

   すべての manifest と lockfile のバージョンだけを書き換えます。`python3 scripts/release.py check` で一致を確認できます。
2. リリースノートを書きます。`CHANGELOG.md` と `CHANGELOG.ja.md` の両方で，前回のリリースの上に，見出しがちょうど
   `## v<version>`(例: `## v0.1.0`)の節を追加します。`### Breaking changes` のような小見出しには `###` を使います。
   GitHub Release では改行がそのまま残るので，1 文ごとに改行してください。英語版の節が GitHub Release の本文に
   なります。どちらかのファイルに節がないと，リリースワークフローは実行を拒否します。
3. プルリクエストを作り，CI が通ったら `main` にマージします。

### 2. 任意: dry run

Actions タブで `Release` ワークフローを，`mode` を既定の `dry-run` のまま実行します。dry run はどのブランチからでも
実行できます。リリースと同じ検査をすべて行い(そのためタグがすでに存在するときも失敗します)，リリースノートを
実行結果の Summary に表示しますが，何も作りません。

### 3. リリースする

`main` の先頭のコミットで CI が通ってから，`main` で `Release` ワークフローを，`mode` を `release` にして実行します。
ワークフロー自身は CI の結果を確認しません。

ワークフローは `Cargo.toml` のバージョンをタグ `v<version>` としてリリースします。タグは自分で push しないでください。
ワークフローが GitHub Release と一緒に，起動したコミットにタグを作ります。次のどれかに当てはまると，何も作らずに失敗します。

- `main` 以外から起動した
- バージョンが食い違う manifest や lockfile がある
- タグがすでに存在する
- `CHANGELOG.md` か `CHANGELOG.ja.md` にそのバージョンの節がない

### 4. リリースを確認する

[GitHub Release](https://github.com/ut-issl/inka/releases) のタイトルが `v<version>` で，本文が `CHANGELOG.md` の
内容になっており，最新リリース(Latest)になっていることを確認します。

## リリースを直すとき

公開したタグは動かしません。リリースに問題が見つかったら，`main` で直して新しいパッチリリースを出します。
文面だけが間違っているときは GitHub 上で GitHub Release の本文を直接編集し，対応する `CHANGELOG` の節はプルリクエストで
直します。
