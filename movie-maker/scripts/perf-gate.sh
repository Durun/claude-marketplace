#!/bin/sh
# 性能の回帰のゲート。同じ runner で基点と PR の `mm` を交互に回し、`mm perf compare` で比べる。
# 使い方: scripts/perf-gate.sh <基点の mm> <PR の mm> <結果の置き場所>
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

rm -rf "$out"
mkdir -p "$out/base" "$out/head"

round() {
  "$base_mm" perf run cpu --repeat "$REPEAT" --out-dir "$out/base" > /dev/null
  "$head_mm" perf run cpu --repeat "$REPEAT" --out-dir "$out/head" > /dev/null
}

# 基点に cpu シナリオが無い（このシナリオを足す PR 自身）ときは比べる相手がない。
if ! "$base_mm" perf run cpu --repeat 1 --out-dir "$out/probe" > /dev/null 2>&1; then
  echo "基点の mm に perf run cpu が無いので、比較を飛ばす（PR の側だけ測る）"
  "$head_mm" perf run cpu --repeat "$REPEAT" --out-dir "$out/head"
  exit 0
fi
rm -rf "$out/probe"

compare() {
  "$head_mm" perf compare "$out/base" "$out/head" --threshold "$THRESHOLD" --min-delta "$MIN_DELTA"
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
