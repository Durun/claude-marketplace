#!/bin/sh
# 性能の回帰のゲート。同じ runner で基点と PR の `mm` を交互に回し、`mm perf compare` で比べる。
# 使い方: scripts/perf-gate.sh <基点の mm> <PR の mm> <結果の置き場所>
# 比べる場面は 2 組。`cpu`（操作の適用など）と、WARP（D3D12 のソフトウェアの GPU）で回す `playback`・`scrub`。
# 閾値と回数の理由は docs/ci.md。1 回目で回帰が出たら、同じ回数を足して束ね直し、それでも出たら失敗する。
set -eu

base_mm=$1
head_mm=$2
out=$3

THRESHOLD=${PERF_THRESHOLD:-30%}
# この絶対値（ms）より小さい差は、runner の揺れとして無視する。
MIN_DELTA=${PERF_MIN_DELTA:-0.05}
ROUNDS=${PERF_ROUNDS:-3}
REPEAT=${PERF_REPEAT:-5}
# WARP の描画の場面。CPU で描くので runner の負荷の影響が大きく、閾値は cpu より緩い。
WARP_THRESHOLD=${PERF_WARP_THRESHOLD:-40%}
WARP_MIN_DELTA=${PERF_WARP_MIN_DELTA:-2}
WARP_REPEAT=${PERF_WARP_REPEAT:-3}
WARP_ADAPTER=${PERF_WARP_ADAPTER:-Microsoft Basic Render Driver}
# WARP が実時間で追いつかなくても測れるよう、小さい絵で短く回す。
WARP_SIZE=${PERF_WARP_SIZE:-480x270}
WARP_SECONDS=${PERF_WARP_SECONDS:-1}
WARP_SEEKS=${PERF_WARP_SEEKS:-20}
# 0 なら WARP の場面を回さない。
WARP=${PERF_WARP:-1}

rm -rf "$out"
mkdir -p "$out/base" "$out/head" "$out/warp-base" "$out/warp-head"

cpu_run() {
  "$1" perf run cpu --repeat "$REPEAT" --out-dir "$2" > /dev/null
}

warp_run() {
  "$1" perf run playback --seconds "$WARP_SECONDS" --repeat "$WARP_REPEAT" --size "$WARP_SIZE" \
    --adapter "$WARP_ADAPTER" --out-dir "$2" > /dev/null
  "$1" perf run scrub --seeks "$WARP_SEEKS" --repeat "$WARP_REPEAT" --size "$WARP_SIZE" \
    --adapter "$WARP_ADAPTER" --out-dir "$2" > /dev/null
}

round() {
  cpu_run "$base_mm" "$out/base"
  cpu_run "$head_mm" "$out/head"
  if [ "$WARP" = 1 ]; then
    warp_run "$base_mm" "$out/warp-base"
    warp_run "$head_mm" "$out/warp-head"
  fi
}

# 基点に cpu シナリオが無い（このシナリオを足す PR 自身）ときは比べる相手がない。
if ! "$base_mm" perf run cpu --repeat 1 --out-dir "$out/probe" > /dev/null 2>&1; then
  echo "基点の mm に perf run cpu が無いので、比較を飛ばす（PR の側だけ測る）"
  "$head_mm" perf run cpu --repeat "$REPEAT" --out-dir "$out/head"
  exit 0
fi
rm -rf "$out/probe"

# 基点に `--adapter` が無い（WARP の場面を足す PR 自身）ときは、WARP の比較を飛ばして PR の側だけ測る。
if [ "$WARP" = 1 ] && ! "$base_mm" perf run scrub --seeks 1 --repeat 1 --size 160x90 \
  --adapter "$WARP_ADAPTER" --out-dir "$out/probe" > /dev/null 2>&1; then
  echo "基点の mm に perf run --adapter が無いので、WARP の比較を飛ばす（PR の側だけ測る）"
  warp_run "$head_mm" "$out/warp-head"
  WARP=0
fi
rm -rf "$out/probe"

compare() {
  status=0
  "$head_mm" perf compare "$out/base" "$out/head" \
    --threshold "$THRESHOLD" --min-delta "$MIN_DELTA" || status=1
  if [ "$WARP" = 1 ]; then
    "$head_mm" perf compare "$out/warp-base" "$out/warp-head" \
      --threshold "$WARP_THRESHOLD" --min-delta "$WARP_MIN_DELTA" || status=1
  fi
  return "$status"
}

i=0
while [ "$i" -lt "$ROUNDS" ]; do
  round
  i=$((i + 1))
done
if compare; then
  exit 0
fi

echo "回帰が出たので、同じ回数を足して確かめる"
i=0
while [ "$i" -lt "$ROUNDS" ]; do
  round
  i=$((i + 1))
done
compare
