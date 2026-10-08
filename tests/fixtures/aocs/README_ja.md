# aocs: 姿勢制御系のサンプル論証

衛星の姿勢制御系(AOCS)を題材にした2つ目のサンプル論証。dogfood(inka 自身のメタな
論証)では荷重がかからない DSL 機能——**bind(`test:`/`veriq:` 付き)、タグ、数値の値埋め込み、
bind ノードの DAG 共有**——を意図的に使う。

英語版(正): [README.md](README.md)

## ファイル構成

| ファイル | 内容 |
| --- | --- |
| `inka.toml` | プロジェクト定義。フェーズは衛星開発の BBM / EM / FM(current = EM) |
| `values.toml` | 要求値・条件値8変数(指向精度、分離レート、余裕率など) |
| `argument/objective.inka` | ルート: 姿勢制御性能と衛星安全の維持 |
| `argument/pointing.inka` | 枝1: 指向性能(#mission)。bind 3枚 |
| `argument/safety.inka` | 枝2・3: デタンブリングとセーフモード(#safety)。markdown 箇条書きの契約を含む |
| `argument/momentum.inka` | 枝4: 角運動量管理。`actuator_authority_ok` を pointing と共有 |
| `argument/assumptions.inka` | 外乱モデル・質量特性の仮定(fiat)。複数枝から共有 |

使っている機能: bind ×5(veriq 参照)、`#safety` / `#mission` タグ、`{$var}` ×8
(数値+単位)、**bind ノードの DAG 共有**(`actuator_authority_ok` は2親)、
fiat の `until: FM` / `promote: bind`(EM 段階の負債の明示)、markdown 箇条書き契約
(`safe_mode_ok`)。

## 発見事項(現在の状態)

この例を書いた後に確定し、確定仕様([docs/ja/design/spec.md](../../docs/ja/design/spec.md))の一部となったもの:

- **ID 名前空間**: 区切りは `::`、`argument/` 配下のディレクトリ構造から自動付与。
  ポリシーセレクタも同記法(`power::*`)。`veriq:` 参照は Python 側の
  `module.function` 名なのでドットのまま。
- **タグは自由語彙**。check はタグについて何も言わない(タイポ検出は M3 のポリシー
  セレクタ側)。
- **解決できない `test:`/`veriq:` 参照**は check では形式検査のみ。実在検証は eval の仕事。

未決・観察として残るもの:

1. **bind の description と証拠の言明の二重性。** description(人間側契約)が検証内容を
   再述する感覚がある。bind 承認では両方が提示される設計なので冗長ではないが、
   書き味の観察として記録。
2. **values は素の文字列で足りる**(`"0.05 deg (3σ)"`)。単位の型付けは今のところ不要。
3. **多重集合は不要のまま**: 同じ子を2回参照したい場面は出ていない
   (「集合+重複は error」を維持)。

## この例で確認できたこと

- モード宣言型ブロック(structure / bind / fiat)は、bind が実際に混ざるとむしろ
  読みやすくなる(機械検証と受諾の境界が一目で分かる)。
- `until: FM` の並びがそのまま「EM 段階の負債一覧」として読める。`--forecast FM` の
  価値が体感できる。
- 数値を values に出したことで、「要求値の変更が界面に波及する」という inka の中核
  ストーリーがこの例だけで実演可能(`pointing_accuracy` を変えると契約2枚+親が stale)。
