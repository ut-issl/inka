# bind: evidence provider の一般化と GitHub CI(テスト)provider の追加

- **ラベル:** enhancement
- **マイルストーン:** M8 周辺 — ただし veriq アダプタより**先に**、最初の実 bind
  provider として実装する有力候補(仮定の集約が不要なため、veriq より厳密に単純)
- **依存:** M3(bind をスタブにした `inka eval`)、ドラフト06(`inka check`)

## 動機

`source:` は現在 veriq 参照のみを受け付ける。CI で実行されるテストコードは同じ役割を
果たせる: 人間の bind 承認が「このテストはこの命題の証拠として適切」を表明し、
CI が真実性の側(このコミットで pass するか)を供給する。

veriq 連携は最初からアダプタ1枚(JSON)に隔離されている。この Issue はその継ぎ目を
**provider 契約**として一般化し、`ci:` を2番目の provider として追加する。
ドッグフーディング上の見返り: dogfood 論証で `fiat until:M2 promote:bind` になっている
命題(例 `local_view_hash_sound`)を、**inka 自身の CI で走る本物の proptest に bind**
できるようになる。

## provider 契約

bind が意味を持つために、どの evidence provider も次の4つを供給する:

1. **内容アドレスの同一性** — 証拠の*界面*のハッシュ(テストなら正規化した関数 AST。
   名前は同一性ではない)。bind 承認はこのハッシュに束縛され、テストの編集は承認を
   失効させる。
2. **言明(statement)** — pass が何を立証するかの人間可読な文(pytest なら
   docstring)。承認時に提示される。
3. **コミットに束縛された結果が「リポジトリの内容」として届くこと** — eval は外部 API を
   呼ばない。provider は export ファイル(結果+コミット SHA)をリポジトリに書く。
   veriq export JSON と同型。
4. **環境** — lockfile ハッシュ+ CI ワークフロー定義ハッシュを環境ノードとして
   (provider が対応するなら宣言された仮定も)。

## スコープ

- `source:` の provider 記法: `veriq:<module>.<name>` / `ci:<test-id>`
  (テスト ID は pytest ネイティブ形式。例
  `tests/test_hash.py::test_order_invariance`)
- **テスト側の opt-in マーカー**(例 `@inka_evidence(statement="...")`):
  注釈されたテストだけが bind 可能。マーカーが言明を運び、証拠の境界を明示する
  (veriq の `@calc` と同じ規律)。
- `ci-export.json` を生成する pytest プラグイン:

  ```json
  {
    "commit": "abc123...",
    "environment": { "lockfile_hash": "...", "workflow_hash": "..." },
    "tests": [
      {
        "id": "tests/test_hash.py::test_order_invariance",
        "interface_hash": "sha256:...",
        "statement": "hash is invariant under child permutation",
        "result": "pass"
      }
    ]
  }
  ```

- スイートを実行し export をコミット(または保存)する GitHub Actions ジョブ。
- eval 側の照合: bind 承認のハッシュ == 現在の界面ハッシュ、export のコミット ==
  評価対象コミット、result == pass。不一致・陳腐化した export は単に established に
  ならない(明確な blame メッセージ付き)。
- concept §2.1 の更新: 「source は veriq に限定」→「source は登録された evidence
  provider への参照」+上記 provider 契約。
- 完全性の強化パス(後日、M7 の機構): GitHub OIDC / Sigstore(`actions/attest`)による
  CI 署名 attestation。`ci:github-actions` は非人間 attestor(`llm:<model>` の前例)と
  同格になる。

## スコープ外

- **有効内容(closure)の変更検出** — 界面が不変のままヘルパが変わるケース。
  意図的に別 Issue(ドラフト08)に分離: veriq と ci に等しく適用され、かつ bind の
  束縛先には**入れてはならない**(失効爆発のリスク)。

## 受け入れ条件

- [ ] provider 契約が rustdoc に文書化され、`source:` の文法が一般化されている。
- [ ] pytest マーカー+ export 生成が inka 自身の CI で動いている。
- [ ] dogfood の命題1つ(例 `local_view_hash_sound`)が本物の proptest に bind され、
      end-to-end で established になる。
- [ ] bind されたテストの本体を編集すると bind 承認が失効する(テスト)。
- [ ] concept.md §2.1 / §6 が provider 一般化に更新されている。

## 参照

- inka-concept.md §2.1(source の限定)、§6(真実性/妥当性の分離)、
  §9(attestation。非人間 attestor の `llm:` 前例)
- inka-implementation-roadmap.md M8