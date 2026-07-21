# inka 実装ロードマップ(Rust・自力実装)

コンセプト(inka-concept.md)を前提に、Coding Agent に頼らず一人で実装するための技術選択とマイルストーンを定める。方針は一貫して「薄い縦切りを早く回し、凍結すべき仕様だけをテストで固める」。

---

## 0. 全体方針

- **一人で持ちきれる複雑さに抑える。** 依存クレートは枯れたものだけ、フレームワーク的な依存(salsa 等のインクリメンタル計算基盤、ORM 的なもの)は入れない。ファイル数百・ノード数千の規模なら毎回全再計算で十分速く、インクリメンタル化は LSP で本当に困ってからで遅くない。
- **凍結仕様(ハッシュ正規化・レコードスキーマ)はコードでなくテストベクタで固める。** 「この入力ならこのハッシュ」というゴールデンファイルを M2 で作り、以後リポジトリの不変条件として守る。実装を書き直してもベクタが通れば互換。
- **最初からドッグフーディングする。** inka 自身の設計論証(concept.md の内容)を inka の DSL で書き、それをテストフィクスチャ兼サンプルにする。自分が最初のユーザーになることで、DSL の書き味の問題が最速で露見する。
- **LSP・TUI・署名は「後から差すもの」として、コアを純粋関数の束で書く。** `parse → graph → hash → policy → eval` がすべて「入力バイト列 → 値」の関数であれば、CLI からも LSP からもテストからも同じコードが呼べる。I/O とロジックの分離を最初から徹底する。

---

## 1. 技術選択

### ワークスペース構成

最初は**単一クレート+モジュール分割**で始め、LSP を入れる段階(M6)で workspace に割る。早すぎる分割は一人開発では境界の引き直しコストの方が高い。

```
inka/
  src/
    syntax/     # lexer, parser, CST/AST, span
    model/      # 命題・履行モード・グラフ・仮定レジストリ
    hash/       # 正規化と局所ビューハッシュ(凍結対象)
    log/        # 承認レコードの読み書き・検証(凍結対象)
    policy/     # ポリシーのパースと要求評価
    eval/       # established 不動点・blame・what-if
    cli/        # clap のサブコマンド群
    tui/        # approve セッション(ratatui)
  tests/
    golden/     # ハッシュ・スキーマのテストベクタ
    fixtures/   # ドッグフード論証
```

M6 以降の分割目安:`inka-core`(syntax〜eval)/ `inka-cli` / `inka-lsp`。core が tokio に依存しないよう保つのが分割の目的。

### クレート選定

| 領域 | 選定 | 理由・注意 |
|---|---|---|
| 字句解析 | `logos`(または手書き) | 高速で導入が軽い。手書きでも量は知れている |
| 構文解析 | **手書き再帰下降** | パーサジェネレータ(pest 等)やコンビネータ(chumsky)より、エラー回復・span 管理・将来の LSP 対応の自由度が高い。DSL は小さいので手書きコストは低く、学習効果も最大 |
| 診断表示 | `ariadne` or `miette` + `thiserror` | span 付きのきれいなエラー表示。LSP 診断への変換も素直 |
| グラフ | **自前の隣接マップ**(`HashMap<NodeId, Vec<NodeId>>`) | petgraph でもよいが、必要なのはトポソート・到達性・不動点だけで自前実装が数百行。ID interning(`String` → `u32`)だけやっておく |
| ハッシュ | `sha2` + `unicode-normalization` | NFC 正規化を正規化規則に含める(§2 参照) |
| シリアライズ | `serde` + `serde_json` | jsonl は1行ずつ `serde_json::from_str` |
| CLI | `clap`(derive) | 標準 |
| TUI | `ratatui` + `crossterm` | 事実上の標準。approve セッションは1画面+キー操作なので学習量は小さい |
| LSP | `tower-lsp`(async/tokio)または `lsp-server`(同期、rust-analyzer 系) | 一人開発なら **`lsp-server` + `lsp-types`** の同期モデルの方がデバッグしやすい。tokio を core に持ち込まない利点もある |
| 署名 | `ed25519-dalek` + `ssh-key`(sshsig) | まず SSH 鍵署名(開発者が既に持っている)。Sigstore(`sigstore` クレート)は成熟を見て後から差す。attestation の署名対象はレコードの正規化 JSON バイト列 |
| HTTP(LLM lint) | `ureq`(同期) | reqwest/tokio を持ち込まない。lint は CLI から叩くバッチ |
| テスト | `insta`(スナップショット)+ `proptest` | blame 出力・診断・パーサは insta、正規化の性質(冪等性等)は proptest |

### あえて選ばないもの

