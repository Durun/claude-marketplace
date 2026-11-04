import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { tempDir } from './tmp.ts';

process.env.DURUN_DASHBOARD_DATA_DIR = tempDir('durun-dashboard-listdir-data-');
const { listDir } = await import('../server/claude.ts');

test('listDir は、フォルダーの更新時刻が変わるまで前に読んだ一覧を返し、変わったら読み直し、無いフォルダーでは空を返す', () => {
  const dir = tempDir('durun-dashboard-listdir-');
  const old = new Date(Date.now() - 60_000);
  fs.writeFileSync(path.join(dir, 'a.json'), '{}');
  fs.utimesSync(dir, old, old);
  assert.deepEqual(listDir(dir), ['a.json']);
  fs.writeFileSync(path.join(dir, 'b.json'), '{}');
  fs.utimesSync(dir, old, old);
  assert.deepEqual(listDir(dir), ['a.json']);
  const older = new Date(old.getTime() - 1000);
  fs.utimesSync(dir, older, older);
  assert.deepEqual([...listDir(dir)].sort(), ['a.json', 'b.json']);
  fs.rmSync(dir, { recursive: true });
  assert.deepEqual(listDir(dir), []);
});
