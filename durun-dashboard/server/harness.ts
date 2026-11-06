// 学習アプリの「知見の一覧」。Claude Code が読む指示（CLAUDE.md・スキル・記憶・フック）を、いつ効くかと置き場所とともに並べる。
// 中身はどれも利用者のファイルで、ここでは読むだけにする。書き換えは、人間がメモ帳で開いて行う。
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { HarnessItem } from '../shared/types.ts';

const read = (file: string): string | undefined => {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return undefined;
  }
};

const dirs = (dir: string): string[] => {
  try {
    return fs.readdirSync(dir, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => path.join(dir, e.name));
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
const instructionsOf = (claudeDir: string, cwds: string[]): HarnessItem[] => {
  const items: HarnessItem[] = [];
  const user = path.join(claudeDir, 'CLAUDE.md');
  const userText = read(user);
  if (userText !== undefined) items.push({ kind: 'instructions', name: 'CLAUDE.md', trigger: 'いつも', source: 'ユーザー', path: user, chars: userText.length });
  const seen = new Set([user]);
  for (const cwd of cwds) {
    for (let dir = cwd; dir.startsWith(home) && dir !== home; dir = path.dirname(dir)) {
      const file = path.join(dir, 'CLAUDE.md');
      if (seen.has(file)) continue;
      seen.add(file);
      const text = read(file);
      if (text !== undefined) items.push({ kind: 'instructions', name: 'CLAUDE.md', trigger: `${shortPath(dir)} の下で動くとき`, source: shortPath(dir), path: file, chars: text.length });
    }
  }
  return items;
};

const skillsIn = (dir: string, source: string): HarnessItem[] =>
  dirs(dir).flatMap((d) => {
    const file = path.join(d, 'SKILL.md');
    const text = read(file);
    if (text === undefined) return [];
    const fm = frontmatterOf(text);
    return [{ kind: 'skill' as const, name: fm.name || path.basename(d), trigger: fm.description ?? '', source, path: file, chars: text.length }];
  });

type InstalledPlugins = { plugins?: Record<string, { installPath?: string }[]> };

const skillsOf = (claudeDir: string): HarnessItem[] => {
  const installed = JSON.parse(read(path.join(claudeDir, 'plugins', 'installed_plugins.json')) ?? '{}') as InstalledPlugins;
  const plugins = Object.entries(installed.plugins ?? {}).flatMap(([id, list]) => {
    const dir = list[0]?.installPath;
    return dir ? skillsIn(path.join(dir, 'skills'), id.split('@')[0] ?? id) : [];
  });
  return [...skillsIn(path.join(claudeDir, 'skills'), 'ユーザー'), ...plugins];
};

/** 記憶は、プロジェクトごとの memory/ にある。MEMORY.md は索引で、そのプロジェクトではいつも読まれる。 */
const memoriesOf = (claudeDir: string): HarnessItem[] =>
  dirs(path.join(claudeDir, 'projects')).flatMap((project) => {
    const dir = path.join(project, 'memory');
    const source = path.basename(project).replace(/^-Users-[^-]+-[^-]+-?/, '') || path.basename(project);
    let files: string[];
    try {
      files = fs.readdirSync(dir).filter((f) => f.endsWith('.md'));
    } catch {
      return [];
    }
    return files.flatMap((f) => {
      const file = path.join(dir, f);
      const text = read(file) ?? '';
      const index = f === 'MEMORY.md';
      return [{ kind: 'memory' as const, name: index ? '記憶の索引' : frontmatterOf(text).name || f, trigger: index ? 'このプロジェクトでいつも' : (frontmatterOf(text).description ?? ''), source, path: file, chars: text.length }];
    });
  });

type HookSettings = { hooks?: Record<string, { matcher?: string; hooks?: { command?: string }[] }[]> };

const hooksOf = (claudeDir: string): HarnessItem[] => {
  const file = path.join(claudeDir, 'settings.json');
  const settings = JSON.parse(read(file) ?? '{}') as HookSettings;
  return Object.entries(settings.hooks ?? {}).flatMap(([event, groups]) =>
    groups.flatMap((g) =>
      (g.hooks ?? []).map((h) => ({ kind: 'hook' as const, name: h.command ?? '', trigger: g.matcher ? `${event}（${g.matcher}）` : event, source: 'ユーザー', path: file, chars: (h.command ?? '').length })),
    ),
  );
};

/** cwds は、指示を集める作業ディレクトリ。 */
export const harnessItems = (claudeDir: string, cwds: string[]): HarnessItem[] => [...instructionsOf(claudeDir, cwds), ...skillsOf(claudeDir), ...memoriesOf(claudeDir), ...hooksOf(claudeDir)];
