# Rust証拠アダプター

実装状況: 以下の例は試作・内部IFの説明であり、通常の導入手順ではない。現在の設計は[連携契約](test-evidence-integration.md)を参照。通常の導入では手書きの対象計画、prepare/finish操作、プロジェクトのテスト設定変更を要求しない。独立した既存テストの自動収集とworkflow内だけの設定は未実装。

通常の`#[test]`関数、`proptest!`内の宣言、インラインモジュールを扱う。
`syn`で構文・トークングループを解析し、spanからソース本文を保持する。
行パターンで関数境界を推測せず、Cargoを実行しない。
英語版（正）: [English](../../en/design/ci-rust.md)。

## 準備と実行

[準備計画](ci-producing.md)の関数対象に`selector`を追加する。値は当該テストバイナリ内の
libtest名（例:`normalization_is_idempotent`、`nested::property`）。必要ケースは
`ENVIRONMENT/SELECTOR`とし、関数名を一致させる。ファイル全体なら`selector`を省略する。

```sh
inka ci-export prepare-rust --plan plan.json --out receipt.json
cargo test -p inka-core --test hash_props -- --format pretty --color never > libtest.txt
inka ci-export finish-rust --receipt receipt.json --report linux=libtest.txt --out ci-export.json
```

テスト実行はworkflowの責務。失敗時も終了・配送処理へ進めつつ、元の失敗を維持する。
準備した実行と同じ実行のレポートを使う。複数環境では`--report ENV=PATH`を繰り返す。
各レポートは1テストバイナリ分とし、workspace全体の出力を混ぜない。filterで実行されない
必要テストは欠落、ignoreは非pass、中断した実行は成功を証明しない。

## 抽出規則と限界

`rust-source:v1`は通常テストの属性と定義を保持する。proptestはブロック共通の内部設定属性と、
選択したpropertyの属性・strategy・本体を`proptest!`で包んで出力する。別propertyの定義は
含めない。外部ヘルパー、import、親モジュール設定は含めず、closure捕捉とは区別する。

別ファイルのモジュールは実ファイルを直接指定する。条件付きの同名定義は曖昧として拒否する。
任意のマクロによるテスト生成、非対応proptest構文、未解決selectorは明示エラーとする。
別フレームワークの属性を暗黙に通常テストとして扱わない。

結果は色なし・通常capture有効のlibtest pretty形式を解析し、行ごとの結果とヘッダー・集計の
件数を照合する。失敗したテストの本文出力は結果として解釈しない。`--nocapture`、
`--show-output`、JSON・terse形式、複数バイナリの混在は使わない。不明な形式は成功を推測せず拒否する。

実行中の変更検査は関数を含むファイル全体に対して行う。別関数の編集は準備し直した際の
interfaceハッシュを維持し得るが、実行途中のファイル変更があれば、そのreceiptの完了を拒否する。
