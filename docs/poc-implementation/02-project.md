# 02: プロジェクト読み込み(inka.toml・argument/ 規約・値展開)

対象: プロジェクトディレクトリ → パース済み AST 群+展開済み description。

## inka.toml

```rust
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub project: Option<ProjectSection>,   // name は今は表示にしか使わない
    pub phases: PhasesSection,             // order: Vec<String>, current: String
    pub values: Option<ValuesSection>,     // files: Vec<PathBuf>
}
```

- `toml` クレートで serde デシリアライズ。**未知キーは拒否**
  (`deny_unknown_fields`)— タイポを黙って通さない
- 検証: `current` が `order` に含まれること。`order` に重複がないこと
- フェーズ順序の比較はここで `HashMap<String, usize>`(名前→序数)にしておき、
  eval(07)はそれを受け取るだけにする

## argument/ 規約読み込み

- `argument/` 配下の `*.inka` を列挙して全部パースする
- **列挙順に依存しない設計にする**: ブロックの登録順がグラフや診断の順序に影響しない
  よう、後段(03)で ID ソートしてから処理する(「ファイル分割・記述順序自由」の原則)
- ミニマムではフラット構成のみ(サブディレクトリ=名前空間の解決は範囲外)。
  サブディレクトリを見つけたら「未対応」の error にして黙って読まない

## `{$var}` 展開

- values ファイル(TOML)をフラットな `HashMap<String, String>` に読む。
  値が文字列以外(整数等)なら TOML の値をそのまま文字列化してよい
- 展開は description の `raw` に対する1パスの手書きスキャン:
  `{$` を見つけたら `}` まで読み、`[A-Za-z_][A-Za-z0-9_]*` に一致すれば置換。
  **一致しなければ(空・不正文字)そこは補間ではなく文字通り**(確定規則1:
  正確なパターンのみ特別扱い)
- 未定義変数は error。span は `Description.span.start + 内部バイトオフセット` で計算
  (マルチバイト文字があるので char 数でなくバイトで管理する)
- 出力は `rendered: String`。**以後の全段(正規化・ハッシュ・表示)は rendered だけを
  見る**。raw を後段に漏らさない型にする(例: 展開後は `RenderedDescription` 型に
  変換し、raw を private に)

## 落とし穴

- 値の中に `{$...}` が入っていても**再帰展開しない**(1パスのみ)。無限ループ防止と
  意味論の単純さのため。rustdoc に明記
- values ファイルが複数指定された場合のキー衝突は error(どちらが勝つかを決めない)

## テスト

- 正常系: dogfood(sig_scheme)・aocs(8変数)が展開できる
- 未定義変数 / キー衝突 / 不正パターン(`{$}` `{$1a}` は文字通りに残る)
- 展開結果の insta スナップショット(rendered 全文)
