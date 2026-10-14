import assert from 'node:assert/strict';
import http from 'node:http';
import { test } from 'node:test';
import { newBoard, reveal, toggleFlag, visibleRows } from '../client/minesweeper.ts';
import { minesHint } from '../server/minesweeper.ts';
import { type Game, deal, draw, move, toFoundation } from '../client/solitaire.ts';

test('マインスイーパは、最初に開いたマスとその周りに地雷を置かず、空きマスから周りを続けて開く', () => {
  const b = reveal(newBoard(), 40, () => 0);
  assert.notEqual(b.state, 'lost');
  assert.equal(b.cells.filter((c) => c.mine).length, 10);
  for (const i of [30, 31, 32, 39, 40, 41, 48, 49, 50]) assert.equal(b.cells[i]!.mine, false);
  assert.ok(b.cells.filter((c) => c.open).length > 1);
});

test('マインスイーパは、地雷を開くと負け、地雷以外をすべて開くと勝ちになり、旗のマスは開かない', () => {
  const b = newBoard({ w: 2, h: 1, mines: 1 });
  const flagged = toggleFlag(b, 0);
  assert.equal(reveal(flagged, 0), flagged);
  const first = reveal(b, 0, () => 0);
  assert.equal(first.state, 'won');
  assert.equal(reveal(first, 1).state, 'won');
  const lost = reveal({ ...first, state: 'play', cells: [{ mine: false, open: false, flag: false }, { mine: true, open: false, flag: false }] }, 1);
  assert.equal(lost.state, 'lost');
});

test('マインスイーパの見えている盤面は、開いたマスを周りの地雷の数、旗を F、ほかを . で表し、地雷の位置を含めない', () => {
  const cell = (mine: boolean, open = false, flag = false) => ({ mine, open, flag });
  const b = { ...newBoard({ w: 3, h: 2, mines: 2 }), state: 'play' as const, cells: [cell(false, true), cell(true), cell(false), cell(false, true), cell(true, false, true), cell(false, true)] };
  assert.deepEqual(visibleRows(b), ['2..', '2F2']);
});

test('マインスイーパのヒントは、数字だけで決まるマスを規則で決め、残りのマスを Jev に問い、答えの差を広げる', async () => {
  const asked: string[] = [];
  // Jev には、割合の高い数字に接したマスほど、少しだけ高く答えさせる。
  const jev = http.createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const { questions } = JSON.parse(body) as { questions: Record<string, { instructions: string }> };
      const answers = Object.entries(questions).map(([id, q]) => {
        asked.push(q.instructions);
        return [id, { noul: q.instructions.includes('（50%）') ? 0.55 : 0.45 }];
      });
      res.end(JSON.stringify({ answers: Object.fromEntries(answers) }));
    });
  });
  await new Promise<void>((resolve) => jev.listen(0, '127.0.0.1', resolve));
  process.env.DURUN_DASHBOARD_FAKE_JEV = `http://127.0.0.1:${(jev.address() as { port: number }).port}/`;
  try {
    // 左下の 0 の周りは安全と決まる。右の列の 3 マスに、地雷が 1 つある。
    const [a, b, c, d, e, f, g, h, i] = await minesHint(['01.', '01.', '...'], 1);
    assert.deepEqual([a, b, d, e, g, h], [null, null, null, null, 0, 0]);
    assert.equal(asked.length, 3);
    // 2 つの数字に接する上の 2 マスは、1 つにしか接しない右下より高く、差は Jev の答えより広い。
    assert.equal(c, f);
    assert.ok(c! > 1 / 3 && i! < 1 / 3 && c! - i! > 0.3);
    // 地雷が決まれば、Jev に問わない。
    assert.deepEqual(await minesHint(['1.', '11'], 1), [null, 1, null, null]);
    assert.equal(asked.length, 3);
  } finally {
    delete process.env.DURUN_DASHBOARD_FAKE_JEV;
    jev.close();
  }
});

const card = (s: number, r: number, up = true) => ({ s, r, up });
const empty = (): Game => ({ stock: [], waste: [], found: [[], [], [], []], tab: [[], [], [], [], [], [], []] });

test('ソリティアは、7 列に 1〜7 枚を配り、各列の一番上だけを表にする', () => {
  const g = deal();
  assert.deepEqual(g.tab.map((c) => c.length), [1, 2, 3, 4, 5, 6, 7]);
  assert.ok(g.tab.every((c) => c.at(-1)!.up && c.slice(0, -1).every((x) => !x.up)));
  assert.equal(g.stock.length, 24);
});

test('ソリティアの場札は、色違いで 1 つ小さい札だけを重ね、空いた列には K だけを置き、動かした後は裏の札を表に返す', () => {
  const g = empty();
  g.tab[0] = [card(0, 5, false), card(1, 7)];
  g.tab[1] = [card(0, 8)];
  g.tab[2] = [card(2, 8)];
  assert.equal(move(g, 't0', 1, 't2'), undefined);
  const next = move(g, 't0', 1, 't1')!;
  assert.deepEqual(next.tab[1], [card(0, 8), card(1, 7)]);
  assert.deepEqual(next.tab[0], [card(0, 5, true)]);
  assert.equal(move(g, 't1', 0, 't3'), undefined);
  g.tab[4] = [card(2, 13)];
  assert.ok(move(g, 't4', 0, 't3'));
});

test('ソリティアの組札は、A から同じマークを 1 つずつ積み、山札が尽きたらめくった札を山札に戻す', () => {
  const g = empty();
  g.waste = [card(2, 1)];
  const a = toFoundation(g, 'waste')!;
  assert.deepEqual(a.found.find((f) => f.length), [card(2, 1)]);
  a.waste = [card(1, 2)];
  assert.equal(toFoundation(a, 'waste'), undefined);
  a.stock = [card(0, 1, false), card(0, 2, false)];
  const drawn = draw(draw(a));
  assert.equal(drawn.stock.length, 0);
  const back = draw(drawn);
  assert.deepEqual(back.waste, []);
  assert.deepEqual(back.stock.map((c) => [c.r, c.up]), [[1, false], [2, false], [2, false]]);
});
