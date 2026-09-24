# claude-marketplace

Claude Code のスキルを配布する公開マーケットプレイスです。

Claude Code のクラウドセッションは利用者のマシンの `~/.claude` を参照しません。
公開マーケットプレイスに置いたプラグインは、クラウドセッションのセットアップスクリプトから取得できます。

## 収録プラグイン

**tech-writing**：日本語技術文書の文章規範と、技術キーワードの調査・解説ページ作成。

- `japanese-tech-writing`：整形、段落と論証の構成、読み手の負荷の管理、冗長の排除を定める文章規範。
- `keyword-explainer`：ライブラリやプロトコルなどのキーワードを公式リファレンスから調査し、Notion に解説ページを作成する。Notion の MCP サーバーを前提とする。

**gankooyaji**：Claude の舐めた返答に頑固オヤジがツッコミを入れる関数フック。
回答を読んで曖昧な言葉遣いを見つけると、次の入力欄の直上にツッコミを出します。`/gankooyaji` で ON/OFF を切り替えます。

**claude-run**：`/claude-run` で Claude が走って柱を跳び越えるゲームを入力欄の上に出す関数フック。

**gh-stack**：スタックしたブランチとプルリクエストを扱う `gh` CLI 拡張のスキル。
GitHub 公式リポジトリ [github/gh-stack](https://github.com/github/gh-stack) をそのまま参照します。

## 導入

```bash
claude plugin marketplace add Durun/claude-marketplace
claude plugin install tech-writing@durun-toolbox
claude plugin install gankooyaji@durun-toolbox
claude plugin install claude-run@durun-toolbox
claude plugin install gh-stack@durun-toolbox
```

gankooyaji と claude-run は Function Hooks で動きます。早期アクセスの機能なので、環境変数で有効にします。

```json
// ~/.claude/settings.json
{ "env": { "CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1" } }
```

## クラウドセッションで使う

[claude.ai/code](https://claude.ai/code) の環境設定ダイアログで、セットアップスクリプトに次を記述します。

```bash
claude plugin marketplace add Durun/claude-marketplace
claude plugin install tech-writing@durun-toolbox -y --scope user
claude plugin install gankooyaji@durun-toolbox -y --scope user
claude plugin install claude-run@durun-toolbox -y --scope user
claude plugin install gh-stack@durun-toolbox -y --scope user
```

Function Hooks を使うプラグインは、環境変数の欄に `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` も設定します。

設定先は `~/.claude/settings.json` なので、対象のリポジトリに変更を加えずに済みます。

## 開発

関数フックのプラグインは `scripts/typecheck.sh` で型検査します。
Claude Code の版が変わると型定義（`/plugin-types` の出力）を `.claude/types/` に生成し直してから、`plugins/*/tsconfig.json` を持つプラグインを全部 tsc にかけます。

版が変わったときだけ自動で走らせるには、`~/.claude/settings.json` の SessionStart フックに足します。

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "$HOME/an/_scratches/claude-marketplace-public/scripts/typecheck.sh --if-stale",
            "async": true,
            "timeout": 300
          }
        ]
      }
    ]
  }
}
```
