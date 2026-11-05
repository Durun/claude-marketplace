#!/usr/bin/env bash
# 使い方: repeat.sh <ログ> <ソース> <印の接頭辞> <回数> [開始の版=1]
# ソースの印が <接頭辞>-v<開始の版> の状態から、次の版へ書き換えて measure.sh で毎回測る。
# ログに同じ印が残っていると正しく測れないので、開始の版はログに出ていない印の直前にする。
set -eu
dir=$(dirname "$0")
log=$1; src=$2; prefix=$3; n=$4; start=${5:-1}
for i in $(seq "$start" $((start + n - 1))); do
  bash "$dir/measure.sh" "$log" "$src" "$prefix-v$i" "$prefix-v$((i + 1))"
  sleep 1
done
