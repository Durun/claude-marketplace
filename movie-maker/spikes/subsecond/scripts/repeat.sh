#!/usr/bin/env bash
# 使い方: repeat.sh <ログ> <ソース> <マーカーの接頭辞> <回数> [開始バージョン=1]
# ソースのマーカーが <接頭辞>-v<開始バージョン> の状態から、次のバージョンへ書き換えて measure.sh で毎回測る。
# ログに同じマーカーが残っていると正しく測れないので、開始バージョンはログに出ていないマーカーの直前にする。
set -eu
dir=$(dirname "$0")
log=$1; src=$2; prefix=$3; n=$4; start=${5:-1}
for i in $(seq "$start" $((start + n - 1))); do
  bash "$dir/measure.sh" "$log" "$src" "$prefix-v$i" "$prefix-v$((i + 1))"
  sleep 1
done
