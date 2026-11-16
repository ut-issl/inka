# inka のリリース手順

この文書は inka のリリースの作り方を説明します([English](RELEASING.md))。

## バージョンの付け方

- リポジトリ内のすべてのパッケージは同じバージョンを持ちます。対象は Cargo ワークスペース(全クレート)，Python
  プロジェクト(`pyproject.toml` と `crate/inka-evidence-py`)，npm パッケージ(`web`，`editors/vscode`，
  `crate/inka-evidence-wasm`)です。基準は `Cargo.toml` の `[workspace.package]` の version です。
  `[workspace.dependencies]` でワークスペースのメンバーを `path` で指す項目は，同じバージョンを version 要件として
  持ちます。`cargo publish` が path をその要件に置き換えるためです。
- バージョンは [セマンティックバージョニング](https://semver.org/lang/ja/) に従う `MAJOR.MINOR.PATCH` で，プレリリースや
  ビルドの接尾辞は付けません。メジャーバージョンが `0` の間は，互換性を壊すリリースでマイナーバージョンを，それ以外の
  リリースでパッチバージョンを上げます。
- 形式のバージョン(`.inka` 言語，テストエビデンス v1，承認ハッシュ v1)はこれとは別に管理し，形式が変わるときだけ
  上げます。形式を変えたリリースでは，そのことをリリースノートに書きます。
- manifest や lockfile のどれかがワークスペースのバージョンと食い違うと，CI(`repository-check.yaml` の
  `Check package versions agree`)が失敗します。

## リリースで公開されるもの

リリースはタグ `v<version>` と，その上の [GitHub Release](https://github.com/ut-issl/inka/releases) です。GitHub Release
には次の成果物が添付されます(それぞれにビルド来歴の attestation が付き，すべて `SHA256SUMS` に載ります)。バイナリは
`inka`(グラフビューアを内蔵しているので `inka serve` に他のファイルは不要)，`inka-lsp`，`inka-evidence-rust`，
`inka-evidence-merge` の 4 つです。

| 成果物 | 内容 |
| --- | --- |
| `inka-v<version>-<target>.tar.gz`(Windows は `.zip`) | 1 プラットフォーム分の 4 つのバイナリ |
| `inka-vscode-v<version>-<platform>.vsix` | `inka-lsp` を同梱した VS Code 拡張。プラットフォームごとに 1 つ |
| `inka_evidence-<version>-*.whl`，`inka_evidence-<version>.tar.gz` | Python パッケージ `inka-evidence` とその sdist |
| `inka-project-inka-evidence-<version>.tgz` | npm パッケージ `@inka-project/inka-evidence` |

プラットフォームは x86_64 と aarch64 の Linux(musl の静的バイナリ)，x86_64 と aarch64 の macOS，x86_64 の Windows
です。

同じ成果物を，それぞれ専用の GitHub environment から各レジストリに公開します。

| レジストリ | パッケージ | environment | 認証 |
| --- | --- | --- | --- |
| [crates.io](https://crates.io) | `inka-core`，`inka-evidence`，`inka-cli`，`inka-lsp`，`inka-evidence-rust` | `crates-io` | Trusted Publishing(OIDC) |
| [PyPI](https://pypi.org) | `inka-evidence` | `pypi` | Trusted Publishing(OIDC) |
| [npm](https://www.npmjs.com) | `@inka-project/inka-evidence` | `npm` | Trusted Publishing(OIDC)，ステージ公開 |
| [VS Code Marketplace](https://marketplace.visualstudio.com) | `inka-project.inka-vscode` | `vscode-marketplace` | Trusted Publishing(OIDC) |
| [Open VSX](https://open-vsx.org) | `inka-project.inka-vscode` | `open-vsx` | secret `OVSX_PAT` |

バインディングのクレート `inka-evidence-py` と `inka-evidence-wasm` は crates.io には公開しません(`publish = false`)。
これらは Python パッケージと npm パッケージとして利用者に届きます。

公開名(npm のスコープ，Marketplace の publisher，Open VSX の namespace としての `inka-project`)は，意図的にリポジトリの
GitHub organization から独立させています。Trusted Publisher はリポジトリに結び付いているので，リポジトリを移すときは
それらを更新するだけで，パッケージ名を変える必要はありません。

## 初回だけの準備

各レジストリの準備は，そのアカウントの所有者が一度だけ行います。準備ができていないレジストリの publish job は失敗しますが，
リリースの残りは完了します。[リリースを直すとき](#リリースを直すとき) を参照してください。

1. **GitHub environment。** Settings → Environments で `crates-io`，`pypi`，`npm`，`vscode-marketplace`，`open-vsx` を
   作ります。必要なら required reviewers を付けると，各レジストリへの公開前にリリースが一時停止します。以下の Trusted
   Publisher は，これらの environment 名とワークフローファイル `release.yaml` に結び付けます。
2. **crates.io。** 各クレートの最初のバージョンは，所有者の API トークンで手動で公開する必要があります(依存順に
   `cargo publish -p <crate>`: `inka-evidence`，`inka-core`，`inka-evidence-rust`，`inka-lsp`，`inka-cli`)。Trusted
   Publishing は存在するクレートにしか設定できないためです。その後，各クレートの Settings ページで，リポジトリ
   `ut-issl/inka`，ワークフロー `release.yaml`，environment `crates-io` の Trusted Publisher を追加します。所有権が 1 人に
   依らないよう，organization の GitHub team を owner に加えることを検討してください。
3. **PyPI。** プロジェクト `inka-evidence` の *pending* Trusted Publisher(owner `ut-issl`，repository `inka`，workflow
   `release.yaml`，environment `pypi`)を追加します。最初のリリースがプロジェクトを作ります。
4. **npm。** `inka-project` organization がなければ作ります。`@inka-project/inka-evidence` の最初のバージョンは手動で公開する必要が
   あります(GitHub Release の tarball を，そのスコープにパッケージを作れるアカウントで `npm publish`)。Trusted Publisher は
   存在するパッケージにしか設定できないためです。その後，パッケージの Settings で `ut-issl/inka`，ワークフロー
   `release.yaml`，environment `npm` の Trusted Publisher を追加します。許可する操作は `npm stage publish` だけにします。
   ワークフローは各バージョンをステージするだけで，公開は保守者が行う(手順の 4)ので，自動化だけで公開されることは
   ありません。*Publishing access* は「2 要素認証を必須にし，トークンを禁止する」設定にします。
5. **VS Code Marketplace。** [Marketplace の publisher 管理](https://marketplace.visualstudio.com/manage) で publisher
   `inka-project` を作ります(個人の Microsoft アカウントで十分で，Azure DevOps の組織やトークンは不要です)。publisher の
   *Trusted Publishing* タブで，Repository owner `ut-issl`，Repository `inka`，Workflow `release.yaml`，Environment
   `vscode-marketplace` のポリシーを作ります。publisher が 1 つのアカウントに依らないよう，*Members* に他の保守者も
   加えてください。
6. **Open VSX。** [open-vsx.org](https://open-vsx.org) に GitHub でサインインし，namespace `inka-project` を作って
   [publisher agreement](https://github.com/EclipseFdn/open-vsx.org/wiki/Namespace-Access) に従って所有権を申請し，
   アクセストークンを作って `open-vsx` environment の secret `OVSX_PAT` に保存します。

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
   なります。どちらかのファイルに節がないと，リリースワークフローはリリースを拒否します。
3. プルリクエストを作り，CI が通ったら `main` にマージします。

### 2. 任意: dry run

Actions タブで `Release` ワークフローを，`mode` を既定の `dry-run` のまま実行します。dry run はどのブランチからでも
実行できます。リリースと同じ検査を行い，すべての成果物をリリースと同じ手順でビルドし，クレートに
`cargo publish --dry-run`(アップロード以外のすべて)を実行し，成果物を実行結果の Summary にサイズ付きで一覧しますが，
何も作らず公開もしません。タグがすでに存在する，CHANGELOG に節がない，といった問題は失敗ではなく警告として報告されるので，
バージョンがすでにリリース済みのブランチでもビルドの予行ができます。各成果物は実行結果の workflow artifact として
ダウンロードできます。

### 3. リリースする

`main` の先頭のコミットで CI が通ってから，`main` で `Release` ワークフローを，`mode` を `release` にして実行します。
ワークフロー自身は CI の結果を確認しません。

ワークフローは `Cargo.toml` のバージョンをタグ `v<version>` としてリリースします。タグは自分で push しないでください。
ワークフローが GitHub Release と一緒に，起動したコミットにタグを作ります。次のどれかに当てはまると，何も作らずに失敗します。

- `main` 以外から起動した
- バージョンが食い違う manifest や lockfile がある
- タグがすでに存在する
- `CHANGELOG.md` か `CHANGELOG.ja.md` にそのバージョンの節がない
- 成果物のどれかがビルドに失敗した
- クレートのどれかがパッケージ化に失敗した

GitHub Release ができた後，各レジストリの job が並行して走ります。あるレジストリが拒否しても，他は止まりません。

### 4. リリースを確認する

[GitHub Release](https://github.com/ut-issl/inka/releases) のタイトルが `v<version>` で，本文が `CHANGELOG.md` の
内容になっており，上の表の成果物がすべて添付され，最新リリース(Latest)になっていることを確認します。続いて，各レジストリ
にそのバージョンが現れていることを確認します。

npm ではバージョンはステージされただけで，まだ公開されていません。npm アカウントに 2FA を設定した保守者が，npmjs.com の
パッケージページの *Staged packages* タブか，ログイン済みのマシンから承認します。

```console
npm stage list @inka-project/inka-evidence
npm stage view <stage-id>       # tarball を確かめるなら `npm stage download <stage-id>`
npm stage approve <stage-id>    # 取り下げるなら `npm stage reject <stage-id>`
```

承認するまで `npm install @inka-project/inka-evidence@<version>` は解決しません。

## リリースを直すとき

公開したタグは動かしません。リリースに問題が見つかったら，`main` で直して新しいパッチリリースを出します。
文面だけが間違っているときは GitHub 上で GitHub Release の本文を直接編集し，対応する `CHANGELOG` の節はプルリクエストで
直します。

レジストリの job が失敗したとき(Trusted Publisher が未設定，Open VSX のトークンの期限切れ，レジストリの障害など)は，原因を直してから
同じ実行で *Re-run failed jobs* を使います。GitHub Release と他のレジストリはそのまま残り，失敗した job だけがその実行で
ビルドした成果物を公開します。レジストリへの公開は取り消せないので，公開済みのバージョンに対してワークフロー全体を再実行
してはいけません。

再実行は起動したコミットのワークフローファイルで走るので，`release.yaml` 自体の修正は反映されません。原因がワークフローに
あるときは，修正を `main` にマージして新しいパッチリリースを出します。そのレジストリには壊れたバージョンが載らず，その
バージョンのリリースノートにそう書きます。
