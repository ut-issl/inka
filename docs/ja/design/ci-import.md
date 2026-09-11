# 別リポジトリからの証拠取り込み

[取り込み例](../../../examples/ci/README.md) と
[workflow](../../../examples/ci/receive-evidence.yaml) を受信側リポジトリに配置します。
証拠元リポジトリ・run ID・artifact ID を明示して、公式 download-artifact Action
で取得します。別の試行や環境の結果を自動混合しません。

受信側は upstream のテストや言語環境を実行せず、ローカルに取り込んだ v2 JSON
だけを inka で評価します。証拠元の更新を自動追従する仕組みや、取り込み結果の
コミット・PR 作成はこの例には含めません。

別の private repository の Artifact を読むには、対象リポジトリの Actions に
読み取り権限を持つ token を受信側 secret に設定します。
この token は公式 Action のみが利用し、inka には渡しません。

pass の証拠を取得しても承認は別に必要です。本文・証拠元・対象・必要集合の変更は
再承認を要求します。対象名が存在しない場合は bind の blame、解釈できない証拠は
CLI の入力エラーになります。
