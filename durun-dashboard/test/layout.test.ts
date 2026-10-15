import assert from 'node:assert/strict';
import { test } from 'node:test';
import { COLS, ROWS, addWidget, dropTab, freeApps, fromSaved, keepTabs, mergeInto, moveWidget, resizeRect, resizeWidget } from '../client/layout.ts';
import { uniqueName } from '../client/dashboards.ts';

test('重なったウィジェットを順に下へ押し出し、配列の順と、グリッドに収まる位置と大きさを保つ', () => {
  const layout = [
    { id: 'a', x: 0, y: 0, w: 10, h: 8 },
    { id: 'b', x: 10, y: 0, w: 14, h: 8 },
    { id: 'c', x: 10, y: 8, w: 14, h: 4 },
  ];
  assert.deepEqual(moveWidget(layout, 'a', 10, 0), [
    { id: 'a', x: 10, y: 0, w: 10, h: 8 },
    { id: 'b', x: 10, y: 8, w: 14, h: 8 },
    { id: 'c', x: 10, y: 16, w: 14, h: 4 },
  ]);
  assert.deepEqual(moveWidget(layout, 'b', 6, -4), layout);
  assert.deepEqual(resizeWidget(layout, 'a', COLS, -COLS)[0], { id: 'a', x: 0, y: 0, w: COLS, h: 1 });
  assert.deepEqual(resizeWidget(layout, 'a', -COLS, 0)[0], { id: 'a', x: 0, y: 0, w: 1, h: 8 });
  // 左と上の辺は、反対側の辺を止めたまま動かし、グリッドの左端と先頭の行で止める。
  assert.deepEqual(resizeWidget(layout, 'c', -COLS, -COLS, 'nw')[2], { id: 'c', x: 0, y: 0, w: 24, h: 12 });
  assert.deepEqual(resizeWidget(layout, 'b', 3, 2, 'w')[1], { id: 'b', x: 13, y: 0, w: 11, h: 8 });
  assert.deepEqual(addWidget(layout.slice(1), { id: 'a', x: 0, y: 0, w: COLS, h: 4 }), [
    { id: 'b', x: 10, y: 4, w: 14, h: 8 },
    { id: 'c', x: 10, y: 12, w: 14, h: 4 },
    { id: 'a', x: 0, y: 0, w: COLS, h: 4 },
  ]);
});

test('ウィジェットはグリッドの下端をはみ出させず、押し出した枠が下端をはみ出すなら動かさない', () => {
  const layout = [
    { id: 'a', x: 0, y: 0, w: 10, h: 8 },
    { id: 'b', x: 0, y: 8, w: 10, h: 12 },
  ];
  // 動かすと大きさを変えるのは、下端で止める。
  assert.deepEqual(moveWidget(layout, 'b', 0, ROWS)[1], { id: 'b', x: 0, y: 12, w: 10, h: 12 });
  assert.deepEqual(resizeWidget(layout, 'b', 0, ROWS)[1], { id: 'b', x: 0, y: 8, w: 10, h: 16 });
  // 押し出した b が下端をはみ出すので、a は動かさず、大きさも変えず、足しもしない。
  assert.equal(moveWidget(layout, 'a', 0, 5), layout);
  assert.equal(resizeWidget(layout, 'a', 0, 5), layout);
  assert.equal(addWidget(layout, { id: 'c', x: 0, y: 0, w: 4, h: 6 }), layout);
  assert.deepEqual(moveWidget(layout, 'a', 0, 4), [
    { id: 'a', x: 0, y: 4, w: 10, h: 8 },
    { id: 'b', x: 0, y: 12, w: 10, h: 12 },
  ]);
  // 前からはみ出している枠は、動かさなければそのままにする。
  const over = [...layout, { id: 'c', x: 10, y: 20, w: 4, h: 8 }];
  assert.deepEqual(moveWidget(over, 'a', 4, 0), [{ ...layout[0]!, x: 4 }, layout[1], over[2]]);
});

test('置いた升目に枠があればタブとして重ね、空いていれば新しい枠にする。移したタブは元の枠から外し、空になった枠は消す', () => {
  const main = { id: 'main', x: 0, y: 0, w: 10, h: 8, tabs: [{ id: 'chat' }, { id: 'code', settings: { a: '1' } }] };
  const side = { id: 'side', x: 10, y: 0, w: 4, h: 4, tabs: [{ id: 'usage' }] };
  const layout = [main, side];
  const size = { w: 8, h: 6 };
  assert.deepEqual(dropTab(layout, { id: 'tasks:1' }, 3, 7, size, 'n'), [{ ...main, tabs: [...main.tabs, { id: 'tasks:1' }], active: 'tasks:1' }, side]);
  // 右端では、グリッドに収まる幅にして置く。
  assert.deepEqual(dropTab(layout, { id: 'code', settings: { a: '1' } }, 20, 2, size, 'n'), [
    { ...main, tabs: [{ id: 'chat' }] },
    side,
    { id: 'n', x: 20, y: 2, w: 4, h: 6, tabs: [{ id: 'code', settings: { a: '1' } }] },
  ]);
  // 下端では、グリッドに収まる高さにして置く。
  assert.deepEqual(dropTab(layout, { id: 'tasks:1' }, 0, ROWS - 2, size, 'n')[2], { id: 'n', x: 0, y: ROWS - 2, w: 8, h: 2, tabs: [{ id: 'tasks:1' }] });
  // 押し出した枠が下端をはみ出すなら、元の枠から外さずにそのまま返す。
  const full = [main, { ...side, y: ROWS - 4 }];
  assert.equal(dropTab(full, { id: 'code' }, 10, ROWS - 8, size, 'n'), full);
  assert.deepEqual(dropTab(layout, { id: 'usage' }, 0, 0, size, 'n'), [{ ...main, tabs: [...main.tabs, { id: 'usage' }], active: 'usage' }]);
  assert.deepEqual(keepTabs(layout, (t) => t.id !== 'usage'), [main]);
});