- **salsa / インクリメンタル計算**:全再計算で足りる規模。LSP の応答が実測で遅くなったら初めて検討。
- **非同期ランタイム(core 内)**:eval もパースも同期で書く。
- **独自バイナリフォーマット**:すべてテキスト(DSL、jsonl、policy)。git との親和性が価値の源泉。
- **可視化の自前レイアウト**:Sugiyama 法を自作しない。JSON を埋め込んだ静的 HTML + CDN の既存 JS ライブラリ(後述 M10)。

---

## 2. 最初に紙で書き切るもの(M0 の成果物)

コードより先に、凍結対象2点を仕様文書として書き切る。ここが曖昧なまま実装に入ると、後の全承認を飛ばす手戻りになる。

**ハッシュ正規化仕様(spec/hash.md)** — 決めるべき項目:

1. 入力単位:局所ビュー = 自ノードの正規化 description + 注釈 + 子の正規化 description の**多重集合**。
2. 文字列正規化:UTF-8、NFC、前後空白除去、連続空白の1化、改行の空白化。コメントは字句段階で落ちるので対象外。
3. 子の順序:**ソートして順序非依存にする**(supported-by の並び替えを失効ゼロにする)。ソートキーは正規化 description のバイト列。
4. 結合方式:長さ接頭辞付き連結(`len || bytes` の列)で区切りの曖昧さを排除し、`sha256` にかける。ドメイン分離タグ(`"inka:structure:v1"` 等)を先頭に入れ、承認対象種別ごとに衝突空間を分ける。
5. ID・タグ(`#safety`)は含めない。期限注釈(`until:/promote:`)は含める。
6. バージョン番号 `v1` を仕様とテストベクタの両方に刻む。

**承認レコードスキーマ(spec/record.md)** — フィールド定義(target, view_hash, label, comment, approver, key_id, timestamp, phase, until, policy_hash, sig, v)、署名対象バイト列の定義(sig を除く正規化 JSON:キーをソートした compact 表現)、jsonl の追記・分割規約。

どちらも 2〜3 ページで書ける。**このマイルストーンの完了条件は、手計算(または使い捨てスクリプト)でテストベクタを 10 個作れること。**

---

## 3. マイルストーン

各マイルストーンは「動くコマンドが1つ増える」単位で切る。完了条件を必ず動作で定義する。

### M0:仕様凍結(コードなし)
上記2文書+テストベクタ。ドッグフード用に concept.md の論証を DSL で手書きしてみる(パーサがなくても書ける。書き味の問題はここで一度洗える)。

### M1:パーサと構造診断 — `inka check`
- lexer / 再帰下降パーサ / span 付き AST。フラット構文+葉インライン+`supported-by` 参照+タグ+期限注釈。
- 複数ファイルの読み込み → ID 解決 → グラフ構築。
- 診断:未定義参照、重複 ID、DAG 違反(閉路の経路表示)、Objective 非到達、未履行 warning、fiat が子を持つ error。
- **完了条件**:ドッグフードフィクスチャが check を通り、壊したフィクスチャ群の診断が insta スナップショットで固定されている。

### M2:局所ビューハッシュ — `inka hash <id>`
- 正規化仕様の実装。structure / bind / fiat の3種の view_hash を計算。
- **完了条件**:M0 のテストベクタ全通過。proptest で「空白・改行・子の順序の摂動でハッシュ不変」「description の1文字変更でハッシュ変化」を検証。**以後この仕様は変更禁止**(変えるなら v2 として並存させる)。

### M3:承認ログとポリシー、eval の不動点 — `inka eval`
- jsonl の読み込み(署名フィールドは無視する段階)。phase.inka の読み込み。
- ポリシーパーサ(セレクタ+要求述語:any / role / including / not-author / fresh / forbidden。llm-lint と eval-clean は構文だけ受けて未実装 warning)。
- established 不動点(DAG なのでトポ順1パスで決まる)、decline の拒否権、until(ノード側・承認側)の失効判定。
- blame 出力:未確立の最深原因ノード列挙。
- **完了条件**:手書きの jsonl フィクスチャに対する blame 出力が insta で固定。「approve 済み→description 変更→stale 検出」のシナリオテスト。
- この時点で TUI なしでも `echo '{...}' >> approvals/me.jsonl` で運用の全体が一周する。**ここが最初のドッグフーディング開始点。**

### M4:承認セッション — `inka approve`(TUI)
- キュー導出(ポリシーが自分に要求する未承認/stale 対象)。approver 名は当面 `--as <id>` か環境変数。
- 1画面:対象の契約/子の契約のみ/前回自分が承認した局所ビューとの diff/過去ログ。a / d(コメント必須)/ c(委譲先)/ s、approve への `until:` 指定。
- diff は正規化前のソーステキスト同士を `similar` クレート等で。
- **完了条件**:自分のドッグフード論証を TUI だけで全承認でき、eval が clean になる。

