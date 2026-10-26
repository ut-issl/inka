# テスト証拠の契約

英語版（正）: [English](../../en/design/test-evidence.md)。

## 責務分担

inkaのprojectがあるレポジトリをプロジェクトレポジトリ、inkaのevidenceとして用いるテストが含まれるレポジトリをソースレポジトリと定義する。両者は同一のレポジトリでもよい。

ソースレポジトリがテストを選択し、自身の環境で実行する。ソースレポジトリ側がinkaのCLIやGitHub Actionなどのアダプターを使って、inkaのprojectがテストの実行結果を認識できる証拠jsonファイルを出力する。

実行結果の保存と配送はcapsulaが担う。ソースレポジトリはテストをcapsulaのrunとして実行し、証拠jsonをrunの成果物として保存する。capsulaのディレクトリ保存・サーバー保存(push/pull)が、プロジェクトレポジトリ側への配送を兼ねる。inkaはこの保存・配送機構を実装せず、その責務を負わない。

inkaのprojectは、設定されたパスに最新の証拠jsonファイルがある前提でevidenceを読み込む。当該のjsonが最新かどうかについては、ソースレポジトリからの配送機構が責任を持つ。inkaはcapsulaのvault構造を解釈せず、証拠jsonのファイルパスだけを扱う。評価は取り込んだ版に対してオフラインで行い、ソースレポジトリのcheckout・コンパイラ・テストランナー・ネットワークを必要としない。

jsonにはあくまでもソースレポジトリが提供するテストとその実行結果が記載されているのみである。これがinkaの命題と対応しているかの判断は、bindノードに対するreview(人間の承認)が担う。

## inkaのbind用証拠jsonファイル

bindは `test: "<test-id>"` で証拠を参照する。test-idはソースレポジトリ側が付与する不透明な識別子であり、ファイル名で始まるのか、レポジトリ指定を含むのかといった内部構造をinkaは解釈しない。inkaに必要なのは、証拠を一意にidentifyできることだけである。読み込んだ証拠の中でtest-idが重複した場合は入力エラーとする。

証拠jsonはリポジトリ直下の `schemas/test-evidence-v1.schema.json` に従う。test-idごとに次を記載する。

- **id** — bindが参照するtest-id。
- **body** — 人間が確認できるテストのソース文字列(空でないUTF-8)。属性・デコレータ・パラメータなど何をどこまで含めるかの抽出はソースレポジトリ側アダプターの責務であり、inkaは言語固有の構文を解析・実行しない。
- **executions** — このtestをpassとするために必要な実行の宣言と結果。リスト全体が必要集合であり、
  `outcome`(pass / fail / skip / cancelled)を省略した項目は「宣言されたが結果が欠落した実行」を意味する。
  各項目は `case`(ケースID)で識別する。項目は1件以上とし、ケースIDの重複は入力エラー。

ファイル先頭の **provenance** には、テストしたソースの版やcapsulaのrun IDなどの記録情報を書く。承認対象には含めず、inkaは未知のキーを無視する。

```json
{
  "v": 1,
  "provenance": {
    "source_revision": "4f2a1c9",
    "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2"
  },
  "tests": [
    {
      "id": "example-repo/tests/hash_props.rs::roundtrip",
      "body": "#[test]\nfn roundtrip() { /* ... */ }",
      "executions": [{ "case": "default", "outcome": "pass" }]
    }
  ]
}
```

契約本文はハッシュ値だけでなく、人間が確認できる形で証拠jsonに含める。inkaは承認インターフェースへ本文を提示する。
証拠jsonは申告ハッシュを運ばない。interfaceハッシュはinkaが読み込み時に計算し、配送中の内容検証はcapsulaの責務とする。

必要な実行の集合は証拠json自身が宣言する。成功した結果だけから必要集合を推測せず、途中で中断した実行によって集合を縮めない。別のrunの成功結果を寄せ集めない。

## 内容のハッシュと再承認

interfaceハッシュはtest-id、契約本文、必要ケースIDの集合から計算する。
本文の正規化はCRLFをLFへ置換するだけとし、コメント、Unicodeの正規化状態、字下げ、行末空白は内容として残す。
フレーミングは[確定仕様](spec.md)のview_hashと同じ規則(u64-LE長の接頭辞による `field` / `list`)を使う。

```text
preimage = field("inka:test-evidence:v1") ‖ field(test-id)
           ‖ field(正規化した契約本文) ‖ list(必要ケースID)
interface_hash = SHA-256(preimage)
```

必要ケースIDはバイト順にソートして並べる。executionsの順序変更は編集ではない。実行結果(outcome)はハッシュに入らない。

bindのview_hashは凍結済みv1のままとする。命題の説明・注釈、interfaceの生digest、現在は空の仮定リストを含む。run ID、実行結果、環境情報は承認契約に含めない。

契約本文・test-id・必要実行集合のいずれかが変われば再承認が必要である。
ソースレポジトリが失敗するケースを黙って必要集合から外しても、既存の承認のままpassに見えることはない。
同じinterfaceに対する新しい実行結果は、既存の承認で評価できる。承認のstaleと実行の失敗は独立した原因として報告し、
承認のstaleをテスト失敗と呼ばず、同じ契約内容のテストが失敗しただけで再承認を要求しない。

この未署名プロトコルでの出自は記録情報である。digestの再計算は内容の矛盾を検出するが、実行者の認証や全実行の報告を証明しない。署名は独立した認証機構として扱う。

