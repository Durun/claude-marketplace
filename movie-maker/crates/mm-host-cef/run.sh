#!/usr/bin/env bash
# CEF の配布物の配置先を通して mm-host-cef を実行する。引数は mm-host-cef にそのまま渡す。
#   CEF_PATH    CEF の配布物の配置先（既定 D:/dev/cef/156）
#   MM_TARGET   cargo の target（既定 D:/dev/mmt。パスが長いと CEF の C++ のビルドが失敗するので短くする）
set -euo pipefail
export CEF_PATH="${CEF_PATH:-D:/dev/cef/156}"
export PATH="$PATH:$CEF_PATH"
exec "${MM_TARGET:-D:/dev/mmt}/release/mm-host-cef.exe" "$@"
