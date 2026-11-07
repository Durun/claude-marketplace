## 構造と開発

| 用語 | 定義 | 識別子 |
|---|---|---|
| geneva のタイムライン | DTM_Vlog の `build_ep.py` が `edit.yaml` から作る、geneva の書き出しの入力の JSON。ep03 の再現はこれを取り込む | `timeline.json`、`mm geneva` |
| 変換できない要素 | geneva のタイムラインのうち movie-maker のプロジェクトに移せず、数えて報告する要素（マスク、効果、回転など） | `gaps`、`geneva_cmd::report::Gap` |
| 前の書き出し | 再現の比べる相手にする、前に書き出した完成の動画 | `reference`、`render/ep03_final.mp4` |
| 包絡の差 | 50 ms の区間ごとの音の大きさ（RMS の dB）の差の平均。参照が -45 dBFS より大きい区間だけ数える | `Sound::envelope_db` |
| ずれ（音の） | 前の書き出しに対する音の時刻の差。相関が最大になる位置で求める | `Sound::lag_ms` |
| サンプルのピーク | 書き出しの音のサンプルの絶対値の最大（dBFS、全部のチャンネル）。トゥルーピークと違い、サンプルの間の値は含めない | `Sound::peak_dbfs`、`measure::peak_dbfs` |

## ノードと描画

| 用語 | 定義 | 識別子 |
|---|---|---|
| 置き方 | `footage` と `image` が素材を画布に置く方法。`contain` は縦横比を保って画布に収め、`none` は素材の画素のまま左上に置く | `FIT_PARAM`、`FIT_CONTAIN`、`FIT_NONE` |
| クリップの矩形 | geneva で、切り抜いた素材を `fit` で収めた矩形。アンカーと拡大の基準 | `geneva_cmd::visual::Geometry` |
| 切り抜き | 素材の一部の矩形だけを使うこと。movie-maker はレイヤーの切り抜きで表す | `crop`、`Layer.crop` |

## デコードと色

| 用語 | 定義 | 識別子 |
|---|---|---|
| 映像の無い素材 | 映像のストリームを持たず、音だけを持つ素材（WAV、FLAC、M4A）。フレームを返さず、描く領域は空 | `SoundOnlyDecoder` |
| 前置き（AAC の） | AAC のエンコーダーが先頭に足し、デコーダーが捨てるサンプル（priming）。MP4 では edit list と `skip_samples` で表す | `set_packet_time_base` |
| 使わないデコーダー | 決まったフレーム数の間に使わなかった、素材のデコーダー。閉じて、次に使うときに開き直す | `IDLE_FRAMES`、`SourcePool::begin_frame` |

## 書き出しと納品

| 用語 | 定義 | 識別子 |
|---|---|---|
| 時間で変わる音量 | キーフレームを持つレイヤーの音量。レンダラーが 64 サンプルごとに評価してサンプルに掛け、ミキサーには 0 dB で渡す | `apply_gain_envelope`、`GAIN_STEP` |
| 雑音の置き換え（PNS） | AAC のエンコーダーが雑音に近い帯域を符号の代わりに雑音の大きさだけで送る仕組み。クリックを作ることがあるので書き出しでは切る | `aac_pns` |
| クリック | 数サンプルだけの不連続な大きな値。耳には短い「プツ」という音に聞こえる | `PEAK_MAX_DBFS` |
