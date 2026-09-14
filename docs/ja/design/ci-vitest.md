# Vitest 証拠アダプター

本書は収集処理の内部インターフェースを説明する。workflowへの導入は[連携設計](test-evidence-integration.md)に従い、準備・対象の発見・結果の組み立ては連携処理が行う。テスト作成者に手動操作を要求しない。

証拠元に配置する Vitest reporter です。テスト実行・環境構築は証拠元の責任とし、
inka は配送された証拠だけを評価します。

TypeScript AST で選択した `test` / `it` 宣言（引数・コールバック込み）を抽出し、
実際の実行結果と共通の `prepare` / `finish` IF で結び付けます。
関数名が曖昧な場合や対応外の動的生成はエラーです。
skip・未実行・中断・フックの失敗を pass として扱いません。
実行前のソースと finish 時の内容が違う場合もエラーになります。

初期対応は Vitest 5.0.0 / TypeScript 5.9.3 です。
設定例・対応範囲・検証方法は [アダプター README](../../../adapters/vitest/README.md) を参照してください。
