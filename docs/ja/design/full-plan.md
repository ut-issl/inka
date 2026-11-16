# inka — 全体実装方針(PoC 以降)

テスト証拠は[テスト証拠の契約](test-evidence.md)に従う。証拠jsonの生成と配送はソースレポジトリ側とcapsulaの責務であり、inkaは取り込んだ証拠を評価する。

PoC の上に積む機能領域。以下の並びは**依存関係にもとづく整理であって線表ではない**。
記載の依存以外に、日付や順序への約束は含まない。
英語版(正): [English](https://www.space.t.u-tokyo.ac.jp/inka/design/full-plan/)

## 機能領域

1. **ポリシー言語。** セレクタ(モード × id/名前空間/`#tag`)と述語(`any`, `role`,
   `including`, `not-author`, `fresh`, `forbidden`)を、`inka.toml` から参照される
   ポリシーファイルからパースする。承認に `policy_hash` を記録。存在しないタグに
   マッチするセレクタは warning(タグのタイポ検出)。組み込み `any(1)` を置き換える。
2. **承認セッションの強化。** 前回自分が承認した局所ビューとの diff(過去ソースの
   復元が必要)、承認者側 `until:` の効力、CLI で不足するなら TUI(ratatui)。
   どの UI でも部分木展開なしの規則は維持。
3. **what-if 評価。** `--forecast <phase>`、`--margin`、`--without-approver`、
   `--as-of`、`--diff-approvals` — すべて純関数 eval の入力を差し替えて呼び直す薄い層。
4. **LSP。** ワークスペース分割(core / cli / lsp。core は同期のまま、tokio なし)、
   `lsp-server` + `lsp-types`。診断、hover、inlay hints(参照位置に子の description)、
   読み取り専用 code lens(承認状態)、goto/references、rename(失効ゼロの実演)。
   semantic tokens が description の本格ハイライトを供給: サーバが description 範囲を
   本物の CommonMark パーサでパースしてトークンを出し、エディタ内では意図的に簡素な
   TextMate 文法を上書きする。
5. **署名と完全性。** レコードの正規化バイト列への SSH 署名(ed25519, sshsig)、
   `eval --verify`、削除を検出可能にする軽量アンカリング(レコード数+ハッシュ
   チェーン)、承認可能なイベントとしてのフェーズ遷移(`eval-clean` 述語+昇格
   attestation)。
6. **evidence provider の拡張。** 関数粒度のテスト証拠(ソース側アダプターによる収集、
   CRLFのみ正規化した確認可能なソースの界面ハッシュ)。証拠ごとの実行閉包ハッシュの記録 — 界面不変のままの
   有効内容の変化を eval の warning として表面化し、opt-in の `closure-fresh`
   ポリシー述語で選択的に厳格化。署名付き CI attestation(OIDC/Sigstore)は署名機構に
   接続する。
7. **calc 証拠(veriq 連携)。** 計算グラフ向けの evidence 種別 `calc:`、まず veriq
   ([calc 証拠](calc-evidence.md)): Python 側アダプタが各検証の定義粒度の上流部分グラフを
   確認可能な生ソースとして export し、仮定する論証ノード(ノードのメタデータで宣言、
   部分グラフ全体で和集合)とリーフ結果を添える。inka はソースを CRLF→LF のみでハッシュ
   する。論証そのものが仮定レジストリ。bind ハッシュの仮定スロットは仮定ノードの
   description を保持する。仮定は bind の暗黙の依存。bind 承認時に仮定集合 diff を提示。
   Rust は Python を決してパースしない — JSON アダプタが継ぎ目のすべて。
8. **LLM lint。** バージョン管理されたプロンプトテンプレート、局所ビューのみの
   レビュー(「親は子から導けるか」「ビュー外の知識を前提とする語はないか」)、
   テンプレートハッシュとモデル ID を記録する `llm:<model>` attestation、理由を
   ログに残す人間1名の override で解除可能な decline。
9. **プロジェクト横断の再利用。** バンドル(ノード+attestation+元ポリシー)の
   export/import、取り込み側の trust 条項、可搬性規則(structure 承認は運べる、
   fiat は再受諾、bind は再束縛+再承認)。署名に依存。
10. **可視化。** 静的な単一 HTML の export(トポロジー・承認状態・stale。
    description はクリック時のみ)。レイアウトは既存 JS グラフライブラリに委譲。
    読み取り専用で、承認フローからは到達不可。

## 継続する技術選択

手書き再帰下降(パーサジェネレータなし)、`ratatui` + `crossterm`、`lsp-server` +
`lsp-types`(同期)、`ed25519-dalek` + `ssh-key`、バッチ HTTP は `ureq`、diff は
`similar`、テストは `insta` + `proptest` + `cargo-fuzz`。意図的に採らないもの:
インクリメンタル計算基盤(実測で困るまで全再計算)、core 内の async、バイナリ
フォーマット(すべてテキスト)。

## 規律

- 各機能が入るたびに dogfood 論証を更新し、承認ループを回し直す — ツール自身の論証が
  最初のユーザーであり続ける。
- ゴールデンベクタのディレクトリは追記専用。非互換のレイアウト変更は v2 として並存。
- パーサと正規化器は早めに fuzz(不正 UTF-8、巨大入力、深い参照鎖)。
