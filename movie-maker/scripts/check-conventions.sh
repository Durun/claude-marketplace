#!/bin/sh
# 並行の PR が衝突しにくくなる決まり（docs/design/14-dev-structure.md の「並行開発の境界」）を確かめる。
#   sh scripts/check-conventions.sh                 ルートの Cargo.toml の並び
#   PR_NUMBER=N BASE=origin/main sh scripts/...     加えて、PR が足した RFC のファイルの名前が N で始まるか
set -eu
cd "$(dirname "$0")/.."
status=0

# workspace.dependencies の中のクレート（mm-*）の行が 1 か所にまとまり、名前の順か。
mm_lines="$(awk '/^\[/{t=$0} t=="[workspace.dependencies]" && /^mm-/{print NR": "$1}' Cargo.toml)"
names="$(printf '%s\n' "$mm_lines" | sed 's/^[0-9]*: //')"
if [ "$names" != "$(printf '%s\n' "$names" | LC_ALL=C sort)" ]; then
  echo "Cargo.toml: [workspace.dependencies] の mm-* を名前の順に並べる（末尾に足さない）" >&2
  status=1
fi
first="$(printf '%s\n' "$mm_lines" | head -n 1 | cut -d: -f1)"
last="$(printf '%s\n' "$mm_lines" | tail -n 1 | cut -d: -f1)"
count="$(printf '%s\n' "$mm_lines" | wc -l | tr -d ' ')"
if [ $((last - first + 1)) -ne "$count" ]; then
  echo "Cargo.toml: [workspace.dependencies] の mm-* を 1 か所にまとめる" >&2
  status=1
fi

# RFC の番号は PR の番号。作業中は docs/rfcs/<題>.md で書き、PR を作ったら <PR の番号 4 桁>-<題>.md に改める。
if [ -n "${PR_NUMBER:-}" ] && [ -n "${BASE:-}" ]; then
  want="$(printf '%04d' "$PR_NUMBER")"
  for f in $(git diff --name-only --diff-filter=A "$BASE...HEAD" -- docs/rfcs); do
    case "$(basename "$f")" in
      "$want"-*.md) ;;
      *)
        echo "$f: RFC のファイルの名前は ${want}-<題>.md にする（番号は PR の番号）" >&2
        status=1
        ;;
    esac
  done
fi

exit $status
