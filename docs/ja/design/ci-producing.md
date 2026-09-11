# CI証拠の生成

テスト実行は証拠元が管理する。inka CLIが実行前の内容を記録し、実行後に独立して
供給された結果を結び付ける。
英語版（正）: [English](../../en/design/ci-producing.md)。

## ファイルの準備

JSON計画には`v: 1`、実際の証拠元の`source: {id, revision}`、現在の
`execution: {id, attempt}`、`targets`を指定する。対象は
`{id, file, required}`。`file`は計画ファイル相対、`required`は成功結果から
推測せず宣言した必要ケース集合とする。完全なJSON例は英語版を参照。

```sh
inka ci-export prepare --plan plan.json --out receipt.json
```

結果のないv2 bundleと、ソースの変更検査用情報をローカルのreceiptへ記録する。
receiptの絶対パスはローカル準備用であり、完成したbundleだけを配送する。
準備コマンドはテストを実行しない。

## 結果と完了

既存workflowでテストを実行し、アダプターから結果JSONを供給する。形式は`v: 1`、
同じ`source`と`execution`、`targets`内の
`{id, interface_hash, results: [{case, result}]}`。ハッシュは準備済み対象のものを使う。
job全体の状態から選択した関数が実行されたと推定せず、実際の結果を渡す。

```sh
inka ci-export finish --receipt receipt.json --results results.json --out ci-export.json
inka ci-export validate ci-export.json
```

`finish`はソースの未変更、版、実行試行、対象ID、digest、ケースの所属を検証する。
有効な失敗・不足の証拠も書き出し、全対象がpassでなければ非ゼロ終了する。不正な
結果や実行中のソース変更では、新しいbundleを生成しない。失敗時に以前のファイルを
誤配送しないよう、実行ごとに新しい出力先を用いる。

`validate`はテストの成功ではなくbundleの整合性を検証する。失敗の証拠も配送できるよう、
正常なfail bundleの検証は成功する。元のテストstepの失敗状態はworkflowで維持する。

環境情報は必須でない。provider連携でv2の任意の`environment`へ説明情報を追加できるが、
interface digestや初期の成立条件は変わらない。関数アダプターも同じreceipt・結果・bundle
の境界を利用し、消費側へ言語固有の処理を追加しない。
