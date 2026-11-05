# 難所の試作: NVDEC → wgpu をコピーなしで

[ロードマップ](../15-roadmap.md#段階-0-で最初に確かめる難所)の 3 行目と、[未決事項](../16-open-questions.md)の 2 の結果。
コードは `crates/mm-decode-d3d12va`（d3d12va）と `crates/mm-decode-cuvid`（cuvid + CUDA の外部メモリ）。数字は `mm-trace` のスパンで取った。

## 結論

- **d3d12va はコピーなしで wgpu に包めた。** 4K の H.264 と HEVC で、画素は CPU 経由の経路と一致した（比べた 21 コマで差の最大 0）。包むのにかかる時間は 1 µs ほど。
- **速度は NVDEC 本体で決まり、経路で変わらない。** 4K で d3d12va は 540〜590 fps、cuvid の 1 コピーは 560〜600 fps。CPU 経由は 250〜265 fps（4K）で、半分以下。
- **cuvid + CUDA の外部メモリの 1 コピーも通った。** コピーを積む時間は 18 µs で、速度は d3d12va と同じ。
- **推奨は d3d12va を既定、cuvid を素材ごとの逃げ道、CPU 経由を最後に置く。** NVENC で作った 4K の HEVC が NVIDIA の d3d12va で復号できないため、逃げ道は飾りではなく要る（下の「詰まった所」）。

## 経路

| 経路 | 絵の運び方 | 同期 |
|---|---|---|
| d3d12va | `ID3D12Resource`（NV12）→ `texture_from_raw` → `wgpu::Texture`（NV12） | FFmpeg の `ID3D12Fence` と値 → `Queue::add_wait_fence` |
| cuvid | CUDA のデバイスポインタ（NV12）→ `cuMemcpy2DAsync` → D3D12 の共有バッファ → `wgpu::Buffer` | `cuSignalExternalSemaphoresAsync` で共有フェンスを進める → `Queue::add_wait_fence` |

### d3d12va で包む手順

1. wgpu の D3D12 デバイスを作り、その `ID3D12Device` を `AVD3D12VADeviceContext.device` に入れて FFmpeg のデバイスにする。同じデバイスの上に復号結果が置かれるので、共有ハンドルは要らない。`resource_flags` に `ALLOW_SIMULTANEOUS_ACCESS` を付ける。
2. `get_format` で `AV_PIX_FMT_D3D12` を選ぶ。復号したコマの `data[0]` が `AVD3D12VAFrame`（`texture`、`subresource_index`、`sync_ctx.fence`、`sync_ctx.fence_value`）。
3. `ID3D12Resource` を AddRef し、`wgpu::hal::dx12::Device::texture_from_raw`（`TextureFormat::NV12`）→ `Device::create_texture_from_hal`（`initial_state = TextureUses::RESOURCE`）で包む。資源ごとに 1 回で、FFmpeg のプールは同じ資源を使い回すので以降は引くだけ。
4. 面の見え方は `Plane0` を `R8Unorm`、`Plane1` を `Rg8Unorm` にする。配列のときは `base_array_layer = subresource_index`。
5. 使う `Queue::submit` の前に `queue.as_hal::<Dx12>().add_wait_fence(fence, value)` を呼ぶ。キューが復号の完了を待つ（CPU は待たない）。
6. wgpu が読み終えるまで、復号したコマ（`AVFrame`）を握る。放すとプールに戻り、復号が上書きする。

同期は素直だった。wgpu-hal 30 に `add_wait_fence` があり、FFmpeg のフェンスをそのまま渡せる。フェンスの待ちを外した対照では、同じコマが壊れた（画素差の平均 1.85、最大 255）。待ちは必要で、かつ効いている。

### cuvid で包む手順

1. `h264_cuvid` / `hevc_cuvid` に CUDA のデバイスを渡し、`AV_PIX_FMT_CUDA`（`data[0]` が Y、`data[1]` が UV のデバイスポインタ）で受ける。
2. D3D12 の共有バッファ（`D3D12_HEAP_FLAG_SHARED`）と共有フェンスを作り、NT ハンドルを `cuImportExternalMemory` と `cuImportExternalSemaphore` で CUDA に取り込む。
3. コマごとに `cuMemcpy2DAsync` を 2 回（Y と UV）積み、`cuSignalExternalSemaphoresAsync` でフェンスの値を進める。復号と同じストリームに積むので順序は保たれる。
4. wgpu 側は `create_buffer_from_hal` で包んだストレージバッファとして読み、`add_wait_fence` で待つ。

CUDA は `nvcuda.dll` を実行時に読むので、CUDA Toolkit は要らない。ただし CUDA の構造体（`CUDA_EXTERNAL_MEMORY_HANDLE_DESC` など）を手で宣言しており、unsafe が増える（`cuda.rs` と `share.rs` で約 350 行）。

## 実測

RTX 5090、ドライバー 616.92、FFmpeg 9.0（BtbN shared）、wgpu 30.0.1。素材は ffmpeg の CLI で作った testsrc2 の 60 fps、GOP 120、B 2 枚。H.264 と HEVC（NVENC）は 60 Mbps、HEVC（x265）も 60 Mbps、1080p は 20 Mbps。500 コマ（先に 30 コマを捨てる）。

「包むまで」は復号が返ってから、wgpu が NV12 → RGBA8 の描画パスを読み終えるまで。`inflight` は完了を待たずに積んでおく枚数。

### 4K の fps

| 経路 | H.264 | HEVC（x265） | HEVC（NVENC） |
|---|---|---|---|
| 復号だけ（d3d12va） | 588 | 556 | 失敗 |
| d3d12va → wgpu（コピーなし） inflight 0 / 4 | 575 / 580 | 542 / 563 | 失敗 |
| cuvid の復号だけ | 594 | 561 | 563 |
| cuvid → 1 コピー → wgpu inflight 0 / 4 | 599 / 599 | 567 / 560 | 569 / — |
| CPU 経由（転送 → アップロード → wgpu） inflight 0 / 4 | 260 / 247 | 265 / 248 | 失敗 |
| 参考: ffmpeg の CLI、CUDA に置いたまま | 576 | 550 | 548 |

1080p の H.264 は、d3d12va の復号だけが 1,817 fps、コピーなしが 1,806 / 1,857、CPU 経由が 611 / 600。cuvid は復号だけが 1,973、1 コピーが 1,983 / 2,004。

予算の表の「4K 700 fps 以上」には届いていない。この素材は 60 Mbps で、ffmpeg の CLI で CUDA に置いたままでも 550〜576 fps なので、経路ではなく NVDEC 本体とこの素材の限りと見てよい（前の実測 700〜800 fps は別の素材）。素材の作りで変わるため、予算の数は実素材（OBS の録画、スマホ）で測り直す。

### 4K の 1 コマの内訳（H.264、スパンの p50 / p95、ms）

| スパン | コピーなし | CPU 経由 | cuvid の 1 コピー |
|---|---|---|---|
| `decode.receive`（復号の発行まで） | 0.14 / 0.22 | 0.28 / 0.37 | 1.0 / 3.2（復号が同期で返る） |
| `decode.wrap`（包む + フェンスを積む） | 0.001 / 0.001 | | |
| `decode.transfer`（GPU → CPU） | | 1.78 / 3.50 | |
| `decode.upload`（CPU → GPU） | | 0.75 / 0.92 | |
| `decode.copy`（CUDA のコピーを積む） | | | 0.018 / 0.024 |
| `decode.convert`（描画パスを積む） | 0.07 / 0.10 | 0.12 / 0.20 | 0.08 / 0.14 |
| `decode.gpu_wait`（inflight 0） | 0.06 / 4.64 | 0.10 / 0.18 | 0.15 / 0.22 |
| `decode.frame`（1 コマの合計） | 0.32 / 4.75 | 3.43 / 5.10 | 1.28 / 3.43 |

- コピーなしの `decode.gpu_wait` の p95 は、復号が先行していて GPU が復号の完了を待つ時間。経路の費用ではない。
- CPU 経由は 1 コマ 3.4 ms の費用が転送とアップロードに乗り、60 fps（16.7 ms）に収まるが、4K を複数本並べると 250 fps で頭打ちになる。
- 包む初回（資源ごとに 1 回）は最大 0.05 ms。包んだ資源の数は 6〜8。

### シーク（4K、GOP 120、40 か所、d3d12va）

| 素材 | 中央値 | p95 | 最大 |
|---|---|---|---|
| H.264 | 114 ms | 195 ms | 220 ms |
| HEVC（x265） | 109 ms | 201 ms | 213 ms |

`decode.seek`（頼んでからテクスチャが使えるまで）。キーフレームから目的のコマまで復号して捨てる時間が支配的で、包む費用は 4 µs。GOP が長い原本は、予算の表のとおりプレビュー動画に頼る。

### 正しさ

- 4K の H.264 と HEVC で、150 コマの復号のうち 21 コマを、コピーなしと CPU 経由で RGBA に変えて比べた。差の最大は 0（プールの資源が 6 回使い回された後も同じ）。
- cuvid と d3d12va の同じコマは、3,300 万バイト中 1〜3 バイトが ±1 だけ違った。シェーダーの読み方の丸め（ストレージバッファのバイトと、`R8Unorm` の読み）の違いで、経路の誤りではない。

## 詰まった所

1. **FFmpeg の既定は NVIDIA ではなく内蔵 GPU を選ぶ。** `av_hwdevice_ctx_create(D3D12VA)` は先頭のアダプター（この PC では Ryzen の内蔵 GPU）を選び、4K HEVC が 131 fps、ffmpeg の CLI でも 134〜205 fps だった。wgpu の `ID3D12Device` を渡す作りなら、wgpu が選んだ GPU（高性能）で復号するので起きない。CLI で試すときは `-init_hw_device d3d12va=nv:1` のようにアダプターの番号を指す。
2. **NVENC で作った 4K の HEVC は、NVIDIA の d3d12va で復号できない。** 1 コマ目の `send_packet` が `EINVAL`（`hardware accelerator failed to decode picture`）で落ちる。ffmpeg の CLI 単体でも再現し、こちらの実装の誤りではない。NVENC の 4K（CTB 32、最小 CB 16）で起き、x265 の 4K（CTB 32 と 64）と NVENC の 1080p、NVENC の 4K の H.264 は通る。cuvid はこの素材を復号できる。原因は未調査（FFmpeg の d3d12va か NVIDIA のドライバー）。OBS の NVENC の録画が同じ形なら当たる。
3. **`ALLOW_SIMULTANEOUS_ACCESS` は、無くても動いた。** この環境では付けても付けなくても画素も速度（522 対 560 fps、ゆらぎの内）も同じ。D3D12 は `COMMON` の資源の暗黙の昇格を、このフラグが付いた texture にだけ保証するので付けてある。デバッグレイヤー付きの確認はしていない。
4. **HEVC のシーク後に `Could not get surface index. Using 0 instead.` が出る。** 40 回のシークのうち 1 回、目的の 1 コマ後が出た（先頭の RASL ピクチャの参照が無いときの動きと思われる）。コマ番号の契約テストで見る。
5. **`create_texture_from_hal` の `initial_state`。** wgpu 30 は初期状態を要求する。`UNINITIALIZED` にすると最初の使用でゼロ埋めされる恐れがあるので、`RESOURCE` を渡した。

## まだ測っていないこと

- 並行 4 本（予算の表の「4 本の合計 1,400 fps」）。同じ `ID3D12Device` に複数のデコーダーを載せたときの動き。
- 配列テクスチャのモード（`AV_D3D12VA_FRAME_FLAG_TEXTURE_ARRAY`）。層を `base_array_layer` に渡す作りは入れたが、今回の素材では資源ごとに別だったので通っていない。
- 10 bit（P010）、HDR、AV1、VP9。
- D3D12 のデバッグレイヤーを付けた確認。
- 描画器の中での使い方（`mm-render` の合成に NV12 を直接読ませる）。ここでは RGBA8 に変える 1 パスだけ。

## 既定の経路の推奨

**d3d12va を既定にし、素材ごとに cuvid へ、最後は CPU 経由へ落とす。**（確度 70%。未決事項の 55% から上げる。）

| 観点 | d3d12va | cuvid + CUDA の外部メモリ |
|---|---|---|
| 4K の速度 | 540〜590 fps | 560〜600 fps（同じ） |
| 包む費用 | 1 µs（コピーなし） | 18 µs（積むだけ。VRAM の中のコピーは非同期） |
| 同期 | wgpu-hal の `add_wait_fence` 1 回。フェンスは FFmpeg が持つ | 共有フェンス + CUDA の外部セマフォ。両方向のハンドル管理が要る |
| 実装の重さ | unsafe は FFmpeg のコマを読む所と包む所 | nvcuda の読み込み、構造体の手書き、D3D12 の共有バッファ、約 350 行 |
| 素材の対応 | NVENC の 4K HEVC が落ちる（原因未調査） | 通った |
| 後の拡張 | OFX の CUDA とは別の経路 | OFX の CUDA と同じ経路に合う |
| メモリの使い方 | プールの資源を握る間は復号が詰まる | 共有バッファの輪（NV12 で 4K 12 MB × 枠数） |

d3d12va を選ぶ理由は、同じ速度で、コピーが無く、同期が素直で、実装が小さいこと。cuvid を逃げ道として残す理由は、d3d12va が落ちる素材が実在することと、OFX を足すときに CUDA の経路が要ること。決めるのは既定だけで、どちらも `mm-decode-api` の実装として残せる。

逃げ道の切り替えは、`open` の後に最初の数コマを復号して `Err` なら次の実装を試す、という作りにする（上の 2 は 1 コマ目で落ちる）。実装は `mm-runtime` の設定で並べる。

## 口に要る変更

`mm-decode-api` の `GpuFrameHandle { id, size }` では、wgpu のテクスチャを描画器に渡せない。次を RFC で決める。

1. **受け渡し。** `GpuFrameHandle` に型を消した中身（`Arc<dyn Any + Send + Sync>`）を持たせ、d3d12va の実装は「Y と UV の見え方 + 復号の完了を待たせる関数」を入れる。描画器が型を戻して使う。または、デコーダーと描画器が共有する登録簿を `id` で引く。
2. **同じ GPU の文脈。** d3d12va は wgpu の `ID3D12Device` の上でしか動かない。`DecoderFactory::open(path)` は GPU の文脈を受け取る口が要る（試作は `D3d12vaFactory::new(gpu)` で持たせた）。
3. **寿命。** コマを握っている間は FFmpeg のプールが詰まる（`extra_hw_frames` を 16 にした）。描画器は、使い終わったら即 `Drop` で返すか、キャッシュに置くコマを NV12 のままにせず RGBA（中間形式）に変えて置く。`FrameSource::frame_at` が返す GPU のコマは「その `submit` が終わるまで」の借り物にする契約が要る。
4. **同期。** 描画器は `Queue::submit` の前に、使う GPU のコマすべてについて待ちを登録する。これを `GpuFrameHandle` の中身の関数にする。

## 再現

README は各クレートにある（`crates/mm-decode-d3d12va/README.md`、`crates/mm-decode-cuvid/README.md`）。素材は ffmpeg の CLI で `target/` の下に作る。

```sh
ffmpeg -f lavfi -i "testsrc2=size=3840x2160:rate=60:duration=10" -c:v h264_nvenc -b:v 60M -g 120 -bf 2 \
  -pix_fmt yuv420p -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv target/spike/4k_h264.mp4
bench_d3d12va.exe target/spike/4k_h264.mp4 zero --inflight 4 --frames 500
bench_cuvid.exe   target/spike/4k_h264.mp4 copy --inflight 4 --frames 500
```
