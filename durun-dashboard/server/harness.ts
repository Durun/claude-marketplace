// 学習アプリの「知見の一覧」。Claude Code が読む指示（CLAUDE.md・スキル・記憶・フック）を、いつ効くかと置き場所とともに並べる。
// 中身はどれも利用者のファイルで、ここでは読むだけにする。書き換えは、人間がメモ帳で開いて行う。
// 数百のファイルを読むので、イベントループを止めないよう非同期にまとめて読む。
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import type { HarnessItem } from '../shared/types.ts';

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
const instructionsOf = async (claudeDir: string, cwds: string[]): Promise<HarnessItem[]> => {
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
      return [dir ? { kind: 'instructions', name: 'CLAUDE.md', trigger: `${shortPath(dir)} の下で動くとき`, source: shortPath(dir), path: file, chars: text.length } : { kind: 'instructions', name: 'CLAUDE.md', trigger: 'いつも', source: 'ユーザー', path: user, chars: text.length }];
    }),
  );
  return items.flat();
};

const skillsIn = async (dir: string, source: string): Promise<HarnessItem[]> => {
  const items = await Promise.all(
    (await dirs(dir)).map(async (d): Promise<HarnessItem[]> => {
      const file = path.join(d, 'SKILL.md');
      const text = await read(file);
      if (text === undefined) return [];
      const fm = frontmatterOf(text);
      return [{ kind: 'skill', name: fm.name || path.basename(d), trigger: fm.description ?? '', source, path: file, chars: text.length }];
    }),
  );
  return items.flat();
};

type InstalledPlugins = { plugins?: Record<string, { installPath?: string }[]> };

const skillsOf = async (claudeDir: string): Promise<HarnessItem[]> => {
  const installed = JSON.parse((await read(path.join(claudeDir, 'plugins', 'installed_plugins.json'))) ?? '{}') as InstalledPlugins;
  const plugins = Object.entries(installed.plugins ?? {}).map(([id, list]) => {
    const dir = list[0]?.installPath;
    return dir ? skillsIn(path.join(dir, 'skills'), id.split('@')[0] ?? id) : [];
  });
  return (await Promise.all([skillsIn(path.join(claudeDir, 'skills'), 'ユーザー'), ...plugins])).flat();
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
          return { kind: 'memory', name: index ? '記憶の索引' : frontmatterOf(text).name || f, trigger: index ? 'このプロジェクトでいつも' : (frontmatterOf(text).description ?? ''), source, path: file, chars: text.length };
        }),
      );
    }),
  );
  return projects.flat();
};

type HookSettings = { hooks?: Record<string, { matcher?: string; hooks?: { command?: string }[] }[]> };

const hooksOf = async (claudeDir: string): Promise<HarnessItem[]> => {
  const file = path.join(claudeDir, 'settings.json');
  const settings = JSON.parse((await read(file)) ?? '{}') as HookSettings;
  return Object.entries(settings.hooks ?? {}).flatMap(([event, groups]) =>
    groups.flatMap((g) =>
      (g.hooks ?? []).map((h) => ({ kind: 'hook' as const, name: h.command ?? '', trigger: g.matcher ? `${event}（${g.matcher}）` : event, source: 'ユーザー', path: file, chars: (h.command ?? '').length })),
    ),
  );
};

/** cwds は、指示を集める作業ディレクトリ。 */
export const harnessItems = async (claudeDir: string, cwds: string[]): Promise<HarnessItem[]> =>
  (await Promise.all([instructionsOf(claudeDir, cwds), skillsOf(claudeDir), memoriesOf(claudeDir), hooksOf(claudeDir)])).flat();
