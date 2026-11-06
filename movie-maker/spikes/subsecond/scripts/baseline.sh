#!/usr/bin/env bash
# 使い方: baseline.sh <パッケージ> <ソース> <マーカーの接頭辞> <回数> [ログに出る行の目印]
# フォールバックの時間を測る。マーカーを書き換えて cargo build（増分）の時間と、起動してから最初の TICK までの時間を出す。
# 起動は直接実行する。Tauri のアプリは WebView の起動を含む。
set -eu
pkg=$1; src=$2; prefix=$3; n=$4
bin=$(cargo metadata --no-deps --format-version 1 | jq -r '.target_directory')/debug/$pkg.exe
for i in $(seq 2 $((n + 1))); do
  sed -i "s/\"$prefix-v$((i - 1))\"/\"$prefix-v$i\"/" "$src"
  b0=$(date +%s%3N)
  cargo build -q -p "$pkg"
  b1=$(date +%s%3N)
  "$bin" > /tmp/baseline-run.log 2>&1 &
  pid=$!
  until grep -aq "TICK.*$prefix-v$i" /tmp/baseline-run.log; do sleep 0.02; done
  r1=$(date +%s%3N)
  taskkill //F //T //IM "$pkg.exe" >/dev/null 2>&1 || true
  while tasklist | grep -qi "$pkg.exe"; do sleep 0.2; done
  echo "$prefix-v$i: build $((b1 - b0)) ms, 起動から最初の TICK $((r1 - b1)) ms, 合計 $((r1 - b0)) ms"
  sleep 3
done
