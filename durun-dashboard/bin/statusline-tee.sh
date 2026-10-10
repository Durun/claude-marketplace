#!/bin/sh
# durun-dashboard: statusline の入力 JSON をセッションごとに保存してから、元の statusline コマンドへ渡す。
# 保存した値から、ダッシュボードが正確なコンテキスト使用率とレート制限を表示する。
# 保存先のデータディレクトリは、install-statusline が引数で渡す。
data=${1:-$HOME/.durun-dashboard}
input=$(cat)
sid=$(printf '%s' "$input" | jq -r '.session_id // empty' 2>/dev/null)
# session_id はファイル名に使うので、英数字とハイフン以外を含むものは保存しない。
case "$sid" in
  '' | *[!0-9A-Za-z-]*) ;;
  *) mkdir -p "$data/statusline" && printf '%s' "$input" > "$data/statusline/$sid.json" ;;
esac
if [ -s "$data/statusline-original" ]; then
  printf '%s' "$input" | sh -c "$(cat "$data/statusline-original")"
fi