### M5:what-if 群 — eval の拡張
- `--forecast <phase>` / `--margin -1` / `--without-approver` / `--as-of` / `--diff-approvals`(ワークツリーの編集で剥がれる承認のプレビュー)。
- すべて M3 の eval を入力を差し替えて呼ぶだけの薄い層になっているはず。なっていなければ M3 の設計を直す(ここが「eval は純関数」の検算になる)。
- **完了条件**:各フラグの出力スナップショット。

### M6:LSP — `inka lsp`
- ここで workspace 分割(core / cli / lsp)。
- 機能を絞る:publishDiagnostics(M1 の診断の変換)、hover(参照先 description)、inlay hints(参照 ID 横の半透明 description)、code lens(承認状態・established)、goto definition / references(共有ノード)、rename(参照一括書き換え=失効ゼロの実演)。
- 実装は「didChange のたびに全ファイル再パース・再 eval」で開始。承認状態は jsonl と veriq キャッシュを読むだけ。
- **完了条件**:VS Code(拡張は package.json + 数十行のクライアント)で自分が日常的に書ける。

### M7:署名とフェーズ遷移
- SSH 鍵署名(sshsig)での attestation 生成・検証。`eval --verify` で全レコード検証。
- 軽量アンカリング(レコード数+ハッシュチェーンの記録)。
- `rule phase-transition` と昇格 attestation、fresh の phase フィールド判定の本実装。
- **完了条件**:改竄した jsonl(1文字書き換え・行削除)を検出するテスト。

### M8:veriq 連携
- **veriq 側(Python)に export スクリプトを書く**:グラフ構造・言明・assumes 参照・契約ハッシュ(関数 AST の正規化ハッシュ。Python の `ast` モジュールで docstring/位置情報を落として dump → sha256)を JSON に吐く。Rust 側はこの JSON を読むだけにして、Rust から Python を解析しない。
- 仮定レジストリのパース(inka DSL の一部)、総仮定集合の集約、bind 承認の束縛先への組み込み、fiat 命題の自動実体化。
- approve TUI に veriq ノード(契約+依存先の言明のみ+diff)と総仮定集合 diff の画面を追加。
- **完了条件**:veriq のサンプルプロジェクトに仮定を1つ追加すると、境界の bind 承認だけが stale になることのテスト。

### M9:LLM lint
- プロンプトテンプレート(バージョン管理)+ `inka lint`(差分のあった局所ビューだけ API に投げ、`llm:<model>` の attestation を書く)。
- override 機構(人間1名の理由付き解除)。
- **完了条件**:意図的に「部分木前提の description」を書いたフィクスチャを lint が decline する(プロンプトの回帰テストは snapshot ではなく手動評価セットで持つ)。

### M10:import と可視化
- バンドル(ノード+attestation+元ポリシー)の export / import、trust 条項の評価、可搬性規則(structure のみ照合、fiat/bind は再承認要求)。
- 可視化:`inka viz` が JSON 埋め込みの単一 HTML を吐く。レイアウトは既存 JS(Cytoscape.js の dagre 等)に任せ、Rust 側はデータ整形のみ。トポロジー+色+バッジのみ、description はクリックで表示。
- **完了条件**:ミッション A のバス系モジュールをミッション B に import し、structure 承認だけが引き継がれる E2E テスト。

---

## 4. 順序の理由と依存関係

- M3 を TUI(M4)より先にするのは、**approve キューの定義がポリシー評価に依存する**ため。TUI を先に作ると「何を表示すべきか」が決まらないまま UI を書くことになる。
- 署名(M7)を M4 より後にするのは、ローカルの一人ドッグフーディングでは完全性の敵がいないため。ただしレコードスキーマ(sig フィールドの席)は M0 で確保済みなので、後付けで形式が壊れない。
- veriq(M8)が中盤なのは、それ以前の全機能が veriq なしの純粋な論証グラフで検証できるから。連携はアダプタ1枚(JSON)に隔離されており、遅らせてもコアの設計に影響しない。
- M5 が M6 より先なのは、what-if がきれいに書けるかどうかが core の設計品質の試金石であり、LSP(最大の追加複雑性)に入る前に直したいから。

## 5. 一人開発での規律

- **各マイルストーン末にドッグフード論証を更新し、自分で approve セッションを回す。** ツールの UX 課題は自分の指が最初に見つける。
- **insta スナップショットを恐れず大量に持つ。** 一人開発ではレビュアーがいないので、スナップショット diff が唯一の「意図しない変更」検出器になる。
- **ゴールデンベクタのディレクトリは CI で read-only 扱いにする**(変更する PR は仕様バージョンを上げることを CONTRIBUTING に明記)。
- パーサとハッシュには早めに `cargo fuzz` を数時間回しておくと、正規化の端(不正 UTF-8、巨大入力、深い参照鎖)の panic を潰せる。
- 見積もりの目安:M1〜M4 がそれぞれ集中して1〜2週間相当、M6 と M8 がやや重く2〜3週間相当。**M4 完了(TUI で一周)を最初の対外デモ点**に置くとモチベーション設計としても良い。
