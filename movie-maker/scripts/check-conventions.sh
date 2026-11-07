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

# 結合テストは 1 クレートに 1 つの実行ファイル（docs/ci.md）。2 つ目からは、冒頭に「別の実行ファイル」と理由を書いたものだけ。
for t in crates/*/tests apps/*/tests; do
  [ -d "$t" ] || continue
  bins="$(ls "$t"/*.rs "$t"/*/main.rs 2>/dev/null || true)"
  [ "$(printf '%s\n' "$bins" | grep -c .)" -le 1 ] && continue
  for f in $bins; do
    [ "$f" = "$t/it/main.rs" ] && continue
    if ! grep -q "別の実行ファイル" "$f"; then
      echo "$f: 結合テストは $t/it/ に置き、main.rs に mod を足す（docs/ci.md）" >&2
      status=1
    fi
  done
done

# RFC の番号は PR の番号。作業中は docs/rfcs/<題>.md で書き、PR を作ったら <PR の番号 4 桁>-<題>.md に改める。
if [ -n "${PR_NUMBER:-}" ] && [ -n "${BASE:-}" ]; then
  want="$(printf '%04d' "$PR_NUMBER")"
  # for の中の置換の失敗は set -e に掛からないので、先に取って失敗を検査の失敗にする。
  added="$(git diff --name-only --diff-filter=A "$BASE...HEAD" -- docs/rfcs)" || exit 1
  for f in $added; do
    case "$(basename "$f")" in
      "$want"-*.md) ;;
      *.md)
        echo "$f: RFC のファイルの名前は ${want}-<題>.md にする（番号は PR の番号）" >&2
        status=1
        ;;
    esac
  done
fi

exit $status
