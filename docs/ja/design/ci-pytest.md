# pytest証拠アダプター

実装状況: 以下の例は試作・内部IFの説明であり、通常の導入手順ではない。現在の設計は[連携契約](test-evidence-integration.md)を参照。通常の導入では手書きの対象計画、prepare/finish操作、プロジェクトのテスト設定変更を要求しない。独立した既存テストの自動収集とworkflow内だけの設定は未実装。

証拠元のPython環境へ`adapters/pytest`をインストールする。inka自体がpytestの環境を
用意するわけではない。完全な設定例は[英語README](../../../adapters/pytest/README.md)を参照。

計画は`v: 1`、`source: {id, revision}`、`execution: {id, attempt}`、
`targets: [{id, file, selector}]`。ファイルは計画相対、selectorは関数名または
`TestClass::test_method`で指定する。

```sh
pytest --inka-plan selection.json --inka-output evidence-run --inka-environment linux
inka ci-export finish --receipt evidence-run/receipt.json --results evidence-run/results.json --out ci-export.json
```

`--inka-plan`がない場合、プラグインは動作しない。CLIは`--inka-bin`または`INKA_BIN`で指定できる。
実行ごとに新しい出力ディレクトリを使う。CIではテスト失敗後もfinishを実行し、元の失敗は維持する。

`pytest-source:v1`はASTで関数・メソッド本体とデコレータを抽出する。fixture本体、親クラス、
import、外部ヘルパーは含めない。収集したパラメータ化ケースを必要集合とし、runnerの選択変更は
interfaceハッシュを変更する。途中終了した未実行ケースは未報告のまま残す。

setup・call・teardownを別々に確認し、teardown失敗やskip・xfailを成功にしない。
実行中のソース変更はfinishで拒否する。対応範囲は単一プロセスのpytestとし、xdist、再実行
プラグイン、外部でラップ・生成された関数、曖昧なselectorは成功を推測せず拒否する。
