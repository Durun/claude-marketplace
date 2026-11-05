# mm-decode-cuvid（試作）

FFmpeg の cuvid（NVDEC）で復号した CUDA のメモリを、D3D12 の共有バッファへ 1 回コピーして wgpu に渡す。
`d3d12va` が使えなかったときの逃げ道の実測。結果と判断は [`docs/design/spikes/nvdec.md`](../../docs/design/spikes/nvdec.md)。

## feature

FFmpeg、CUDA、D3D12 に触る所は feature `cuvid` の中にあり、既定で切ってある。
切ったままなら FFmpeg も libclang も要らず、ワークスペースはそのままビルドできる。
CUDA は `nvcuda.dll`（NVIDIA のドライバー）を実行時に読むので、CUDA Toolkit は要らない。

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
cargo build --release -p mm-decode-cuvid --features cuvid --examples
./target/release/examples/bench_cuvid.exe <素材> <decode|copy|hash> [--frames N] [--inflight K]
```

## 経路

1. `h264_cuvid` / `hevc_cuvid` が `AV_PIX_FMT_CUDA`（`data[0]` が Y、`data[1]` が UV のデバイスポインタ）で返す。
2. D3D12 の共有バッファ（`D3D12_HEAP_FLAG_SHARED`）を CUDA の外部メモリとして取り込み、Y と UV を `cuMemcpy2DAsync` で 1 回コピーする。
3. D3D12 の共有フェンスを CUDA の外部セマフォとして取り込み、コピーの後に `cuSignalExternalSemaphoresAsync` で値を進める。
4. wgpu 側は `Queue::add_wait_fence` で同じフェンスの値を待ち、バッファを `create_buffer_from_hal` で包んだストレージバッファとして読む。

## 範囲

試作なので、`Decoder` の実装までは行かず、bench で可否と時間を測る。シークと音は無い。