## bindの評価

ノードが期限切れでなく、現在のハッシュにdeclineがない前提で、bindは有効な承認とpassの証拠の両方がそろったときに成立する。既存の承認定足数・拒否権・期限の規則は維持する。

| 観測した状態 | bindへの意味 |
| --- | --- |
| 必要な全実行がpass | 証拠によって成立し得る |
| 必要な実行が1件でもfail | 証拠によって成立しない |
| 必要な実行がskipまたはcancel | 証拠によって成立しない |
| 必要な実行の結果が欠落 | 証拠によって成立しない |
| 参照したtest-idが証拠に存在しない | 証拠なし。成立しない |
| 同じ実行に矛盾・重複した結果 | 入力不正として評価を中断 |

| 取り込んだ証拠と承認 | 評価 |
| --- | --- |
| 契約本文A、対応する承認、必要な全実行がpass | 成立 |
| 対応する承認あり、必要な実行がfail | 未成立。証拠の失敗 |
| 証拠はpass、承認なし | 未成立。承認なし |
| 本文Bを取り込み、承認はAのものだけ | 未成立。承認がstale |
| 本文Aのまま、新実行がpass | 承認維持。成立 |
| 本文Aのまま、新実行がfail | 承認維持。未成立 |
| ソースレポジトリだけ編集し、取り込み済み本文はA | 編集前と同じ評価 |
| 同じ本文で別のtest-idへ変更 | 変更後のinterfaceを承認するまで未成立 |
| 本文は同じまま、必要実行集合を変更 | 変更後のinterfaceを承認するまで未成立 |

## capsula経由の配送

以下のフローは2026-10-26に、別々の2リポジトリとPostgreSQLバックエンドの
capsulaサーバで実地検証した。Rustアダプターが出力した証拠は `capsula push`
と `capsula pull` だけでリポジトリ境界を越え、inkaは転送を一切実装せずに
プロジェクト側で成立まで評価できた。`capsula pull` を持つcapsula
(リリース0.14.0より新しいもの)が必要。

### ソース側

ソースレポジトリは `capsula run` の中でテストを実行し、アダプターが証拠
jsonを書き、`capture-file` フックがそれをrun成果物として保存する。
`capsula.toml` に:

```toml
server = "https://capsula.example.com"

[vault]
name = "telemetry-ci"

[[pre-run.hooks]]
id = "capture-git-repo"
name = "telemetry"
path = "."

[[post-run.hooks]]
id = "capture-file"
glob = "evidence.json"
mode = "copy"
```

```console
$ capsula run -- inka-evidence-rust --output evidence.json \
    --provenance source_revision=$(git rev-parse HEAD)
$ capsula push <RUN_NAME>
```

`capsula run` は2つの識別子を表示する: runのULID(`<RUN_ID>`。エクスポート
される `CAPSULA_RUN_ID` が証拠の `capsula_run_id` provenanceに書き込むのと
同じ値)と、生成された名前(`<RUN_NAME>`。例 `uppity-bead`)で、後者は
プロジェクト側の `capsula pull` も再表示する。テストビルドが生成するものは
すべてコミットするか無視リストに入れること(アダプターの `evidence.json`
もlockfileも含む): `capture-git-repo` フックはダーティなツリーを拒否する
ため、初回runのビルドが生成したlockfileが2回目のrunを中断させる。

### プロジェクト側

プロジェクトレポジトリはrunを復元し、その成果物をマニフェストに設定した
パスへ接続する:

```console
$ capsula pull <RUN_ID>
$ cp "$(capsula run-dir <RUN_NAME>)/post-0-capture-file/evidence.json" \
    evidence/test-evidence.json
```

```toml
[evidence]
files = ["evidence/test-evidence.json"]
```

コピー方式なら、設定パスはvaultの日付入りrunディレクトリに依存しない。
pullしたrunの場所は名前から `capsula run-dir` で解決できる。パス中の
`post-0-capture-file/` はcapsulaのフック出力レイアウト(添字はフックの
順序に従う)で、このシェル手順にだけ現れる — inkaがvaultレイアウトを解釈
しないことは変わらない。配送を信頼する前に、pullしたrun IDと証拠自身の
provenanceを突き合わせる:

```console
$ jq -e --arg run "<RUN_ID>" '.provenance.capsula_run_id == $run' \
    evidence/test-evidence.json
```

責務は責務分担の節のとおり: capsulaはpull時にサイズとSHA-256を検証し、
pullで復元したrunの再pushを拒否する。設定パスを最新に保つのは、責務分担の
節がinkaの外に置いた配送機構の一部 — このフローでは配送を運用する者が行う
`pull` と `cp` の手順 — であり、inkaは設定パスをそのまま読むだけ。
このパスはマニフェスト相対で、意図的に制限しない — 絶対パスも `..` も
許容する。証拠の置き場所を宣言するのはマニフェストの作者だからである。

1つの証拠ファイルは1つの実行構成を運ぶ。複数構成(プラットフォーム、
featureセット)を1ファイルの複数caseへ併合する仕組みは後続作業とし、
それまでは構成ごとに別ファイル(test-idを分ける)か単一構成で運用する。

## スコープ外・後続作業

- 複数構成の証拠: 複数runの実行結果を1ファイルのcase集合へ併合する仕組みと、アダプターが付ける実行構成ラベル。
- 環境情報(lockfileハッシュなど)の記録と、環境変更の警告・エラーポリシー。
- 署名による実行者の認証。
