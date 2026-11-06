# RFC 0007: デコーダーの共通実装（mm-decode-ffmpeg-core、mm-decode-hw）

状態: 採用

## 変える理由

- `mm-decode-cuvid` が `mm-decode-d3d12va` のソースを `#[path]` で取り込んでいた。依存の規則 3（実装どうしは依存しない）の抜け道で、テストの補助も同じ形だった。
- 映像の時間軸（`scan_video`、`detect_fps`、`fits`、`is_discarded`、`vfr_index`、`Matrix`）が `mm-decode-ffmpeg` と d3d12va の 2 か所にあり、シークの判断はすでにずれていた（ffmpeg は GOP を見る `can_read_forward`、GPU の実装は定数 30）。

先行事例は RFC 0001 の `mm-gpu`（共通実装を 1 つ例外にする）。同じ形で、共通実装を実装から切り出して名前のあるクレートにする。

## 決めたこと

共通実装のクレートを 2 つ置く。GPU に依存するかで分ける（ffmpeg のデコーダーが GPU の依存を引かないため）。

| クレート | 依存 | 内容 |
|---|---|---|
| `mm-decode-ffmpeg-core` | インターフェース、基礎、FFmpeg（feature `ffmpeg`） | `timeline`（PTS の索引、CFR の判定、VFR の時刻、`can_read_forward`）、`reader`（`StreamReader`、`Sequencer`: 番号づけとシーク）、`matrix`（`Matrix`、レンジ、PQ と HLG の判定） |
| `mm-decode-hw` | 上と `mm-gpu`、wgpu、D3D12（feature `hw`） | `adapter`（D3D12 のアダプターの頼み方）、`device`（キューの待ちと信号、デバイスの消失の確認）、`release`（デコードしたサーフェスの保持）、`yuv`（係数と WGSL）、`render`（変換の描画パス）、`pool`、`testing`（統合テストの補助。feature `testing`） |

- `mm-decode-ffmpeg`、`mm-decode-d3d12va`、`mm-decode-cuvid` は共通実装に依存してよい。共通実装はインターフェースと基礎、`mm-gpu` にだけ依存し、実装には依存しない。`mm-gpu` と並べて依存の規則 3 の例外に書く。
- `mm-decode-ffmpeg` も `Sequencer` を使う。3 つの実装で番号づけとシークの判断（`can_read_forward`）が 1 本になる。
- 検査のためのインターフェース（待ちを外す、故障の注入）は feature `test-hooks` の中だけで公開する。

## 契約として決めた他のこと

### 変換の submit と待ちの登録（指摘 6）

`wgpu::hal::dx12::Queue::add_wait_fence` は「キューの次の submit」に付くので、別のスレッドが先に submit すると待ちがそちらに付く。待ちは `ID3D12CommandQueue::Wait` をキューへ直接積む（`mm_decode_hw::device::queue_wait`）。直接積んだ待ちは、積んだ後にキューへ入る全部の実行に効くので、どのスレッドの submit が挟まっても自分の変換より前に効く。信号は submit の後に直接積み、挟まった submit があっても遅れるだけで早まらない。デコーダーとレンダラーが別のスレッドで同じキューに submit してよい。

### PQ と HLG（指摘 12）

伝達特性が PQ（SMPTE ST 2084）か HLG（ARIB STD-B67）の素材は、`Sequencer::new` が `Err(Unsupported)` にする。3 つの実装が同じ判定で拒否する。BT.2020 の原色を作業色空間へ変換するかは色の名前の RFC 0005 で決める。

## 影響

- API クレート（`mm-*-api`）は変えない。
- `mm-decode-d3d12va` の公開は、`shared` モジュールが無くなり、`D3d12vaDecoder::finish()` が増えた。`set_gpu_wait` は feature `test-hooks`。`PROBE_FRAMES` はこれまでどおり公開の定数。
