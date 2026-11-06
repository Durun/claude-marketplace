import assert from 'node:assert/strict';
import { test } from 'node:test';
import { tempDir } from './tmp.ts';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
process.env.DURUN_DASHBOARD_DATA_DIR = tempDir('durun-dashboard-reflect-');
const { digestOf, isDue } = await import('../server/reflect.ts');

const HOUR = 60 * 60_000;

test('前回より後の人間の発言を、直前の Claude の発言と組にして並べる', () => {
  const digest = digestOf(
    [
      {
        title: 'CATS の検索',
        cwd: '/repo',
        texts: [
          { kind: 'prompt', text: '検索を直して', at: 1 },
          { kind: 'message', text: '直しました', at: 2 },
          { kind: 'tool', text: 'Edit', at: 3 },
          { kind: 'message', text: '手順をスキルにしました', at: 4 },
          { kind: 'prompt', text: 'そうではない、手順はスキルにしなくていい', at: 5 },
        ],
      },
    ],
    2,
  );
  assert.equal(digest, '## CATS の検索\n作業ディレクトリ: /repo\n\nClaude: 手順をスキルにしました\n人間: そうではない、手順はスキルにしなくていい\n');
});

test('前回より後に人間の発言が無いセッションと、学習の依頼そのものは拾わない', () => {
  const digest = digestOf(
    [
      { title: '古い', cwd: '/a', texts: [{ kind: 'prompt', text: '前の発言', at: 1 }] },
      { title: '学習', cwd: '/b', texts: [{ kind: 'prompt', text: '[durun-dashboard 学習] 人間が会話で教えたことを', at: 10 }] },
    ],
    5,
  );
  assert.equal(digest, '');
});

test('定期実行は、前回から 20 時間たったときだけ学習する', () => {
  assert.equal(isDue(21 * HOUR, 0), true);
  assert.equal(isDue(19 * HOUR, 0), false);
});
