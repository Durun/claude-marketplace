#!/bin/bash
# Claude Code の版が変わったら型定義を生成し直し、関数フックのプラグインを全部 tsc にかける。
# 型は早期アクセスで版ごとに変わる。$.model.complete の戻りが文字列からオブジェクトに変わったとき、
# 古い型のままでは tsc が通ってしまい、実行時の TypeError でフックが無言で捨てられた。
#
#   scripts/typecheck.sh             版が変わっていれば再生成してから検査する
#   scripts/typecheck.sh --if-stale  版が同じなら何もしない（SessionStart フック向け）
#   scripts/typecheck.sh --force     版に関係なく再生成する
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TYPES="$ROOT/.claude/types"
STAMP="$TYPES/.claude-version"

# 型の生成は claude -p で行う。その入れ子セッションの SessionStart から自分を呼び直さないための印。
if [ -n "${CLAUDE_MARKETPLACE_TYPECHECK-}" ]; then exit 0; fi
export CLAUDE_MARKETPLACE_TYPECHECK=1

mode=${1-}
current=$(claude --version 2>/dev/null | head -1)
stamped=$(cat "$STAMP" 2>/dev/null || true)

if [ "$mode" = "--if-stale" ] && [ "$current" = "$stamped" ]; then exit 0; fi

if [ "$mode" = "--force" ] || [ "$current" != "$stamped" ]; then
  mkdir -p "$TYPES"
  (cd "$ROOT" && claude -p "/plugin-types $TYPES" --output-format text >/dev/null </dev/null)
  printf '%s\n' "$current" >"$STAMP"
  echo "plugin types regenerated for $current"
fi

status=0
for cfg in "$ROOT"/plugins/*/tsconfig.json; do
  dir=$(dirname "$cfg")
  if ! (cd "$dir" && npx -y -p typescript@5 tsc -p tsconfig.json); then
    echo "tsc failed: $(basename "$dir")"
    status=1
  fi
done
exit $status
