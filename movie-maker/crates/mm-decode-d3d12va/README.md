# mm-decode-d3d12va

FFmpeg の `d3d12va` で復号した NV12 / P010 の `ID3D12Resource` を、コピーなしで wgpu のテクスチャに包み、描画器と同じデバイスの上で線形の `Rgba16Float` に変えて返す `Decoder`。
結果と判断は [`docs/design/spikes/nvdec.md`](../../docs/design/spikes/nvdec.md)。`mm-runtime` の `decode = "d3d12va"`（と `auto`）で選ぶ。

## feature

FFmpeg と D3D12 に触る所は feature `d3d12va` の中にあり、既定で切ってある。
切ったままなら FFmpeg も libclang も要らず、ワークスペースはそのままビルドできる（中身は定数 `ENABLED` と `ffi` の構造体だけ）。

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
cargo test -p mm-decode-d3d12va --features d3d12va
```

## 作り

- デバイスは `DecoderFactory::open` の `gpu`（`mm-gpu` の `GpuContext`）。D3D12 のアダプターで作る必要があり、頼み方は `gpu_request()`（`GpuRequest::backends` を D3D12 にする）。`required_features()` が NV12、P010、16 bit の形式を求める。
- コマの絵は `GpuFrameTexture`（`Rgba16Float`、線形、premultiplied）。変換先は借りが終われば置き場へ戻る。
- 復号した面は、変換を読み終えたことを wgpu のキューのフェンスで確かめるまで FFmpeg のプールに返さない（`shared/release.rs`）。
- 開く前に、別プロセス `mm-d3d12va-probe` で先頭 4 コマを復号して確かめる（NVIDIA の d3d12va は失敗するとプロセスの D3D12 のデバイスが全部消えるため）。実行ファイルは `MM_D3D12VA_PROBE`、なければ実行ファイルと同じ場所（かその一つ上）の `mm-d3d12va-probe.exe`。無ければ試さずに通す。
- 時間軸（PTS の索引、VFR）、シーク、フェンスの待ち、変換の係数は `src/shared/` にあり、`mm-decode-cuvid` が `#[path]` で同じソースを取り込む。
- 音は無音を返す。`mm-runtime` が FFmpeg のデコーダーの音と素材の情報を付ける。

## 測る

```sh
cargo build --release -p mm-decode-d3d12va --features d3d12va --bins --examples
./target/release/examples/bench_d3d12va.exe <素材> <decode|render|seek> [--frames N] [--warmup N]
```

時間は `mm-trace` のスパン（`decode.next_frame`、`decode.wrap`、`decode.seek`）で出す。素材は ffmpeg の CLI で作る（`target/` の下など、リポジトリの外に置く）。

## テスト

`tests/contract.rs` が `mm-testkit` の契約（CFR と VFR）、焼いた番号、シーク、10 bit、プール、フェンス（GPU を 600 ms 遅らせる負荷）を実機で確かめる。
素材は `fixtures/media/generate.sh` が作る（10 bit の HEVC と NVENC の 4K HEVC を含む。NVENC が無い PC では後者を作らず、そのテストは飛ぶ）。
