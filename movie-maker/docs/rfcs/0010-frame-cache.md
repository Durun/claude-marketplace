# RFC 0010: フレームのキャッシュ（段階 2）: ディスクのキャッシュ、結果の覚え、PTS の索引のキャッシュ

状態: 採用

## 変える理由

- 同じ位置のシークや再生のやり直しで、デコードと文字の描画を毎回やり直している。
- 素材を開くたびに全パケットを読んで PTS の索引を作る（1 時間の 4K 50 Mbps で約 22 GB。段階 0 の繰り越し 13）。
- `Renderer::invalidate` が開いた素材を全部閉じる。編集のたびに全素材を開き直す。

## 先行事例と新規の設計

| 先行事例 | 採ったもの |
|---|---|
| rustc のクエリ（キーで引く計算のメモ化、依存した入力が変わったら該当する結果だけ捨てる） | `Memo`: `FrameCacheKey` で引き、結果が頼った ID（レイヤー、素材、コンポジション）を覚える。操作の `touches` と重なる結果だけ捨てる |
| Nuke / After Effects のディスクキャッシュ（内容のハッシュで引き、容量の上限で古いものから消す） | `mm-cache-fs`: `<project>/cache/<種類>/<contentHash>/<variant>`、触った順で消す |
| 2Q / ARC のような入場の条件つきキャッシュ | 上限で追い出した画像は、追い出されるまでに 2 回以上使われたものだけをディスクへ退避する。スクラブで通り過ぎただけの画像は読み戻しの待ちが無駄 |

新規に設計したもの:

- GPU のテクスチャを覚えの値にし、追い出しと解放を分ける（`graveyard`）。同じフレームの合成が参照している途中のテクスチャを手放さない。
- 入れ子のコンポジションの画像のキーは、入れ子の先まで含めた中身の JSON の FNV-1a を `input_keys` に入れる。
- PTS の索引は素材の指紋（大きさと先頭・中央・末尾の 64 KiB の BLAKE3）で引く。

## 決めたこと

### `mm-cache-fs`

`FsCache::open(root, limit)` が `CacheStore` を実装する。契約テスト（`mm_testkit::cache::contract`）を通る。

- ファイル名はキーの `%XX` 符号化。書き込みは一時ファイルから名前を付け替える。開くときに一時ファイルを捨て、既存のファイルを更新時刻の順に索引へ読む。
- 触った順は `get` で更新時刻も進めるので、開き直しても保つ。

### `mm-render-api`（足すだけ）

| 足したもの | 内容 |
|---|---|
| `digest_bytes(bytes)` | FNV-1a。`input_keys` に中身の指紋を入れるのに使う |
| `Renderer::invalidate_touched(ids)` | 既定は何もしない。操作の `touches` に頼ったキャッシュを捨て、素材 ID が入っていればその素材だけ閉じる |

`Scheduler` の `prioritize`（先読みの順）は変えていない。

### `mm-render` の覚え（`frame_cache::memo`）

API クレートは impl を持たないので、覚えの実装はレンダラーの側に置く（RFC 0013 で `mm-render-api` から移した）。

| もの | 内容 |
|---|---|
| `Memo<V>`、`Weighted`、`Evicted`、`Dropped`、`MemoStats` | キーで引く覚え。メモリの上限（`weight_bytes` の合計）で、触った順に古いものから追い出す。追い出した値は呼んだ側へ返す。`MemoStats` は `mm_render::MemoStats` |
| `Memo::invalidate(ids)` | `ids` のどれかに頼った値だけを捨てて返す |
| `store_key(key)` | ディスクのキャッシュのキー。素材があれば `contentHash` の下、無ければ `digest` の 16 進の下 |

### `mm-render`

- `kind_image`（レイヤーの画像）と `nested_image`（入れ子のコンポジションの画像）が `Memo` を引く。決定的でない種類は覚えない。
- レイヤーの画像のキー: 種類・版・評価済みのパラメータ・画布の大きさ・領域・品質・色の変換の設定の指紋と素材を読むデコーダーの実装（RFC 0013）・素材の `contentHash`・素材のフレームの番号（素材が無く時間で変わる種類は正確な時刻）。頼る ID はレイヤーと素材。
- 入れ子の画像のキー: 上に加えて、入れ子の先までのコンポジションの JSON と、レイヤー・効果・カット・トランジションが参照する素材のハッシュとデコーダーの実装の指紋。頼る ID は入れ子の先までのコンポジション・レイヤー・効果・素材。効果も含めて決定的でない種類があるか、描けなかったレイヤーがあった画像は覚えない。
- 覚えるのはレンダラーが持つ GPU のテクスチャ。デコーダーが借用したフレームは、`Rgba16Float` で `COPY_SRC` があれば GPU でコピーして覚え、そうでなければ覚えない。
- `FrameCacheConfig { memory_bytes, store }`（既定は 1 GiB、退避先なし）と `WgpuRenderer::with_frame_cache`。
- `invalidate(composition, range)`: そのコンポジションに頼った入れ子の画像だけ捨てる。素材は閉じない（範囲は見ない。時刻はキーに入っている）。
- `invalidate_touched(ids)`: `ids` に頼った画像を捨て、素材 ID が入っていればその素材だけ閉じる。
- デバイスを作り直すときは覚えた画像を解放せず忘れる。

### `mm-decode-ffmpeg-core`

- `index_cache::set_index_cache(store)` と `scan_video_cached`。`Timeline::build` が通る。3 つのデコーダー（ffmpeg、d3d12va、cuvid）の共通の道なので、デコーダーのインターフェースは変えない。
- 置き場所は処理全体で 1 つ（`mm-runtime` の組み立てが 1 回決める）。
- コンテナの索引（mov の `index_entries`）は使わない: エントリーの時刻は DTS で、B フレームの PTS は composition offset が要りパケットを読まないと得られない。mkv の cues は粗くて全フレームに足りない。
- 引く名前は素材の指紋とストリーム番号。種類は `CacheKind::Intermediate`（`cache-api` は変えない）。

### `mm-runtime`

- `Backend::Fs`（`cache: fs`）、`RuntimeConfig::cache_dir`、`frame_memory_bytes`。`cache: fs` に `cache_dir` が無ければ組み立てが拒否する。
- `Engine::cache()`、`Engine::invalidate_touched(ids)`。

## 迷った所

- 指紋は部分的なハッシュ。素材の内部だけが同じ大きさのまま変わると外れる。デコーダーに `Asset.contentHash` を渡せるよう `DecoderFactory::open` を変える案もあったが、3 つのデコーダーと `decode-api` に触るので、今回は見送った。
- 退避の条件は「2 回以上使われた」。4K の素材は 1 枚が約 64 MiB で、1 GiB に 16 枚しか入らず、60 フレームの区間を回すと LRU のために 1 度も当たらず、退避にも至らない。縮小してから覚える案は次の段階。
