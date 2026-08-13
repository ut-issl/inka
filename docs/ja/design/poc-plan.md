# inka — PoC 実装方針

一人で inka のシステムを端から端まで体験できる最小の実装。
英語版(正): [English](https://www.space.t.u-tokyo.ac.jp/inka/design/poc-plan/)

## ゴール

次のループが dogfood プロジェクト(`tests/fixtures/dogfood`)に対して回ること:

1. 論証を書く → `inka check` が通る
2. 自分で `inka review` で承認する → `inka eval` が clean になる
3. 契約(description または値)を編集する → 影響を受けた承認だけが stale になり
   blame に出る
4. 再承認する → clean に戻る
5. **本物の CI bind が1本 established になる**: `local_view_hash_sound` を実在する
   proptest ファイルに bind する。テストを編集すると bind 承認が失効し、実装だけの
   変更では承認は無傷のまま CI が真実性を再判定する

5 が bind(機械の真実性/人間の妥当性)のコンセプト証明にあたる。

## スコープ

- **読み込み**: 確定構文の lexer + 再帰下降パーサ、`inka.toml`、`argument/` 規約
  (フラット構成のみ — 名前空間解決は外)、description を消費する何よりも前段の
  `{$var}` 展開。
- **`inka check`**: グラフ構築(interning、隣接、トポソート、到達性)と構造診断。
  error があれば非ゼロ終了。
- **ハッシュ**: 正規化+ view_hash 4種(objective / structure / bind / fiat)。
  bind ハッシュは証拠の界面ハッシュ+空の仮定スロット。本物の承認を記録し始める前に最小のゴールデンベクタでレイアウトを凍結。
- **承認レコード**: `sig` を空にした v1 スキーマ、jsonl の読み書き、署名対象の
  正規化バイト列関数(署名はしない)。
- **`inka review`(CLI、TUI なし)**: 不足/stale 対象のキュー、`--as <approver>` の
  下での対象ごとの approve / decline(コメント必須)。対話処理に加えて単一対象への
  `--approve` / `--decline` の非対話形を持つ。追記専用のレコード書き込み。
  キューと eval は1つの有効性判定関数を共有する。
- **ポリシー**: 組み込みデフォルトのみ — 全対象 `any(1)`。ポリシー言語なし。
  decline の拒否権は有効。
- **`inka eval`**: established 不動点(各 objective も自身の承認を要する)、stale 検出、
  decline 拒否権、宣言されたフェーズ順序に対するノード側 `until:` のハード失効
  (期限後に付けた承認もカウントしない)、原因別の blame 出力、clean でなければ
  非ゼロ終了。
- **CI evidence provider(テストターゲット粒度)**: `ci: "<target>"`、CI で
  生成する `ci-export.json`(生成は `inka ci-export` サブコマンドが望ましい)、
  eval 側の照合。

## 明示的にスコープ外

署名とアンカリング / veriq 連携 / ポリシー言語 / TUI / LSP / what-if 評価フラグ /
可視化 / import・バンドル / LLM lint / 名前空間解決 / 承認者側 `until:` の効力 /
承認時の diff 表示 / closure ハッシュ。

## PoC 自身のテスト

- フィクスチャ: dogfood(ループの対象)と aocs(パーサ/check の負荷試験。export が
  無い限り bind は未確立のままで、それが正しい)。
- 壊しフィクスチャ群で各診断を1回ずつ発火、insta スナップショットで固定。
- ハッシュ: ゴールデンベクタ+プロパティテスト(冪等性/インデント・行末空白・CRLF
  不変/改行と1文字の変更に敏感/structure の子順序不変)。
- シナリオテスト: approve → 編集 → stale/値変更 → 参照契約のみ stale/テスト
  ファイル変更 → bind 失効/decline → unsatisfied。

## モジュール構成(単一クレート)

```text
src/
  syntax/    // lexer, parser, AST, span
  model/     // グラフ、ID interning
  hash/      // 正規化と view_hash(凍結対象)
  record/    // 承認レコード(凍結対象)
  eval/      // established 不動点・blame・組み込みポリシー
  provider/  // ci-export.json の読み込みと照合
  cli/       // check / hash / approve / eval / ci-export
tests/
  golden/    // ハッシュ・レコードのベクタ(追記専用)
  fixtures/  // dogfood, aocs, 壊しバリアント
```
