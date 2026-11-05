#!/bin/sh
# FFmpeg ありの設定で fmt / clippy / test を回す（ローカルの確認用。手順は crates/mm-decode-ffmpeg/README.md）。
set -u
cd "$(dirname "$0")/.."
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
F=mm-decode-ffmpeg/ffmpeg,mm-encode-ffmpeg/ffmpeg,mm-runtime/ffmpeg,mm/ffmpeg,mm-decode-d3d12va/d3d12va,mm-decode-cuvid/cuvid
MODE="${1:-clippy}"
case "$MODE" in
  clippy) cargo clippy --workspace --all-targets --features "$F" -- -D warnings ;;
  test) MM_REQUIRE_GPU=1 cargo test --workspace --features "$F" ;;
  render) cargo run -p mm --features ffmpeg -- render fixtures/projects/stage0/project.json -o target/stage0-check.mp4 ;;
esac
echo "exit $?"
