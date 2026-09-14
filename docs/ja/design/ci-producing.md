# テスト証拠の生成

本書は収集処理の内部インターフェースを説明する。workflowへの導入は[連携設計](test-evidence-integration.md)に従い、準備・対象の発見・結果の組み立ては連携処理が行う。テスト作成者に手動操作を要求しない。

テスト実行は証拠元が管理する。inka CLIが実行前の内容を記録し、実行後に独立して
供給された結果を結び付ける。
英語版（正）: [English](../../en/design/ci-producing.md)。

## ファイルの準備

収集連携が生成するJSON計画には`v: 1`、実際の証拠元の`source: {id, revision}`、現在の
`execution: {id, attempt}`、`targets`を指定する。対象は
`{id, file, required}`。`file`は計画ファイル相対、`required`は成功結果から
推測せず宣言した必要ケース集合とする。完全なJSON例は英語版を参照。

```sh
inka test-export prepare --plan plan.json --out receipt.json
```

結果のないv2 bundleと、ソースの変更検査用情報をローカルのreceiptへ記録する。
receiptの絶対パスはローカル準備用であり、完成したbundleだけを配送する。
準備コマンドはテストを実行しない。

## 抽出済み契約

Rust・pytest・Vitestの連携が宣言とケースを発見し、関連属性・デコレータを含む本文を
抽出する。共通生成機能は言語を解析せず、テストも実行しない。
入力は連携が生成し、テスト作成者に計画・ケース一覧・抽出コードの保守を要求しない。

対象には`{id, file, required}`の代わりに
`{id, kind, profile, content, required, source_guards}`を指定できる。
`kind`は`file`または`function`、`profile`は抽出規則を識別する。
共通機能が受け取った契約と必要ケースからinterface hashを計算する。
同じ計画に両形式の対象を含めてよいが、各対象は一方の形式だけを使い、未知フィールドは拒否する。

`source_guards`は空でない`{path, sha256}`の配列で、パスは計画ファイル相対。
収集側はテスト実行前に、抽出に使ったファイルの生バイト列をSHA-256でハッシュする。
値は`sha256:`と小文字16進64桁。interface digestと異なり改行を正規化しない。
準備時に照合し、絶対パスをローカルreceiptへ記録して完了時にも再照合する。
抽出から準備まで、または準備から完了までにソースが変わればエラーとする。
検査対象ファイル内では、抽出関数の外側の変更も検出する。

抽出の正しさ、必要ケースの発見、実行との対応は収集連携の責任とする。
ファイルの検査は検査時点の変更を検出するもので、抽出の正しさや実行依存全体を保証しない。
結果には準備済みinterface hashを再利用する。共通IFはjob全体の成功から推論せず、再実行もしない。

準備receiptには少なくとも1つの対象とソース検査情報が必要。空の計画は成功証拠ではなく
エラーとする。receiptの絶対パスは証拠元workspaceを指し、完成bundleだけが可搬形式となる。
別jobへ収集を移す場合は、連携側が必要なソース・snapshotを配送し、ローカルパスを
共有ストレージとして扱わない。

## 結果と完了

既存workflowでテストを実行し、アダプターから結果JSONを供給する。形式は`v: 1`、
同じ`source`と`execution`、`targets`内の
`{id, interface_hash, results: [{case, result}]}`。ハッシュは準備済み対象のものを使う。
job全体の状態から選択した関数が実行されたと推定せず、実際の結果を渡す。

```sh
inka test-export finish --receipt receipt.json --results results.json --out test-export.json
inka test-export validate test-export.json
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
