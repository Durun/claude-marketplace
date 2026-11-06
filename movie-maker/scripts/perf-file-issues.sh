#!/bin/sh
# 予算を超えた項目を GitHub の issue にする。同じ題の開いた issue があれば、コメントを足すだけ。
# 使い方: scripts/perf-file-issues.sh <mm> <結果のフォルダー>
# 必要な環境変数: GH_TOKEN（Actions では GITHUB_TOKEN。issues: write）、GITHUB_REPOSITORY。
# 実行は main への push と夜間だけ（PR では呼ばない。.github/workflows/movie-maker.yml）。
set -eu

mm=$1
results=$2

over=$("$mm" perf budget "$results" --json)
count=$(printf '%s' "$over" | jq 'length')
if [ "$count" -eq 0 ]; then
  echo "予算を超えた項目は無い"
  exit 0
fi

run_url="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID:-0}"
sha=${GITHUB_SHA:-unknown}

printf '%s' "$over" | jq -c '.[]' | while read -r row; do
  name=$(printf '%s' "$row" | jq -r '.name')
  title="perf: ${name} が予算を超えた"
  body=$(printf '%s' "$row" | jq -r --arg run "$run_url" --arg sha "$sha" '
    "予算の表の行: \(.row)\n\n" +
    "- 項目: `\(.name)`（シナリオ `\(.scene)`）\n" +
    "- 測った値: \(.value) \(.unit)\n" +
    "- 予算: \(if .kind == "at_most" then "以下" else "以上" end) \(.limit) \(.unit)\n" +
    "- コミット: \($sha)\n" +
    "- 実行: \($run)\n\n" +
    "手元で確かめる: `mm perf run \(.scene)`、`mm perf budget <結果>`（docs/ci.md）。"')
  existing=$(gh issue list --state open --search "\"$title\" in:title" --json number,title \
    --jq "[.[] | select(.title == \"$title\")][0].number // empty")
  if [ -n "$existing" ]; then
    gh issue comment "$existing" --body "$body"
  else
    gh issue create --title "$title" --body "$body"
  fi
done
