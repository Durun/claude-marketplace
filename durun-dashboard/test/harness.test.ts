import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { frontmatterOf, harnessItems } from '../server/harness.ts';
import { tempDir } from './tmp.ts';

test('frontmatter の 1 行の値と、> で続ける複数行の値を読む', () => {
  assert.deepEqual(frontmatterOf('---\nname: lint\ndescription: >\n  lint を直す\n  ときに使う\n---\n本文'), { name: 'lint', description: 'lint を直す ときに使う' });
  assert.deepEqual(frontmatterOf('本文だけ'), {});
});

test('CLAUDE.md・スキル・記憶・フックを、いつ効くかとともに並べる', () => {
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
  const items = harnessItems(dir, [os.tmpdir()]).map(({ kind, name, trigger }) => ({ kind, name, trigger }));
  assert.deepEqual(items, [
    { kind: 'instructions', name: 'CLAUDE.md', trigger: 'いつも' },
    { kind: 'skill', name: 'lint', trigger: 'lint を直すときに使う' },
    { kind: 'memory', name: '記憶の索引', trigger: 'このプロジェクトでいつも' },
    { kind: 'memory', name: 'a', trigger: '手順はスキルにしない' },
    { kind: 'hook', name: 'guard.sh', trigger: 'PreToolUse（Bash）' },
  ]);
});
