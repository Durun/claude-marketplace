// statusline の入力を保存するラッパーを Claude Code のユーザー設定に登録する（install）、または外す（uninstall）。
// フックと MCP サーバーはプラグイン durun-dashboard が登録する。statusLine はプラグインから設定できないので、ここで書く。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { platform } from './platform.ts';
import { DATA_DIR } from './store.ts';

const CLAUDE_DIR = process.env.CLAUDE_CONFIG_DIR ?? path.join(os.homedir(), '.claude');
const SETTINGS = path.join(CLAUDE_DIR, 'settings.json');
const BACKUP = `${SETTINGS}.bak-durun-dashboard`;
const TEE = platform.shellPath(path.join(import.meta.dirname, '..', 'bin', 'statusline-tee.sh'));
const STATUSLINE_ORIGINAL = path.join(DATA_DIR, 'statusline-original');

type Settings = { statusLine?: { type?: string; command?: string } & Record<string, unknown> };

const installStatusline = (settings: Settings): void => {
  const current = settings.statusLine?.command ?? '';
  // 登録済みなら元のコマンドを上書きせず、データディレクトリの引数だけ今の値にそろえる。
  if (!current.includes(TEE)) {
    fs.mkdirSync(path.dirname(STATUSLINE_ORIGINAL), { recursive: true });
    fs.writeFileSync(STATUSLINE_ORIGINAL, current);
  }
  settings.statusLine = { ...settings.statusLine, type: 'command', command: `sh '${TEE}' '${platform.shellPath(DATA_DIR)}'` };
};

const uninstallStatusline = (settings: Settings): void => {
  if (!settings.statusLine?.command?.includes(TEE)) return;
  const original = fs.existsSync(STATUSLINE_ORIGINAL) ? fs.readFileSync(STATUSLINE_ORIGINAL, 'utf8') : '';
  if (original) settings.statusLine.command = original;
  else delete settings.statusLine;
};

const editSettings = (edit: (s: Settings) => void): void => {
  const raw = fs.existsSync(SETTINGS) ? fs.readFileSync(SETTINGS, 'utf8') : '{}';
  const settings = JSON.parse(raw) as Settings;
  edit(settings);
  fs.writeFileSync(BACKUP, raw);
  fs.writeFileSync(SETTINGS, `${JSON.stringify(settings, null, 2)}\n`);
};

const mode = process.argv[2];
if (mode === 'install') {
  editSettings(installStatusline);
  process.stdout.write(`statusline のラッパーを登録しました。設定の控え: ${BACKUP}\n`);
} else if (mode === 'uninstall') {
  editSettings(uninstallStatusline);
  process.stdout.write(`外しました。設定の控え: ${BACKUP}\n`);
} else {
  process.stderr.write('usage: node server/setup.ts install|uninstall\n');
  process.exit(1);
}
