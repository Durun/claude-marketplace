# mm-decode-cuvid

FFmpeg の cuvid（NVDEC）でデコードした CUDA のメモリを、D3D12 の共有バッファへ 1 回コピーし、レンダラーと同じ wgpu のデバイスの上で線形の `Rgba16Float` に変えて返す `Decoder`。
`d3d12va` が使えない素材（NVENC で作った 4K の HEVC など）のフォールバック。結果と判断は [`docs/design/spikes/nvdec.md`](../../docs/design/spikes/nvdec.md)。`mm-runtime` の `decode = "cuvid"`（と `auto`）で選ぶ。

## feature

FFmpeg、CUDA、D3D12 に触る所は feature `cuvid` の中にあり、既定で切ってある。
切ったままなら FFmpeg も libclang も要らず、ワークスペースはそのままビルドできる。
CUDA は `nvcuda.dll`（NVIDIA のドライバー）を実行時に読むので、CUDA Toolkit は要らない。

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
cargo test -p mm-decode-cuvid --features cuvid
./target/release/examples/bench_cuvid.exe <素材> <decode|render|seek> [--frames N] [--warmup N]
```

## 作り

1. `h264_cuvid` / `hevc_cuvid` が `AV_PIX_FMT_CUDA`（`data[0]` が Y、`data[1]` が UV のデバイスポインタ。10 bit は P010）で返す。CUDA のデバイスは、レンダラーの D3D12 のアダプターの LUID で選ぶ。
2. D3D12 の共有バッファ（`D3D12_HEAP_FLAG_SHARED`）を CUDA の外部メモリとして取り込み、Y と UV を `cuMemcpy2DAsync` で 1 回コピーする。
3. スロットごとの共有フェンスを CUDA と wgpu が交互に進める。奇数が「CUDA がコピーを終えた」、偶数が「wgpu が読み終えた」。CUDA は次のコピーの前に `cuWaitExternalSemaphoresAsync` で前の偶数を待ち、wgpu は `Queue::add_wait_fence` で奇数を待ち、提出のあとに偶数を `Signal` する。
4. 共有バッファをストレージバッファとして読む描画パスが、`Rgba16Float`（線形、premultiplied）の `GpuFrameTexture` に変える。
5. FFmpeg のサーフェス（CUDA のメモリ）は、コピーの完了までだけ保持する（`shared/release.rs`）。

時間軸（PTS の索引、VFR）、シーク、行列は `mm-decode-ffmpeg-core`、変換の係数、描画パス、キューの待ちは `mm-decode-hw` を使う（[RFC 0007](../../docs/rfcs/0007-decode-shared-crates.md)）。音は無音を返す（`mm-runtime` が FFmpeg の音を付ける）。

## テスト

`tests/it/contract.rs` が `mm-testkit` の契約（CFR と VFR）、焼き込んだ番号、シーク、10 bit、リングバッファ、フェンス（GPU を 600 ms 遅らせる負荷）、NVENC の 4K HEVC を実機で確かめる。
共通の補助は `mm-decode-d3d12va/tests/common/` を取り込む。素材は `fixtures/media/generate.sh`。
