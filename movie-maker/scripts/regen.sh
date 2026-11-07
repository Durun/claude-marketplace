#!/bin/sh
# 生成物を元から作り直す。マージで生成物が衝突したら手で直さず、これを実行してから git add する。
#   Cargo.lock                                 衝突していれば main の側を土台にし、足りない分だけ cargo に足させる
#   crates/mm-ops/schema/ops.json              操作の型から書き直す（tests/it/schema.rs）
#   crates/mm-ingest-api/schema/*.schema.json  付属ファイルの型から書き直す
#   ui/sdk/src/generated/ops.ts                ops.json から書き直す（ui/tools/gen-sdk.ts）
# CARGO_TARGET_DIR などの cargo の環境変数はそのまま引き継ぐ。
set -eu
cd "$(dirname "$0")/.."
base="${BASE:-origin/main}"

if grep -q '^<<<<<<< ' Cargo.lock; then
  git show "$base:movie-maker/Cargo.lock" > Cargo.lock
  echo "Cargo.lock: $base の側を土台にした"
fi
cargo metadata --format-version 1 > /dev/null

MM_UPDATE_SCHEMA=1 cargo test -q -p mm-ops --test it -- 凍結したスキーマは最新
MM_UPDATE_SCHEMA=1 cargo test -q -p mm-ingest-api --test schema -- 凍結したスキーマは最新
node ui/tools/gen-sdk.ts

git status --short -- Cargo.lock crates/mm-ops/schema crates/mm-ingest-api/schema ui/sdk/src/generated
