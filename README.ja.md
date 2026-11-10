# inka

説明付きの承認プロセスによって論証化の構造化をするツール
テキストベースかつLSP対応(予定)で、反復的な開発に適した設計

## QuickStart

inka のプロジェクトは，`inka.toml` と `argument/` 以下の `*.inka` を持つディレクトリです。
このリポジトリには 2 つ入っているので，以下はリポジトリのルートでそのまま実行できます。
`tests/fixtures/aocs`(衛星の姿勢制御の論証)と `tests/fixtures/dogfood`(inka 自身についての論証)です。

コマンドには Rust ツールチェイン，手順 3 のグラフビューアには Node.js が必要です。

### 1. 論証を検査する

`check` はプロジェクトを読み込み，構造上の問題をすべて報告します。未定義の参照，id の重複，依存の
循環，objective の不在，どの objective からも到達できないノードなどです。問題がなければ何も出力
せず，終了コード 0 で終わります。

```console
cargo run -p inka-cli -- check tests/fixtures/aocs
```

指摘は該当箇所のスニペットとして表示されます。エラーがあれば終了コード 1，警告のみなら 0 のままです。

毎回 `cargo run -p inka-cli --` と打つ代わりに，`inka` コマンドを PATH に入れるには:

```console
cargo install --locked --path crate/inka-cli
```

以降はこのバイナリを使います。

### 2. 論証を書き出す

`export` は論証を cytoscape.js の `elements` JSON として標準出力に書き出します。これが inka の
機械可読なインタフェースです。ビューアはその 1 つの利用者にすぎず，JSON を読めるものなら何でも
利用できます。つまり，論証を読む手段が内蔵 UI だけに閉じることはありません。

```console
inka export tests/fixtures/aocs > aocs.elements.json
```

各ノードは id・discharge モード・namespace・展開済みの description・タグ・証拠ターゲット・期限を
持ち，各辺は親から 1 つの子を指します。出力は決定的なので，実行ごとの差分がきれいに取れます。
`check` を通らない論証は何も書き出しません。

### 3. グラフビューアをビルドする

ビューアは [`web/`](web/README.md) 以下の静的なフロントエンドバンドルです。最初に一度ビルドします。

```console
cd web
npm install
npm run build
cd ..
```

`web/dist` が生成されます。`serve` は既定でここを見ます。

### 4. 対話的なグラフを配信する

`serve` はビューアを配信し，そこに論証を渡します。厳密に read-only で，配信されるどこからも論証・
承認・その他の状態は変更できません。

```console
inka serve tests/fixtures/aocs
```

<http://127.0.0.1:8080> を開きます。全体表示は objective を最上位にしたトップダウンのレイアウトで，
ノードをクリックすると直接の親子とともにフォーカス，namespace バッジまたは一覧の見出しをクリック
するとその namespace をフォーカスします。ノードにホバーすると description の全文が出て，`Esc` で
全体表示に戻ります。

主なオプション: `--port`(既定 8080)，`--host`(既定はループバック。査読中の論証をネットワークに
公開しない)，`--assets <DIR>`(バンドルが `web/dist` 以外にある場合)。バンドルが無くても
`GET /elements.json` は応答し，これは手順 2 と同じ JSON です。

### 次に読むもの

- [`docs/`](docs/ja/index.md) — 概念，仕様，各出力の設計
- [`web/README.md`](web/README.md) — ビューアと `inka export` の契約，および開発方法
- `inka <command> --help` — 各コマンドの全オプション

## pre-commit のセットアップ

このリポジトリでは，[pre-commit](https://pre-commit.com) と互換性のある [prek](https://prek.j178.dev) を推奨しています。

```console
prek install --hook-type pre-commit --hook-type pre-push
```

`pre-commit` を使う場合は，上のコマンドの `prek` を `pre-commit` に置き換えてください。

## ライセンス

次のいずれかのライセンスで提供します。どちらを適用するかは利用者が選べます。

- Apache License, Version 2.0([LICENSE-APACHE](LICENSE-APACHE) または <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license([LICENSE-MIT](LICENSE-MIT) または <https://opensource.org/licenses/MIT>)

貢献者が明示的に別段の意思表示をしない限り，Apache-2.0 ライセンスの定義に従い，貢献者が本作品に取り込まれることを意図して提出したあらゆる貢献は，追加の条件なしに上記のデュアルライセンスで提供されるものとします。
