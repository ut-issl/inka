# 08: CI bind provider(テストターゲット粒度)

対象: `source: ci:<target>` の bind を、CI が生成した ci-export.json と照合して
established にする。ミニマムの bind はこの provider のみ。

## ci-export.json

リポジトリ内(例: `ci/ci-export.json`)に置く。eval はこのファイルを読むだけで、
ネットワークを見ない。

```json
{
  "v": 1,
  "commit": "<git rev-parse HEAD の40桁hex>",
  "environment": { "lockfile_hash": "sha256:..." },
  "targets": [
    {
      "id": "tests/hash_props.rs",
      "interface_hash": "sha256:...",
      "result": "pass"
    }
  ]
}
```

- `interface_hash` = テストファイル内容のハッシュ。正規化は **CRLF→LF のみ**
  (AST 正規化はしない。コメント編集でも bind が剥がれる = 粗いが健全側)
- `result` は `"pass" | "fail"`
- serde で読み、`v != 1`・未知フィールドは error

## export 生成 — `inka ci-export` サブコマンド(推奨)

シェルでも書けるが、生成ロジックを Rust に持つと interface_hash の正規化を実装本体と
共有できるので、サブコマンドにするのを推奨:

1. 対象列挙: `tests/*.rs`(まずは全部でよい)
2. 各ターゲットについて `cargo test --test <name>` を子プロセスで実行し pass/fail を
   記録(exit code で判定)
3. `git rev-parse HEAD`・lockfile の sha256・各ファイルの interface_hash を集めて
   JSON を書く(`--out ci/ci-export.json`)

## GitHub Actions

```yaml
# .github/workflows/inka-export.yml(骨子)
on: push
jobs:
  export:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo run -- ci-export --out ci/ci-export.json
      - run: |
          git config user.name "inka-ci"
          git config user.email "ci@invalid"
          git add ci/ci-export.json
          git commit -m "ci: update inka export" || exit 0   # 変化なしなら何もしない
          git push
```

- export のコミットが積まれた時点の HEAD と export 内 `commit` は1つズレる
  (export は「検証されたコミット」を指す)。ミニマムでは commit の厳密一致検査を
  **warning に緩め、内容の一致は interface_hash の照合(下記3)で担保する**。
  この緩和は rustdoc に明記(厳格化は後からできる)
- push ループ防止: export コミットのパスを workflow の `paths-ignore` に入れる

## eval 側の照合(07 の `ci_matches`)

bind ノードについて:

1. `source` が `ci:<id>` であること(`veriq:` はミニマムでは常に未確立+blame)
2. export に `<id>` のエントリがある(なければ BindNoExport)
3. **現在のワークツリーのそのファイルの interface_hash** と export の
   `interface_hash` が一致(不一致 = テストが検証後に変わった → BindInterfaceChanged)
4. bind 承認の view_hash が「現在の desc + 現在の interface_hash」で再計算した値と
   一致(不一致 = StaleApproval)
5. `result == "pass"`(fail なら BindTestFailed)

3 と 4 の違いに注意: 3 は「export が今のテストを検証したものか」、4 は「承認が今の
契約(desc+テスト)に付いているか」。両方通って初めて established。

## テスト

- フィクスチャに小さな ci-export.json を同梱し、07 のシナリオテスト(export なし /
  interface 変化 / fail / pass)を回す
- `inka ci-export` 自体のテスト: 一時ディレクトリに偽の tests/*.rs を置いて
  JSON の insta スナップショット
- E2E(手動でよい): dogfood の `local_view_hash_sound` を
  `bind { source: ci:tests/hash_props.rs }` に書き換え → approve → eval established、
  テストファイルに1文字足す → eval で BindInterfaceChanged が出る
