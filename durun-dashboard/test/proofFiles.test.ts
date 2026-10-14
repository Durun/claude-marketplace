import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-proof-files-'));
process.env.DURUN_DASHBOARD_DATA_DIR = tmp;
const { readProofFile, recentProofFiles, writeProofFile } = await import('../server/proofFiles.ts');
const { newFile } = await import('../client/proofread.ts');

test('文章校正のファイルは 1 件を 1 つの JSON に保存し、最近使ったファイルは最後に保存した順に並べる', () => {
  assert.deepEqual(recentProofFiles(), []);
  assert.equal(readProofFile('a'), null);
  const a = { ...newFile('a', '甲'), title: '甲', updatedAt: 1 };
  writeProofFile(a);
  writeProofFile({ ...newFile('b', '乙'), title: '乙', updatedAt: 2 });
  // 保存した時刻の順を確かめるため、先に保存したほうを古くする。
  fs.utimesSync(path.join(tmp, 'proofread', 'a.json'), new Date(0), new Date(0));
  assert.deepEqual(recentProofFiles(), [{ id: 'b', title: '乙', updatedAt: 2 }, { id: 'a', title: '甲', updatedAt: 1 }]);
  assert.deepEqual(readProofFile('a'), JSON.parse(JSON.stringify(a)));
  assert.deepEqual(fs.readdirSync(path.join(tmp, 'proofread')).sort(), ['a.json', 'b.json']);
});
