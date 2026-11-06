#!/usr/bin/env bash
# 使い方: measure.sh <ログ> <ソース> <旧のマーカー> <新のマーカー>
# ソースのマーカーを書き換えた時刻(T0)から、ログに新しいマーカーの TICK が初めて出た時刻(T1, アプリ側の epoch ms)までを出す。
# 保存から反映までを、アプリ自身の時計で測るため。
set -eu
log=$1; src=$2; old=$3; new=$4
t0=$(date +%s%3N)
sed -i "s/\"$old\"/\"$new\"/" "$src"
while :; do
  line=$(grep -a -m1 "TICK .*$new" "$log" || true)
  [ -n "$line" ] && break
  sleep 0.02
  if [ $(( $(date +%s%3N) - t0 )) -gt 120000 ]; then echo "timeout"; exit 1; fi
done
t1=$(echo "$line" | sed -E 's/.*TICK ([0-9]+).*/\1/')
echo "$old -> $new: $(( t1 - t0 )) ms"
