# テスト証拠bundle v2

[連携設計](test-evidence-integration.md)がテストの自動公開を定義する。配送形式は結果の収集方式から独立し、版付きdigest識別子と凍結ベクタは表示上の名称に依存しない。

[証拠の意味論](ci-evidence.md)を具体化する配送契約。
英語版（正）: [English](../../en/design/ci-evidence-format.md)。
[JSON Schema](../../../schemas/ci-evidence-v2.schema.json)と
[共通適合ベクトル](../../../tests/golden/evidence/v2/README.md)を提供する。
ベクトルは追記のみとする。ファイルハッシュだけのv1 exportをv2証拠として暗黙に
受け付けない。

## bundle

1つのbundleは1つの証拠元の版と実行試行を表す。必須フィールドは`v: 2`、
`source: {id, revision}`、`execution: {id, attempt}`、`targets`。
`attempt`は正のu32。任意の`environment`は説明用JSONオブジェクトであり、
初期実装では成立条件に影響しない。

| 対象のフィールド | 意味 |
| --- | --- |
| `id` | 証拠元の中で一意な対象ID |
| `kind` | `file`または`function` |
| `profile` | `file-source:v1`等の版付き抽出規則 |
| `content` | 人間が確認する空でないUTF-8ソース本文 |
| `interface_hash` | `sha256:`に小文字16進64桁を続けた値 |
| `required` | 空でなく重複のない必要実行ID集合 |
| `results` | 必要実行についての`{case, result}`レコード |

`result`は`pass`、`fail`、`skip`、`cancelled`。結果の欠落は必要実行が未報告であることを
表す。複数環境を要求する場合、例えば`linux/normalization_is_idempotent`のように
ケースIDで環境を区別する。必要集合を成功結果の集合から推測しない。

識別子・版・抽出規則・ケースの文字列は空白以外の内容を含む。`environment`内部の
任意情報を除き、未知フィールドとオブジェクトキーの重複はエラーとする。対象ID、
必要ID、結果のケースIDはそれぞれの範囲で一意とする。未宣言ケースの結果は不正。
空の`targets`は許容するが、参照された対象を成立させることはできない。

Schemaの構造検査に加え、消費側はIDの一意性、結果の所属、版、ハッシュの再計算を
検証する。構造やハッシュの不整合は評価中断。解釈可能な証拠の不足・非passはbindの
blameとする。bundleファイルの欠落は証拠不足、存在するが読めない・壊れている場合は
入力エラーとする。

## interface digest

`field(s)`はUTF-8バイト長をu64 little-endianで書き、そのバイト列を続けたもの。
次の連結をSHA-256でハッシュする。

```text
field("inka:ci-interface:v2")
field(source.id)
field(target.id)
field(target.kind)
field(target.profile)
field(target.contentのCRLFをLFに置換した本文)
u64-LE(必要IDの数)
field(必要ID 1) ... field(必要ID N)
```

必要IDはUTF-8バイト順に並べる。入力順は影響せず、重複は不正。必要集合を含めるため、
本文を変えずにテスト選択を弱めた場合も再承認が必要になる。

証拠元、対象ID、粒度、抽出規則、本文、必要集合は承認に影響する。ソースの版、
実行・試行、結果、環境情報は影響しない。生32バイトのdigestを既存bind v1の
interface欄へ入れ、既存のbind・recordベクトルを変更しない。

## ローカル参照

manifestには任意の証拠元マップを追加する。

```toml
[evidence.sources.upstream]
id = "github:example/source"
file = "evidence/upstream.json"
```

別名はDSLの識別子文法に従う。`file`はmanifestディレクトリ相対で、実行するソースでは
なく取り込んだデータを指す。同じ証拠元に複数の版が競合しないよう、証拠元IDの重複を
拒否する。bundleの`source.id`は設定と一致しなければならない。

bindは`test: "upstream/tests/example.rs::works"`のように`<別名>/<対象ID>`を指定する。
最初のスラッシュで分け、残り全体を不透明な対象IDとする。不正な参照、未知の別名、
bundle欠落、対象欠落は、該当bindのblameとして報告する。構造の`check`は引き続き
参照の意味を解釈しない。

ローカル別名は配送先を選ぶ設定であり、ハッシュに入る証拠元IDとは異なる。別名だけの
変更では承認を維持できるが、証拠元・対象変更では再承認する。inkaのHEADを証拠元の版と
比較しない。版と試行は証拠元・取り込み手順が選択し、evalはその選択だけを評価する。

## 結果の集約

bundle検証後、対象の必要ケースを評価し、非pass・欠落したケースをすべて報告する。
各必要ケースにちょうど1つのpassがある場合だけ対象をpassとする。空の結果配列や
workflow全体の状態から成功を推定せず、別試行の結果を暗黙に混合しない。

closure情報と環境ポリシーは後続の明示的な拡張で扱う。未知のフィールドは意味を
黙って落とさず、エラーとする。
