# inka

説明付きの承認プロセスによって論証化の構造化をするツール
テキストベースかつLSP対応で、反復的な開発に適した設計

## インストール

各 [GitHub Release](https://github.com/ut-issl/inka/releases) は，すべてのパッケージを同じバージョンで，x86_64 と aarch64 の
Linux，x86_64 と aarch64 の macOS，x86_64 の Windows 向けに配布します。

- **コマンドライン。** リリースから `inka-v<version>-<target>.tar.gz`(Windows は `.zip`)をダウンロードし，`inka`，
  `inka-lsp`，`inka-evidence-rust`，`inka-evidence-merge` を `PATH` に置きます。Rust ツールチェインがあれば，
  `cargo binstall inka-cli` が同じアーカイブを取得し，`cargo install --locked inka-cli`(または `inka-lsp`，
  `inka-evidence-rust`)が [crates.io](https://crates.io/crates/inka-cli) からビルドします。
- **VS Code。** [Marketplace](https://marketplace.visualstudio.com/items?itemName=inka-project.inka-vscode) または
  [Open VSX](https://open-vsx.org/extension/inka-project/inka-vscode) から `inka-project.inka-vscode` をインストールします。
  `inka-lsp` を同梱しています。リリースには *Install from VSIX…* 用の `.vsix` も添付されています。
- **Web UI。** `inka` に内蔵されているので，`inka serve` に他のファイルは不要です。自分でビルドしたバンドルを
  使うときは `--assets <DIR>` で指定します。
- **テストエビデンスのアダプタ。** `pip install inka-evidence`(pytest，[PyPI](https://pypi.org/project/inka-evidence/))と
  `npm install --save-dev @inka-project/inka-evidence`(Vitest，[npm](https://www.npmjs.com/package/@inka-project/inka-evidence))。
  Rust のアダプタ `inka-evidence-rust` はコマンドラインのアーカイブに入っています。

リリースの `SHA256SUMS` にすべての成果物が載り，各成果物には GitHub のビルド来歴の attestation が付きます
(`gh attestation verify <file> --repo ut-issl/inka`)。リリースの作り方は [RELEASING.ja.md](RELEASING.ja.md) を参照して
ください。

## QuickStart

inka のプロジェクトは，`inka.toml` と `argument/` 以下の `*.inka` を持つディレクトリです。
このリポジトリには 2 つ入っているので，以下はリポジトリのルートでそのまま実行できます。
`tests/fixtures/aocs`(衛星の姿勢制御の論証)と `tests/fixtures/dogfood`(inka 自身についての論証)です。

コマンドには Rust ツールチェイン，手順 3 の Web UI には Node.js が必要です。

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
機械可読なインタフェースです。JSON を読めるものなら何でも利用できるので，論証を読む手段が内蔵の
Web UI だけに閉じることはありません。

```console
inka export tests/fixtures/aocs > aocs.elements.json
```

各ノードは id・discharge モード・namespace・展開済みの description・タグ・objective の根かどうか・
証拠ターゲット・期限・付随するコメントを持ち，各辺は親から 1 つの子を指して，その子の項目に付いた
コメントを持ちます。出力は決定的なので，実行ごとの差分がきれいに取れます。
`check` を通らない論証は何も書き出しません。

### 3. Web UI をビルドする

Web UI は [`web/`](web/README.md) 以下の静的なフロントエンドバンドルで，`inka` がコンパイル時に埋め込みます。最初に
一度ビルドし，その後 `inka` をビルドし直して取り込ませます。

```console
cd web
npm install
npm run build
cd ..
cargo build -p inka-cli
```

`crate/inka-cli/web-dist` が生成され，クレートのビルドスクリプトがこれをバイナリに取り込みます(Web UI 入りの `inka`
を得るには，`cargo install --locked --path crate/inka-cli` はこの手順の前ではなく後に実行してください)。リリースの
バイナリには最初から入っています。

### 4. ブラウザでレビューし，探索する

`serve` は Web UI を配信し，そこに論証を渡します。このように起動した場合は read-only で，配信される
どこからも論証・承認・その他の状態は変更できません。ページを読み込むたびにプロジェクトを読み直すので，編集や新しい
承認はサーバを再起動しなくても再読み込みで反映されます。`check` に通らないプロジェクトは，直るまで
その診断として表示されます。

```console
inka serve tests/fixtures/aocs
```

<http://127.0.0.1:8080> を開きます。面は 2 つあり，どちらにもサイドバーがあって，レビューキューと
全ノードを一覧し，id と description で検索できます。

- **Review** は `inka review --list` が出力するキューを，対象 1 つずつ辿ります。各対象は局所ビュー
  (description，証拠への参照，直接の子の description，期限と，それぞれに付いたコメント)として表示され，
  description は Markdown として描画されます。
- **Graph** は論証全体を，各ノードのモード・id・ステータス・description を載せたカードとして表示
  します。objective とその直接の子が見える状態で開き，カードを展開して深く進み，カードをクリックすると
  その局所ビューがグラフの横に開きます。`Esc` で閉じます。

すべてのノードは `inka eval` の評価結果を示し，`established n/m` の件数は常に表示されます。

ブラウザでレビューするには，承認者を指定して起動します。

```console
inka serve tests/fixtures/dogfood --as iwata
```

このときレビュー面は，判断を受けられる各対象の下に承認と decline を出し，判断は
`inka review --as iwata` が書くのと同じレコードを `approvals/iwata.jsonl` に追記します。サーバが変更
できるのはこれだけです。誰として記録するかはサーバ起動時に固定され(`--key-id` は `inka review` と
同じく key id を指定します)，記録するサーバはループバックアドレスでのみ待ち受け，自分のページからの
判断だけを受け付けます。また，対象が画面に出ていた内容のままである場合にだけ記録します。

主なオプション: `--port`(既定 8080)，`--host`(既定はループバック。査読中の論証をネットワークに
公開しない)，`--assets <DIR>`(内蔵のバンドルの代わりにディスク上のものを配信する)。手順 2 の
cytoscape JSON は `inka export` でのみ得られ，サーバは配信しません。

### 5. 自分のプロジェクトを始める

`init` は新しいプロジェクトの雛形を作ります。`inka.toml`，空の `values.toml`，そして
`argument/objective.inka` の最初の論証(fiat を 1 つ子に持つ objective)で，そのままで `check` と
`fmt --check` を通ります。

```console
inka init my-argument --phases BBM,EM,FM
```

ディレクトリがなければ作成します。プロジェクト名は既定でディレクトリ名(`--name` で上書き)，
`--phases` の先頭が現在のフェーズになります(既定は `M0` のみ)。`init` はファイルを追加するだけで，
作成するファイルのどれかが既にあれば何も書かずに失敗します。

### 6. 評価して承認する

`check` が問うのは論証が正しい形をしているかで，`eval` が問うのはそれが今成り立っているかです。
成立しているノードの数と，成立していない各ノードについてその理由を報告します。すべてのノードが成立
しているときだけ終了コード 0 で終わり，`--json` を付けると同じ結果を他のツール向けに書き出します。

```console
inka eval my-argument
```

作ったばかりのプロジェクトは，まだ誰も承認していないので何も成立していません。`review` は判断を
待っている対象を示し，それに対する判断を記録します。

```console
inka review --path my-argument --as alice --list
inka review --path my-argument --as alice premise --approve
inka review --path my-argument --as alice goal --decline --comment "state a real objective"
```

`--list` も対象も指定しなければ，`review` は待ち行列を対話的に順に巡ります。待ち行列は部分木ごとに
葉から根へ向かって並ぶので，あるブロックはその下にあるすべてのブロックの後に出てきます。判断はすべて
`approvals/<approver>.jsonl` に追記され，書き換えられることはありません。承認はそのとき示された内容に
対するものなので，ブロックを編集するとその承認は古くなり，そのブロックの契約に依拠する親の承認も
古くなります。承認が古くなったブロックは待ち行列に戻ります。

### 7. 整形して編集する

`fmt` はすべての論証ファイルを正準のレイアウトに書き換えます。`fmt --check` は変更されるファイルを
列挙し，1 つでもあれば失敗するので CI に向いています。

```console
inka fmt my-argument
```

編集には [VS Code 拡張](editors/vscode/README.md) が `inka-lsp` を動かします。入力に追従する診断，
補完，ホバー，定義へ移動，参照の検索，リネーム，インレイヒント，承認状態のコードレンズ，そして `inka fmt`
と全く同じ出力の整形を提供します。

### 次に読むもの

- [`docs/`](docs/ja/index.md) — 概念，仕様，各出力の設計
- [`web/README.md`](web/README.md) — Web UI と `inka serve` の契約，および開発方法
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
