import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向け、そこに state.json を置いてからストアを読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-store-'));
process.env.DURUN_DASHBOARD_DATA_DIR = tmp;
const doc = { sessionId: 's1', title: '題', createdAt: 1, seen: true };
fs.writeFileSync(path.join(tmp, 'state.json'), JSON.stringify({ docs: [{ ...doc, id: 'a', format: 'html', content: '<h1>本文</h1>' }, { ...doc, id: 'b', format: 'markdown', path: '/tmp/b.md' }] }));
const { state } = await import('../server/store.ts');

test('本文を state.json に持つ資料は、読み込むときに本文をデータの置き場所の docs へ書き出し、そのファイルを指す資料にする', () => {
  const file = path.join(tmp, 'docs', 'a.html');
  assert.deepEqual(state.docs, [{ ...doc, id: 'a', format: 'html', path: file }, { ...doc, id: 'b', format: 'markdown', path: '/tmp/b.md' }]);
  assert.equal(fs.readFileSync(file, 'utf8'), '<h1>本文</h1>');
});
