import assert from 'node:assert/strict';
import { test } from 'node:test';
import { askAnswer, headLines } from '../client/format.ts';

test('headLines は、収まるところまでの行を返し、1 行目が収まらなければ 1 行目を丸ごと返す', () => {
  assert.equal(headLines('短い', 10), '短い');
  assert.equal(headLines('- **一つ目**\n- **二つ目の長い行**', 12), '- **一つ目**');
  assert.equal(headLines('**とても長い 1 行目**\n続き', 5), '**とても長い 1 行目**');
  assert.equal(headLines('改行の無いとても長い文', 5), '改行の無いとても長い文');
});

test('askAnswer は、選んだ選択肢を質問ごとの行にし、答えていない質問を省いて、自由記述を最後に添える', () => {
  const questions = [{ question: '添付の値' }, { question: '直す画面' }];
  assert.equal(askAnswer(questions, [['ファイル'], ['受信箱', 'チャット']], ''), '「添付の値」への回答: ファイル\n「直す画面」への回答: 受信箱、チャット');
  assert.equal(askAnswer(questions, [[], ['受信箱']], '添付はおまかせ\n'), '「直す画面」への回答: 受信箱\n\n添付はおまかせ');
  assert.equal(askAnswer(questions, [], '両方おまかせ'), '両方おまかせ');
});
