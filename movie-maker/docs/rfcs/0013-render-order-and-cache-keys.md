# RFC 0013: 段階 2 のレビュー（レンダラー）: テクスチャの取り置きと提出の順序、キャッシュのキーの外の入力、LUT の素材

状態: 採用

## 変える理由

- 中間のテクスチャを、まだ提出していない合成が読んでいる間に取り置きへ戻し、次の `create` がすぐ使い回していた。
  使い回した側の `fill`・種類の自前のパス・`upload` はその場で提出するので、溜まった合成より先に GPU で走る。
  親と同じ大きさの入れ子の中の「単色 + 効果」と親の単色、シーケンスのカットの受け渡しで絵が化け、キャッシュに残った。
- フレームのキャッシュのキーに、パラメータの外の入力が入っていなかった。入れ子のレイヤーの効果（種類が決定的か、
  効果が参照する素材）、`.cube` の中身（`color.lut` はパスの文字列で読んでいた）、OCIO の設定、`auto` が素材ごとに
  選んだデコーダー。
- `Memo`（LRU・依存の索引・追い出し）を API クレートの `mm-render-api` が持っていた（`14-dev-structure.md` の
  「API クレートは impl を持たない」に反する）。

## 先行事例と新規の設計

| 先行事例 | 採ったもの |
|---|---|
| wgpu と Vulkan の「同じキューの提出は順に走る」という保証、wgpu-core が資源を最後の提出の後まで生かす仕組み | 手放したテクスチャは、読むコマンドを提出するまで使い回さない（下の決まり） |
| Nuke と After Effects のキャッシュのキー（入力のファイルの中身のハッシュ、OCIO の設定を含む） | LUT を素材にして `contentHash` をキーに入れる。色の変換の設定の指紋をキーに入れる |
| rustc のクエリ（依存した入力を全部たどる） | 入れ子の指紋は効果のノードと素材もたどる |

新規に設計したもの: シーケンスのトランジションの半分の長さの切り方（両端にトランジションがあるカットは長さの半分ずつ、
素材の余白も超えない）。

## テクスチャの取り置きと提出の順序（`mm-gpu` の `WgpuGpu`）

決まりは `crates/mm-gpu/src/helper.rs` の冒頭の 1 か所に書く。要点:

- `release_id` で手放した中間のテクスチャは「提出待ち」に置き、`create` は使い回さない。
- 呼び手は積んだコマンドを提出した直後に `recycle` を呼ぶ（フレームの終わりの `after_submit` が中で呼ぶ。
  レンダラーは効果の前の `flush` の後にも呼ぶ）。ここで初めて取り置きに入る。
- 提出待ちは 64 枚までで、超えた分は取り置かずに捨てる（提出しない呼び手でも溜まり続けない）。

## 足したもの（既定の実装つき。API クレートの実装は変えなくてよい）

| クレート | 足したもの |
|---|---|
| `mm-node-api` | `InputFrames::asset_path(&AssetId) -> Option<PathBuf>`。既定は `None`。レンダラーはプロジェクトのデータから答える |
| `mm-color-api` | `ColorTransform::config_fingerprint() -> String`。既定は `id`。`mm-color-ocio` は OCIO のバージョン・設定の場所（ファイルなら更新時刻）・LUT の一辺 |
| `mm-decode-api` | `Decoder::backend_id() -> Option<&'static str>`。既定は `None`。`ffmpeg`・`d3d12va`・`cuvid` が名乗り、`mm-runtime` の `SplitDecoder` は映像の側を返す |

## 消したもの

| クレート | 消したもの | 移した先 |
|---|---|---|
| `mm-render-api`（0.2.0 に上げる） | `Memo`、`Weighted`、`Evicted`、`Dropped`、`MemoStats`、`FrameCacheKey::store_key`、依存の `mm-cache-api` | `mm-render` の `frame_cache::memo`（`MemoStats` は `mm_render::MemoStats`）、`store_key` は関数 |

API には `FrameCacheKey` と `digest`、`digest_bytes`、`size_bucket` だけが残る。

## プロジェクトのデータ（スキーマ v5）

- `color.lut` のパラメータ `path`（文字列）を `lut`（`AssetRef`）にする。
- v4 → v5 の移行（`mm-doc` の `migrate_lut`）は、`path` をその場所の素材に移す。中身を読まないので素材の `contentHash` は空。
  同じパスは 1 つの素材にまとめ、素材の ID は `lut-` とパスの FNV-1a。
- フレームのキャッシュは `contentHash` が空の素材に頼る画像を覚えない。取り込み直すとハッシュが入る（取り込みが `.cube` を
  受けるのは `mm-ingest` の担当）。

## `mm-render`

- キャッシュのキー（`frame_cache/key.rs`）: 入れ子の指紋は、レイヤーの有効な効果・シーケンスのカットとトランジションのノードの種類が
  決定的かを見て、参照する素材の `contentHash` と、素材を読むデコーダーの実装を入れる。効果の ID と素材の ID を依存に入れる。
  `color_config` は `ColorTransform::config_fingerprint` で、レイヤーの画像には素材を読むデコーダーの実装を足す。
- ディスクへの退避（`frame_cache/disk.rs`）: 写す命令までを描画のスレッドで出し、待ちと書き込みは別のスレッドで行う。
  ファイルの先頭にキーの JSON を入れ、読み戻すときに照合する（先頭は `MMF2`）。`WgpuRenderer::wait_frame_cache_spills` で待てる。
- シーケンス（`crossfade.rs`）: トランジションの半分は、トランジションの長さの半分・両側のカットの長さ（両端にトランジションが
  あるカットはその半分）・延ばす側の素材の余白（出ていくカットは出の後ろ、入ってくるカットはイン点の前、速さで割る）の最小。
- `SourcePool::footage_with_backend` は読むデコーダーの実装の名前も返す。

## 確かめて、直さなかったもの

- 同じフレームで同じ素材の N と N+1 を同じデコーダーに読ませる選び方（`source.rs` の `pick`）。デコーダーはプールの
  テクスチャを借用として貸し、借用は `after_submit` まで返らない（RFC 0001）ので、N+1 は別のテクスチャに書く。
  cuvid の NVDEC のサーフェスもコピーの完了まで保持する。`tests/footage_reuse.rs` がプールの形のデコーダーで確かめる。

## 迷った所

- LUT のパスを種類に渡す口。`FrameCx` にフィールドを足すと全部の呼び手が壊れるので、既定の実装を持てる
  `InputFrames` のメソッドにした。入力の映像と素材は別の話だが、どちらも「レンダラーが種類に渡すもの」。
- 入れ子の指紋に入れるデコーダーの実装は、まだ開いていない素材ではファクトリーの名前（`auto`）になる。最初のフレームだけ
  別のキーで覚え、次から実際の実装のキーになる。
