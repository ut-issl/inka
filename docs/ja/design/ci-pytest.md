# pytest証拠アダプター

本書は収集処理の内部インターフェースを説明する。workflowへの導入は[連携設計](test-evidence-integration.md)に従い、準備・対象の発見・結果の組み立ては連携処理が行う。テスト作成者に手動操作を要求しない。

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
