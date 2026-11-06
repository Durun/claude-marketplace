import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { frontmatterOf, harness, setHarnessToggle } from '../server/harness.ts';
import { tempDir } from './tmp.ts';

test('frontmatter の 1 行の値と、> で続ける複数行の値を読む', () => {
  assert.deepEqual(frontmatterOf('---\nname: lint\ndescription: >\n  lint を直す\n  ときに使う\n---\n本文'), { name: 'lint', description: 'lint を直す ときに使う' });
  assert.deepEqual(frontmatterOf('本文だけ'), {});
});

test('CLAUDE.md・スキル・記憶・フックを、いつ効くかとともに並べる', async () => {
  const dir = tempDir('durun-dashboard-harness-');
  const write = (rel: string, text: string) => {
    fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
    fs.writeFileSync(path.join(dir, rel), text);
  };
  write('CLAUDE.md', '全体の指示');
  write('skills/lint/SKILL.md', '---\nname: lint\ndescription: lint を直すときに使う\n---\n');
  write('projects/-Users-me-work-repo/memory/MEMORY.md', '- 索引');
  write('projects/-Users-me-work-repo/memory/a.md', '---\nname: a\ndescription: 手順はスキルにしない\n---\n');
  write('settings.json', JSON.stringify({ hooks: { PreToolUse: [{ matcher: 'Bash', hooks: [{ command: 'guard.sh' }] }] } }));
  const items = (await harness(dir, [os.tmpdir()])).items.map(({ kind, group, name, trigger }) => ({ kind, group, name, trigger }));
  assert.deepEqual(items, [
    { kind: 'instructions', group: 'ユーザー', name: 'CLAUDE.md', trigger: 'いつも' },
    { kind: 'skill', group: 'ユーザー', name: 'lint', trigger: 'lint を直すときに使う' },
    { kind: 'memory', group: 'repo', name: '記憶の索引', trigger: 'このプロジェクトでいつも' },
    { kind: 'memory', group: 'repo', name: 'a', trigger: '手順はスキルにしない' },
    { kind: 'hook', group: 'PreToolUse', name: 'guard.sh', trigger: 'PreToolUse（Bash）' },
  ]);
});

test('切り替えは無効のときだけ settings.json にキーを残し、ほかのキーはそのまま書き戻す', async () => {
  const dir = tempDir('durun-dashboard-harness-');
  const file = path.join(dir, 'settings.json');
  fs.writeFileSync(file, JSON.stringify({ model: 'opus', skillOverrides: { other: 'off' } }));
  fs.mkdirSync(path.join(dir, 'skills/lint'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'skills/lint/SKILL.md'), '---\nname: lint\n---\n');
  await setHarnessToggle(dir, { setting: 'skillOverrides', key: 'lint' }, false);
  await setHarnessToggle(dir, { setting: 'enabledPlugins', key: 'p@m' }, false);
  await setHarnessToggle(dir, { setting: 'disableAllHooks' }, false);
  await setHarnessToggle(dir, { setting: 'claudeMdExcludes', key: '/r/CLAUDE.md' }, false);
  assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), { model: 'opus', skillOverrides: { other: 'off', lint: 'off' }, enabledPlugins: { 'p@m': false }, disableAllHooks: true, claudeMdExcludes: ['/r/CLAUDE.md'] });
  const h = await harness(dir, []);
  assert.equal(h.items[0]?.toggle?.enabled, false);
  assert.equal(h.kindToggles.hook?.enabled, false);
  await setHarnessToggle(dir, { setting: 'skillOverrides', key: 'lint' }, true);
  await setHarnessToggle(dir, { setting: 'disableAllHooks' }, true);
  await setHarnessToggle(dir, { setting: 'claudeMdExcludes', key: '/r/CLAUDE.md' }, true);
  assert.deepEqual(JSON.parse(fs.readFileSync(file, 'utf8')), { model: 'opus', skillOverrides: { other: 'off' }, enabledPlugins: { 'p@m': false } });
});
