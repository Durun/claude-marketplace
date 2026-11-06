# RFC 0004: プロジェクトのデータ v2 の Layer.transform と入れ子の扱い

状態: 採用

## 変える理由

プロジェクトのデータの `Layer` に変形のフィールドが無く、レンダラーがノードのパラメータ `transform.*` を先に取り除いて読んでいた。
設計書（`03-data-model.md`）どおり `Layer.transform` に置き、ノードのパラメータと混ざらないようにする。

## 新しい形

```rust
pub struct Layer { /* 既存 */ pub transform: Transform }
pub struct Transform { pub position: Param, pub scale: Param, pub rotation: Param, pub anchor: Option<Param> }
pub const SCHEMA_VERSION: u32 = 2;
pub fn load(json: &str) -> Result<Project>;      // 古いバージョンを上げて読む
pub fn migrate(value: Value) -> Result<Value>;   // v1 → v2: transform.* を Layer.transform に移す
```

- 値はコンポジションの画素と度。`scale` は Number か Vec2。`anchor` が無ければ画像の中心。
- v1 のプロジェクトのデータは `fixtures/doc/v1/` に凍結し、`mm-doc` のテストが読めることを確かめる。

## 入れ子（決定）

ノードの種類 `composition` は API クレートの `NodeKind` として登録するが、展開はレンダラーが行う。ノードの種類のインターフェース（`FrameCx`）からはプロジェクトのデータが見えず、入れ子の中のベクターを出力の解像度で 1 回だけラスタにする（`04-nodes.md`「ベクターの解像度」）にはレンダラーが親の変形を知っている必要があるため。ノードの種類自身は透明を返し、レンダラーの外で呼ばれても安全。`04-nodes.md` に書く。

## 影響する実装

- `mm-render`: `LayerTransform::from_doc(&layer.transform, t)`。
- `apps/mm`: プロジェクトのデータは `mm_doc::load` で読む。
- `fixtures/projects/stage0`: `schema_version` を 2 に。

## 移行の手順

1. `mm-doc` に `Transform` と移行を足し、バージョンを 2 にする。
2. レンダラーの読み先を替え、テストのレイヤーを `Layer.transform` で書く。
3. プロジェクトのデータを読む経路は全部 `mm_doc::load` を通す。
