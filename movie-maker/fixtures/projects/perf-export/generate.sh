#!/bin/sh
# 書き出しの性能のフィクスチャ。out/long-1080p60.mp4（60 秒、1080p60、毎フレーム動く映像と音）を作り、
# それを 5 秒ずつ 12 カットに切って素材の時刻を入れ替えて並べた project.json を書く。
# カットごとに素材の時刻が違うので、フレームのキャッシュが当たらず、編集点でデコーダーがシークする。
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$here/out"

FFMPEG=${FFMPEG:-ffmpeg}
SECONDS_TOTAL=60
CUT_SECONDS=5

"$FFMPEG" -y -v error -f lavfi -i "testsrc2=s=1920x1080:r=60:d=$SECONDS_TOTAL" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$SECONDS_TOTAL,aformat=channel_layouts=stereo" \
  -c:v libx264 -preset veryfast -crf 18 -g 60 -pix_fmt yuv420p \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 \
  -c:a aac -b:a 192k -shortest "$here/out/long-1080p60.mp4"

jq -n --argjson total "$SECONDS_TOTAL" --argjson cut "$CUT_SECONDS" '
  def num($v): {value: {type: "number", value: $v}, keyframes: []};
  def t($n; $d): {num: $n, den: $d};
  def zero_db: {role: "voice", gain_db: num(0.0), pan: num(0.0)};
  ($total / $cut) as $cuts |
  def footage($i): {
    id: "l-cut-\($i)",
    source: {kind: "footage", version: 1, inputs: [],
      params: {asset: {value: {type: "asset_ref", value: "clip"}, keyframes: []}}},
    start: t($i * $cut; 1),
    # 7 と 12 は互いに素なので、素材の 12 区間を 1 回ずつ、順を入れ替えて使う
    in: t((($i * 7) % $cuts) * $cut; 1),
    out: t((($i * 7) % $cuts) * $cut + $cut; 1),
    opacity: num(1.0), audio: zero_db, lock: false
  };
  {
    schema_version: 2,
    assets: [{
      id: "clip", path: "out/long-1080p60.mp4", content_hash: "perf-export-long-1080p60",
      color_space: "bt709", duration: t($total; 1), fps: t(60; 1), timebase_offset: t(0; 1)
    }],
    compositions: [{
      id: "main", width: 1920, height: 1080, fps: t(60; 1), duration: t($total; 1),
      layers: [range(0; $cuts) | footage(.)]
    }],
    render_targets: [{
      id: "mp4", composition: "main", width: 1920, height: 1080, fps: t(60; 1),
      codec: "h264_nvenc", output_path: ".mm/out/perf-export.mp4", loudness_lufs: null
    }]
  }' > "$here/project.json"
