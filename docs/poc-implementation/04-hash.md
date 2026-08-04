# 04: 正規化と view_hash(凍結対象)

対象: 展開済み description(と関連情報)→ 32バイトのハッシュ。**パーサ非依存の純関数**
として書けるので、最初に着手できる。ここで決めたバイトレイアウトはゴールデンベクタで
凍結し、以後変更しない(変えるときは v2 として並存)。

## 正規化(展開済み description → 正規形文字列)

順に適用。冪等になるように書く:

1. NFC 正規化(`unicode-normalization` の `.nfc()`)
2. 改行コード統一: CRLF / CR → LF
3. 各行の行末空白(スペース・タブ)を除去
4. 先頭・末尾の空行を除去
5. 共通インデントの剥ぎ取り: 非空行の先頭スペース数の最小値を全行から削る
   (**タブはインデントとして数えない** — インデントにタブが混ざっていたら
   そのまま残る。rustdoc に明記)
6. 行を `\n` で連結(末尾に改行なし)

```rust
pub fn normalize(rendered: &str) -> String
```

## 注釈の正規形

`until` / `promote` / `reason` を `key:value` の文字列にし、**キーでソートした列**に
する。`until` / `promote` の値はフェーズ名・モード名のみ。`reason` の値は引用符を
除いた本文そのもの(空白を含みうるが、長さ接頭辞フレーミングなので曖昧にならない)。
例: `["promote:bind", "reason:awaiting the mass-properties test", "until:M3"]`。

## フレーミング(バイトレイアウト v1)

すべてのフィールドは**8バイト・リトルエンディアンの長さ接頭辞付き**。リストは
要素数(u64 LE)を先頭に置く。

```
field(s)   = u64_le(len(s)) ++ bytes(s)          // s は UTF-8 文字列 or 生バイト
list(xs)   = u64_le(count) ++ field(x1) ++ ...   // 順序は呼び出し側が確定させる

objective_hash = SHA256( field("inka:objective:v1")
                      ++ field(normalize(desc))
                      ++ list(annotations_sorted)
                      ++ list(children_descs_sorted) )

structure_hash = SHA256( field("inka:structure:v1")
                      ++ field(normalize(desc))
                      ++ list(annotations_sorted)          // 正規形 key:value、キー順
                      ++ list(children_descs_sorted) )     // 正規化済み desc、バイト列順ソート

fiat_hash      = SHA256( field("inka:fiat:v1")
                      ++ field(normalize(desc))
                      ++ list(annotations_sorted) )

bind_hash      = SHA256( field("inka:bind:v1")
                      ++ field(normalize(desc))
                      ++ list(annotations_sorted)
                      ++ field(interface_hash_raw)         // ci-export の32生バイト
                      ++ list(assumptions_sorted) )        // ミニマムでは常に空リスト
```

- 子の順序は**正規化済み description のバイト列**でソート(順序非依存の実装点)
- ID・タグ・コメント・`source:` の参照文字列はどこにも入れない
  (interface_hash が evidence の同一性を運ぶ。参照名の付け替えは失効させない)
- bind に**空の仮定リストの席を最初から置く**: veriq 導入時に総仮定集合が入っても
  レイアウトが v2 にならずに済む

## API

```rust
pub enum ViewKind { Objective, Structure, Fiat, Bind }
pub struct LocalView<'a> {
    pub kind: ViewKind,
    pub description: &'a str,             // rendered(正規化前)
    pub annotations: &'a [(String, String)],
    pub children: &'a [&'a str],          // rendered descs
    pub interface_hash: Option<[u8; 32]>, // bind のみ
}
pub fn view_hash(v: &LocalView) -> [u8; 32]
```

## ゴールデンベクタ

`tests/golden/hash/*.json` に置く。1件 = 入力(kind, desc, annotations, children,
interface_hash)+ `expected: "sha256:..."`(hex)。最低カバレッジ:

- 4種それぞれ / 注釈あり・なし(reason 含む)/ 子の順序違いで同一ハッシュになるペア /
  1文字違いで異なるペア / 日本語・結合文字(NFC の効きを確認)/ 空 children
- 2〜3件は Rust 実装と独立に(Python ワンライナー等で)再導出して照合。
  スクリプトは PR 説明に貼り、リポジトリには入れない

## proptest(性質)

- `normalize` は冪等
- インデント一律付加・行末空白付加・CRLF 化 → ハッシュ不変
- 内部改行の増減・desc の1文字変更 → ハッシュ変化
- structure: children のシャッフル → 不変