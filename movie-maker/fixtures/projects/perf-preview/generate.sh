#!/bin/sh
# 性能のフィクスチャ（1080p60、10 秒、solid 10 枚と text 3 枚）を project.json に書く。素材のファイルは使わない。
# 回転と拡大を付けた半透明の solid を重ねて、合成の負荷が出るようにする。
set -eu
here=$(cd "$(dirname "$0")" && pwd)

jq -n '
  def num($v): {value: {type: "number", value: $v}, keyframes: []};
  def t($n; $d): {num: $n, den: $d};
  def layer($id; $src): {
    id: $id, source: $src, start: t(0;1), in: t(0;1), out: t(10;1),
    opacity: num(0.7), lock: false, audio: null
  };
  def solid($i): {
    kind: "solid", version: 1, inputs: [],
    params: {color: {value: {type: "color", value: {r: (($i * 37 % 100) / 100), g: (($i * 61 % 100) / 100), b: (($i * 83 % 100) / 100), a: 1.0}}, keyframes: []}}
  };
  def text($i): {
    kind: "text", version: 1, inputs: [],
    params: {
      color: {value: {type: "color", value: {r: 1.0, g: 1.0, b: 1.0, a: 1.0}}, keyframes: []},
      position: {value: {type: "vec2", value: [(100 + $i * 300), (200 + $i * 250)]}, keyframes: []},
      size: num(72),
      text: {value: {type: "text", value: "性能のフィクスチャ \($i + 1)"}, keyframes: []}
    }
  };
  def moved($i): . + {transform: {
    position: {value: {type: "vec2", value: [($i * 40), ($i * 20)]}, keyframes: []},
    scale: num(0.9),
    rotation: num($i * 3)
  }};
  {
    schema_version: 2,
    assets: [],
    compositions: [{
      id: "main", width: 1920, height: 1080, fps: t(60;1), duration: t(10;1),
      layers: ([range(0;10)] | map(. as $i | layer("l-solid-\($i)"; solid($i)) | moved($i)))
        + ([range(0;3)] | map(. as $i | layer("l-text-\($i)"; text($i))))
    }],
    render_targets: [{
      id: "mp4", composition: "main", width: 1920, height: 1080, fps: t(60;1),
      codec: "h264", output_path: ".mm/out/perf-preview.mp4", loudness_lufs: null
    }]
  }' > "$here/project.json"
