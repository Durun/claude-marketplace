// 学習アプリの「知見の一覧」。Claude Code が読む指示（CLAUDE.md・スキル・記憶・フック）を、いつ効くかと置き場所とともに並べる。
// 中身はどれも利用者のファイルで、ここでは読むだけにする。書き換えは、人間がメモ帳で開いて行う。
// 有効・無効の切り替えだけは、Claude Code が設定に用意したキーを settings.json に書く。
// 数百のファイルを読むので、イベントループを止めないよう非同期にまとめて読む。
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import type { Harness, HarnessItem, HarnessToggle } from '../shared/types.ts';

const read = async (file: string): Promise<string | undefined> => {
  try {
    return await fs.readFile(file, 'utf8');
  } catch {
    return undefined;
  }
};

const dirs = async (dir: string): Promise<string[]> => {
  try {
    return (await fs.readdir(dir, { withFileTypes: true })).filter((e) => e.isDirectory()).map((e) => path.join(dir, e.name));
  } catch {
    return [];
  }
};

/** 先頭の --- で囲んだ frontmatter から、1 行の値と、| や > で続ける複数行の値を読む。 */
export const frontmatterOf = (text: string): Record<string, string> => {
  const body = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? '';
  const out: Record<string, string> = {};
  const lines = body.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const m = /^([\w-]+):\s*(.*)$/.exec(lines[i] ?? '');
    if (!m?.[1]) continue;
    let value = m[2] ?? '';
    if (value === '|' || value === '>' || value === '|-' || value === '>-') {
      const rest: string[] = [];
      while (/^\s+\S/.test(lines[i + 1] ?? '')) rest.push((lines[++i] ?? '').trim());
      value = rest.join(' ');
    }
    out[m[1]] = value.replace(/^(["'])(.*)\1$/, '$2');
  }
  return out;
};

const home = os.homedir();
const shortPath = (p: string): string => (p.startsWith(home) ? `~${p.slice(home.length)}` : p);

/** 作業ディレクトリから上へたどり、Claude Code が読む CLAUDE.md を集める。ホームより上は見ない。 */
const instructionsOf = async (claudeDir: string, cwds: string[], settings: Settings): Promise<HarnessItem[]> => {
  const user = path.join(claudeDir, 'CLAUDE.md');
  const found = new Map<string, string>([[user, '']]);
  for (const cwd of cwds) {
    for (let dir = cwd; dir.startsWith(home) && dir !== home; dir = path.dirname(dir)) {
      const file = path.join(dir, 'CLAUDE.md');
      if (!found.has(file)) found.set(file, dir);
    }
  }
  const items = await Promise.all(
    [...found].map(async ([file, dir]): Promise<HarnessItem[]> => {
      const text = await read(file);
      if (text === undefined) return [];
      if (!dir) return [{ kind: 'instructions', group: 'ユーザー', name: 'CLAUDE.md', trigger: 'いつも', source: 'ユーザー', path: user, chars: text.length }];
      const toggle: HarnessToggle = { setting: 'claudeMdExcludes', key: file, enabled: !settings.claudeMdExcludes?.includes(file) };
      return [{ kind: 'instructions', group: 'プロジェクト', name: 'CLAUDE.md', trigger: `${shortPath(dir)} の下で動くとき`, source: shortPath(dir), path: file, chars: text.length, toggle }];
    }),
  );
  return items.flat();
};

/** plugin を渡したスキルは、プラグインごと切り替える。skillOverrides はプラグインのスキルに効かないため。 */
const skillsIn = async (dir: string, source: string, settings: Settings, plugin?: string): Promise<HarnessItem[]> => {
  const items = await Promise.all(
    (await dirs(dir)).map(async (d): Promise<HarnessItem[]> => {
      const file = path.join(d, 'SKILL.md');
      const text = await read(file);
      if (text === undefined) return [];
      const fm = frontmatterOf(text);
      const name = fm.name || path.basename(d);
      const item: HarnessItem = { kind: 'skill', group: source, name, trigger: fm.description ?? '', source, path: file, chars: text.length };
      if (plugin) item.groupToggle = { setting: 'enabledPlugins', key: plugin, enabled: settings.enabledPlugins?.[plugin] !== false };
      else item.toggle = { setting: 'skillOverrides', key: name, enabled: settings.skillOverrides?.[name] !== 'off' };
      return [item];
    }),
  );
  return items.flat();
};

type InstalledPlugins = { plugins?: Record<string, { installPath?: string }[]> };

const skillsOf = async (claudeDir: string, settings: Settings): Promise<HarnessItem[]> => {
  const installed = JSON.parse((await read(path.join(claudeDir, 'plugins', 'installed_plugins.json'))) ?? '{}') as InstalledPlugins;
  const plugins = Object.entries(installed.plugins ?? {}).map(([id, list]) => {
    const dir = list[0]?.installPath;
    return dir ? skillsIn(path.join(dir, 'skills'), id.split('@')[0] ?? id, settings, id) : [];
  });
  return (await Promise.all([skillsIn(path.join(claudeDir, 'skills'), 'ユーザー', settings), ...plugins])).flat();
};

/** 記憶は、プロジェクトごとの memory/ にある。MEMORY.md は索引で、そのプロジェクトではいつも読まれる。 */
const memoriesOf = async (claudeDir: string): Promise<HarnessItem[]> => {
  const projects = await Promise.all(
    (await dirs(path.join(claudeDir, 'projects'))).map(async (project): Promise<HarnessItem[]> => {
      const dir = path.join(project, 'memory');
      const source = path.basename(project).replace(/^-Users-[^-]+-[^-]+-?/, '') || path.basename(project);
      let files: string[];
      try {
        files = (await fs.readdir(dir)).filter((f) => f.endsWith('.md'));
      } catch {
        return [];
      }
      return Promise.all(
        files.map(async (f): Promise<HarnessItem> => {
          const file = path.join(dir, f);
          const text = (await read(file)) ?? '';
          const index = f === 'MEMORY.md';
          return { kind: 'memory', group: source, name: index ? '記憶の索引' : frontmatterOf(text).name || f, trigger: index ? 'このプロジェクトでいつも' : (frontmatterOf(text).description ?? ''), source, path: file, chars: text.length };
        }),
      );
    }),
  );
  return projects.flat();
};

type Settings = {
  hooks?: Record<string, { matcher?: string; hooks?: { command?: string }[] }[]>;
  claudeMdExcludes?: string[];
  skillOverrides?: Record<string, string>;
  enabledPlugins?: Record<string, boolean>;
  disableAllHooks?: boolean;
  autoMemoryEnabled?: boolean;
};

/** フックはイベントごとの枝に分ける。1 つずつ止めるキーは Claude Code に無いので、切り替えはフック全体だけにする。 */
const hooksOf = (file: string, settings: Settings): HarnessItem[] =>
  Object.entries(settings.hooks ?? {}).flatMap(([event, groups]) =>
    groups.flatMap((g) =>
      (g.hooks ?? []).map((h) => ({ kind: 'hook' as const, group: event, name: h.command ?? '', trigger: g.matcher ? `${event}（${g.matcher}）` : event, source: 'ユーザー', path: file, chars: (h.command ?? '').length })),
    ),
  );

const settingsFile = (claudeDir: string): string => path.join(claudeDir, 'settings.json');

/** cwds は、指示を集める作業ディレクトリ。 */
export const harness = async (claudeDir: string, cwds: string[]): Promise<Harness> => {
  const file = settingsFile(claudeDir);
  const settings = JSON.parse((await read(file)) ?? '{}') as Settings;
  const items = (await Promise.all([instructionsOf(claudeDir, cwds, settings), skillsOf(claudeDir, settings), memoriesOf(claudeDir), hooksOf(file, settings)])).flat();
  return {
    items,
    kindToggles: {
      memory: { setting: 'autoMemoryEnabled', enabled: settings.autoMemoryEnabled !== false },
      hook: { setting: 'disableAllHooks', enabled: settings.disableAllHooks !== true },
    },
  };
};

/** 有効を既定として、無効のときだけ settings.json にキーを残す。 */
const apply = (s: Settings, { setting, key = '' }: Omit<HarnessToggle, 'enabled'>, on: boolean): void => {
  if (setting === 'claudeMdExcludes') {
    const rest = (s.claudeMdExcludes ?? []).filter((p) => p !== key);
    s.claudeMdExcludes = on ? rest : [...rest, key];
    if (!s.claudeMdExcludes.length) delete s.claudeMdExcludes;
  } else if (setting === 'skillOverrides') {
    const { [key]: _, ...rest } = s.skillOverrides ?? {};
    s.skillOverrides = on ? rest : { ...rest, [key]: 'off' };
    if (!Object.keys(s.skillOverrides).length) delete s.skillOverrides;
  } else if (setting === 'enabledPlugins') {
    s.enabledPlugins = { ...s.enabledPlugins, [key]: on };
  } else if (setting === 'disableAllHooks') {
    if (on) delete s.disableAllHooks;
    else s.disableAllHooks = true;
  } else if (on) delete s.autoMemoryEnabled;
  else s.autoMemoryEnabled = false;
};

/** ほかのキーは読んだまま書き戻す。読めない settings.json は書き換えず、例外のまま返す。dotfiles のリンクを保つため、置き換えずにその場へ書く。 */
export const setHarnessToggle = async (claudeDir: string, toggle: Omit<HarnessToggle, 'enabled'>, on: boolean): Promise<void> => {
  const file = settingsFile(claudeDir);
  const settings = JSON.parse((await read(file)) ?? '{}') as Settings;
  apply(settings, toggle, on);
  await fs.writeFile(file, `${JSON.stringify(settings, null, 2)}\n`);
};