test('free の枠は、ほかの枠と重なっても押し出さず、押し出されず、タブを重ねる先にもならない', () => {
  const main = { id: 'main', x: 0, y: 0, w: 10, h: 8, tabs: [{ id: 'chat' }] };
  const icon = { id: 'icon', x: 12.5, y: 1.25, w: 2, h: 3, tabs: [{ id: 'diagram' }], free: true };
  const layout = [main, icon];
  assert.deepEqual(moveWidget(layout, 'main', 5, 0), [{ ...main, x: 5 }, icon]);
  assert.deepEqual(moveWidget(layout, 'icon', -10, 0), [main, { ...icon, x: 2.5 }]);
  assert.deepEqual(addWidget([main], { ...icon, x: 1, y: 1 }), [main, { ...icon, x: 1, y: 1 }]);
  assert.deepEqual(dropTab(layout, { id: 'tasks' }, 13, 2, { w: 4, h: 4 }, 'n'), [main, icon, { id: 'n', x: 13, y: 2, w: 4, h: 4, tabs: [{ id: 'tasks' }] }]);
});

test('アプリのアイコンだけを入れた枠を free にし、ほかのウィジェットと重ねた枠はそのままにする', () => {
  const isApp = (t: { id: string }) => t.id.startsWith('diagram');
  const main = { id: 'main', x: 0, y: 0, w: 10, h: 8, tabs: [{ id: 'chat' }, { id: 'diagram:1' }] };
  const icon = { id: 'icon', x: 16, y: 0, w: 2, h: 3, tabs: [{ id: 'diagram' }] };
  assert.deepEqual(freeApps([main, icon], isApp), [main, { ...icon, free: true }]);
});

test('枠を別の枠へ重ねると、タブをすべて移して選んでいたタブを選び、元の枠を消す', () => {
  const main = { id: 'main', x: 0, y: 0, w: 10, h: 8, tabs: [{ id: 'chat' }, { id: 'code' }], active: 'code' };
  const side = { id: 'side', x: 10, y: 0, w: 4, h: 4, tabs: [{ id: 'usage' }] };
  assert.deepEqual(mergeInto([main, side], 'main', 'side'), [{ ...side, tabs: [{ id: 'usage' }, { id: 'chat' }, { id: 'code' }], active: 'code' }]);
  assert.deepEqual(mergeInto([main, side], 'side', 'main'), [{ ...main, tabs: [...main.tabs, { id: 'usage' }], active: 'usage' }]);
  assert.deepEqual(mergeInto([main, side], 'main', 'none'), [main, side]);
});

test('前の形で保存した配置は、升目を 2 倍にし、「タブ」のウィジェットを、選んでいた種類を重ねた枠に直す', () => {
  assert.deepEqual(
    fromSaved([
      { id: 'jira:1', x: 0, y: 0, w: 4, h: 6, settings: { jql: 'x' } },
      { id: 'tabs', x: 4, y: 0, w: 8, h: 6, settings: { tabs: 'chat,code', active: 'code', 's:chat': '{"a":"1"}', 's:code': '壊れた JSON' } },
      { id: 'side', x: 0, y: 12, w: 24, h: 2, tabs: [{ id: 'usage' }] },
    ]),
    [
      { id: 'jira:1', x: 0, y: 0, w: 8, h: 12, tabs: [{ id: 'jira:1', settings: { jql: 'x' } }] },
      { id: 'tabs', x: 8, y: 0, w: 16, h: 12, tabs: [{ id: 'chat:tabs', settings: { a: '1' } }, { id: 'code:tabs', settings: {} }], active: 'code:tabs' },
      { id: 'side', x: 0, y: 12, w: 24, h: 2, tabs: [{ id: 'usage' }] },
    ],
  );
});

test('ウィンドウは、つかんだ辺だけを動かし、最小の大きさと画面の上端で止める', () => {
  const r = { x: 100, y: 50, w: 400, h: 300 };
  const min = { w: 240, h: 120 };
  assert.deepEqual(resizeRect(r, 'se', 20, 10, min), { x: 100, y: 50, w: 420, h: 310 });
  assert.deepEqual(resizeRect(r, 'w', -30, 99, min), { x: 70, y: 50, w: 430, h: 300 });
  assert.deepEqual(resizeRect(r, 'w', 300, 0, min), { x: 260, y: 50, w: 240, h: 300 });
  assert.deepEqual(resizeRect(r, 'nw', 10, -80, min), { x: 110, y: 0, w: 390, h: 350 });
});

test('ダッシュボードの名前が使われていれば、(2) から順に番号を付ける', () => {
  assert.equal(uniqueName([], '新しいダッシュボード'), '新しいダッシュボード');
  assert.equal(uniqueName(['新しいダッシュボード', '新しいダッシュボード (2)'], '新しいダッシュボード'), '新しいダッシュボード (3)');
});
