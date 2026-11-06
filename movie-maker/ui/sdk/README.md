# @mm/sdk

UI から mm-ops の操作を送る SDK。

## 型の生成

`crates/mm-ops/schema/ops.json` から `src/generated/ops.ts` を作る（`node tools/gen-sdk.ts`、`pnpm build` の先頭で走る）。
生成物は commit する。`ops.json` を作り直したら `pnpm build` を回して差分も commit する。テストが両者のずれを検出する。

- `Op`: 全部の操作の和（`op` が名前）。`OpName`、`OpOf<N>`、`OpArgs<N>`、`OP_NAMES`
- 操作ごとの型 `SetParamOp` など、`$defs` の型（`Layer`、`Param`、`ParamValue` ...）
- `PARAM_VALUE_SCHEMAS`: `ParamValue.type` ごとの `value` の JSON Schema（Inspector が入力欄を決める）

## 送り方

```ts
const client = createClient(createIpcTransport());
await client.send("setParam", { target: { layer: "l1", path: "opacity" }, value: { type: "number", value: 0.5 } });
```

`Transport`（`send(op) => Promise<OpResult>`）が差し替えられるトランスポート。今は IPC だけ。HTTP は同じ形の `Transport` を足す。

## アプリ側の IPC のハンドラー

プレビューのウィンドウの担当が作る。

| 項目 | 値 |
|---|---|
| コマンドの名前 | `apply_op` |
| 引数 | `{ op: Op }`。`Op` は `ops.json` の 1 つの操作を serde で出した JSON そのまま（例 `{ "op": "setParam", "target": { "layer": "l1", "path": "opacity" }, "value": { "type": "number", "value": 0.5 } }`） |
| 成功 | 操作を適用した後の値。形は問わない（sdk は `unknown` で返す） |
| 失敗 | `Err(String)`。sdk は `{ ok: false, error }` にする |

Rust の側は `#[tauri::command] fn apply_op(op: mm_ops::Op, ...) -> Result<_, String>` の形。
