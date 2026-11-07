#!/usr/bin/env bash
# sandbox つきで動く mm-host-cef を 1 つのフォルダーに組み立てる（Windows）。
#   使い方: crates/mm-host-cef/bundle.sh <置き場>
#   CEF_PATH          CEF の配布物の配置先（既定 D:/dev/cef/156）
#   CARGO_TARGET_DIR  cargo の target（既定 D:/dev/mmt。パスが長いと CEF の C++ のビルドが失敗する）
#
# 置き場には、CEF の bootstrapc.exe を mm-host-cef.exe の名前で、本体の DLL（examples/sandboxed.rs）を
# mm-host-cef.dll の名前で、CEF の実行時のファイルと並べて置く。bootstrapc.exe は自分と同じ名前の DLL を読み込み、
# sandbox の情報を渡す。アプリは MM_CEF_HOST でこの mm-host-cef.exe を指す。
set -euo pipefail
out="${1:?置き場を指定する}"
export CEF_PATH="${CEF_PATH:-D:/dev/cef/156}"
target="${CARGO_TARGET_DIR:-D:/dev/mmt}"
cd "$(dirname "$0")/../.."
CARGO_TARGET_DIR="$target" cargo build --release -p mm-host-cef --features cef --example sandboxed
mkdir -p "$out/locales"
# 同じドライブなら複製せずにハードリンクにする（配布物は 400 MB を超える）。
link() { cp -l -f "$1" "$2" 2>/dev/null || cp -f "$1" "$2"; }
for f in "$CEF_PATH"/*.dll "$CEF_PATH"/*.pak "$CEF_PATH"/*.bin "$CEF_PATH"/*.dat "$CEF_PATH"/*.json; do
  link "$f" "$out/"
done
for f in "$CEF_PATH"/locales/*; do
  link "$f" "$out/locales/"
done
cp -f "$CEF_PATH/bootstrapc.exe" "$out/mm-host-cef.exe"
cp -f "$target/release/examples/sandboxed.dll" "$out/mm-host-cef.dll"
# sandbox の LPAC（ネットワークのサービスなど）が置き場を読めるようにする。CEF の cmake の SET_LPAC_ACLS と同じ。
MSYS_NO_PATHCONV=1 icacls "$(cygpath -w "$out")" /grant '*S-1-15-2-2:(OI)(CI)(RX)' >/dev/null
echo "$out/mm-host-cef.exe"
