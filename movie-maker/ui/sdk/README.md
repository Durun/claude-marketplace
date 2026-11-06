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

## Media の命令

Media パネル（`ui/panels/media`）が使う Tauri のコマンド。ハンドラーは `apps/movie-maker/src/media_ipc.rs`、型は `src/media.ts`（`createIpcMediaApi` が呼ぶ）。失敗は `Err(String)`。

| コマンド | 引数 | 成功 |
|---|---|---|
| `media_list` | なし | `MediaItem[]`（`id`、`name`、`path`、`content_hash`、`kind`、`duration`（秒）、`fps`、`width`、`height`、`has_thumbnail`、`has_waveform`、`status`: `ready` / `partial` / `missing`） |
| `media_thumbnail` | `{ id }` | JPEG の生のバイト列（`ArrayBuffer`） |
| `media_waveform` | `{ id, buckets }` | `{ duration, peaks }`。`peaks` は `buckets` 個以下で 0 から 1（チャンネルの最大） |
| `media_pick_files` | なし | 選んだパスの配列。キャンセルは空 |
| `media_import` | `{ paths }` | 全部終わってから `()`。取り込み中にもう一度呼ぶと `Err("取り込み中")` |
| `media_cache_info` | なし | `{ total_bytes, folders: [{ name, bytes, files, clearable }] }` |
| `media_cache_clear` | `{ kinds }` | 空けたバイト数。`preview`、`audio`、`thumbs` だけ受け、ほかの名前は何も消さず `Err` |

`media_import` は `mm ingest` と同じ `mm_ingest::ingest` で素材ごとに `cache/` へ出力を作り、まだ無い素材だけ `addAsset` を再生スレッドに適用させる。進み具合は Tauri のイベント `mm:ingest-progress`（`INGEST_PROGRESS_EVENT`）で送る。中身は `type` で分かれる。

| `type` | 中身 |
|---|---|
| `started` | `index`、`total`、`path` |
| `done` | `index`、`path`、`asset_id`、`added`（偽なら同じコンテンツハッシュの素材がすでにあった） |
| `failed` | `index`、`path`、`error`（失敗した素材は飛ばして次へ進む） |
| `finished` | `added`、`skipped`、`failed` |

ファイルのドロップは Tauri の `tauri://drag-enter`、`tauri://drag-leave`、`tauri://drag-drop`（`{ paths }`）を `MediaApi.onDrop` で受ける。
