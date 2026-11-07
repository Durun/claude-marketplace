#!/bin/sh
# CI のテスト。隔離した揺れるテスト（ラベル flaky の開いた issue）を外して回し、隔離分は別に回して結果を問わない。
# 再試行で通ったテストを JUnit から拾い、MM_FILE_FLAKY=1 のときは issue にして隔離する。仕組みは docs/ci.md。
# 使い方: scripts/nextest-ci.sh [cargo nextest run の引数...]
# 環境変数: MM_HEAVY=1（#[ignore] の通しのテストも回す）、MM_FILE_FLAKY=1（起票する）、GH_TOKEN、GITHUB_REPOSITORY。
set -u

LABEL=flaky
PREFIX="flaky: "
# 測定用の #[ignore]。release で手動で回すので、通しのテストを回すときも除く。
MEASURE='test(操作_10_万件の大きさ) | test(一万語の素材)'
NEXTEST_DIR="${CARGO_TARGET_DIR:-target}/nextest"
RUN_URL="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY:-}/actions/runs/${GITHUB_RUN_ID:-0}"

summary() {
  printf '%s\n' "$1"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then printf '%s\n' "$1" >> "$GITHUB_STEP_SUMMARY"; fi
}

# 開いた issue の題「flaky: <binary_id> <テスト名>」を nextest の filterset にする。読めなければ空（隔離なし）。
# nextest はどのバイナリにも当たらない binary_id() を誤りにするので、引数 $1（この実行のバイナリの一覧）に有るものだけ入れる。
# 題は filterset にそのまま入るので、CI が起票したものだけを読み、filterset の記号を含むテスト名は読みスキップする。
quarantine_filter() {
  titles=$(gh issue list --label "$LABEL" --state open --author app/github-actions --limit 500 \
    --json title --jq '.[].title' 2>/dev/null) || {
    echo "::warning::flaky の issue を読めないので、隔離なしで回す" >&2
    return 0
  }
  printf '%s\n' "$titles" | awk -v p="$PREFIX" -v bins="$1" '
    BEGIN { n = split(bins, a, "\n"); for (i = 1; i <= n; i++) have[a[i]] = 1 }
    index($0, p) == 1 {
      rest = substr($0, length(p) + 1); sp = index(rest, " ")
      if (sp == 0) next
      b = substr(rest, 1, sp - 1)
      if (!(b in have)) next
      t = substr(rest, sp + 1)
      if (t ~ /[()|&,\\ ]/ || b ~ /[()|&,\\ ]/) {
        print "::warning::flaky の issue の題を filterset にできないので読みスキップする: " $0 > "/dev/stderr"
        next
      }
      f = "(binary_id(=" b ") & test(=" t "))"
      out = (out == "") ? f : out " | " f
    }
    END { print out }'
}

# JUnit の flakyFailure（再試行で通った）を持つテストを「<binary_id> <テスト名>」で 1 行ずつ出す。
flaky_tests() {
  [ -f "$1" ] || return 0
  awk '
    function attr(s, k) {
      if (!match(s, " " k "=\"[^\"]*\"")) return ""
      return substr(s, RSTART + length(k) + 3, RLENGTH - length(k) - 4)
    }
    /<testcase / { name = attr($0, "name"); cls = attr($0, "classname") }
    /<flakyFailure/ { print cls " " name }' "$1" |
    sed -e 's/&lt;/</g' -e 's/&gt;/>/g' -e 's/&quot;/"/g' -e "s/&apos;/'/g" -e 's/&amp;/\&/g' | sort -u
}

# 1 つのテストを起票する。同じ題の開いた issue があれば、実行の URL をコメントに足すだけ。
file_issue() {
  binary=${1%% *}
  test=${1#* }
  title="${PREFIX}$1"
  body="再試行で通ったテスト（flaky）を自動で隔離した。この issue が開いている間、CI はこのテストを別に回して結果を問わない。

- テスト: \`${test}\`
- バイナリ: \`${binary}\`
- コミット: ${GITHUB_SHA:-unknown}
- 実行: ${RUN_URL}

手元で再現する: \`cargo nextest run -E 'binary_id(=${binary}) & test(=${test})' --retries 0\` を繰り返す。
直す PR に \`Closes #<この issue の番号>\` と書く。issue が閉じると隔離が外れる（docs/ci.md の「揺れるテストの隔離」）。"
  existing=$(gh issue list --label "$LABEL" --state open --search "in:title \"$1\"" --json number,title \
    --jq "[.[] | select(.title == \"$title\")][0].number // empty")
  if [ -n "$existing" ]; then
    gh issue comment "$existing" --body "再び揺れた: ${RUN_URL}"
  else
    gh issue create --title "$title" --label "$LABEL" --body "$body"
  fi
}

report_flaky() {
  found=$(flaky_tests "$NEXTEST_DIR/ci/junit.xml")
  [ -n "$found" ] || return 0
  summary "### 再試行で通ったテスト（flaky）"
  printf '%s\n' "$found" | while read -r line; do
    summary "- \`$line\`"
    echo "::warning title=flaky::$line"
  done
  [ "${MM_FILE_FLAKY:-0}" = 1 ] || return 0
  gh label create "$LABEL" --color FBCA04 --description "再試行で通ったテスト。CI が隔離する" --force > /dev/null 2>&1 || true
  printf '%s\n' "$found" | while read -r line; do file_issue "$line"; done
}

base='all()'
if [ "${MM_HEAVY:-0}" = 1 ]; then
  base="not ($MEASURE)"
  set -- --run-ignored all "$@"
fi
# 一覧はテストのバイナリを作るので、続く実行はビルド済みのものを使う。
binaries=$(cargo nextest list --list-type binaries-only --message-format json "$@" | jq -r '."rust-binaries" | keys[]')
quarantine=$(quarantine_filter "$binaries")
expr=$base
if [ -n "$quarantine" ]; then expr="($base) & not ($quarantine)"; fi

cargo nextest run --profile ci --no-tests=warn "$@" -E "$expr"
status=$?

if [ -n "$quarantine" ]; then
  summary "### 隔離したテスト（結果を問わない）"
  if cargo nextest run --profile quarantine --no-tests=warn "$@" -E "($base) & ($quarantine)"; then
    summary "全部通った。続けて通るなら issue を閉じて隔離を外せる"
  else
    summary "失敗したものがある。JUnit（artifact）を見る"
  fi
fi

report_flaky
exit "$status"
