import assert from 'node:assert/strict';
import { test } from 'node:test';
import { headLines } from '../client/format.ts';

test('headLines は、収まるところまでの行を返し、1 行目が収まらなければ 1 行目を丸ごと返す', () => {
  assert.equal(headLines('短い', 10), '短い');
  assert.equal(headLines('- **一つ目**\n- **二つ目の長い行**', 12), '- **一つ目**');
  assert.equal(headLines('**とても長い 1 行目**\n続き', 5), '**とても長い 1 行目**');
  assert.equal(headLines('改行の無いとても長い文', 5), '改行の無いとても長い文');
});
