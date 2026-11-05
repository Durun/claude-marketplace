# mm-decode-d3d12va（試作）

FFmpeg の `d3d12va` で復号した NV12 の `ID3D12Resource` を、コピーなしで wgpu のテクスチャに包む。
結果と判断は [`docs/design/spikes/nvdec.md`](../../docs/design/spikes/nvdec.md)。

## feature

FFmpeg と D3D12 に触る所は feature `d3d12va` の中にあり、既定で切ってある。
切ったままなら FFmpeg も libclang も要らず、ワークスペースはそのままビルドできる（中身は定数 `ENABLED` と `ffi` の構造体だけ）。

入れるときは FFmpeg の準備が要る（`mm-decode-ffmpeg` の README と同じ。FFmpeg 9.0 の shared ビルドと libclang）。

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
cargo build --release -p mm-decode-d3d12va --features d3d12va --examples
MM_SPIKE_SAMPLE=<H.264 の mp4> cargo test -p mm-decode-d3d12va --features d3d12va
```

## 測る

```sh
./target/release/examples/bench_d3d12va.exe <素材> <mode> [--frames N] [--inflight K]
```

| mode | 内容 |
|---|---|
| `decode` | 復号だけ |
| `zero` | 復号 → wgpu に包む → RGBA に変換して完了まで待つ（コピーなし） |
| `cpu` | 復号 → `av_hwframe_transfer_data` → アップロード → 同じ変換 |
| `verify` | 2 経路の同じコマを画素で比べる。`SPIKE_NOWAIT=1` でフェンス待ちを外した対照 |
| `seek` | 40 か所へシークし、頼んでからテクスチャが使えるまで |

時間は `mm-trace` のスパン（`decode.receive`、`decode.wrap`、`decode.convert`、`decode.gpu_wait`、`decode.transfer`、`decode.upload`、`decode.seek`）で出す。
素材は ffmpeg の CLI で作る（`target/` の下など、リポジトリの外に置く）。

## 範囲

試作なので、`Decoder` の契約テストは通していない。固定フレームレートだけを扱い、音は無音を返す。
`next_frame` が返す `GpuFrameHandle` の中身は `D3d12vaDecoder::take_frame` で引く。口には `gpu` の受け渡しの型が無く、RFC が要る（nvdec.md の「口に要る変更」）。
