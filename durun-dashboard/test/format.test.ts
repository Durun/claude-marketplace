import assert from 'node:assert/strict';
import { test } from 'node:test';
import { askAnswer, firstLines, fmtSpan, headLines, termSplitter } from '../client/format.ts';

test('headLines は、収まるところまでの行を返し、1 行目が収まらなければ 1 行目を丸ごと返す', () => {
  assert.equal(headLines('短い', 10), '短い');
  assert.equal(headLines('- **一つ目**\n- **二つ目の長い行**', 12), '- **一つ目**');
  assert.equal(headLines('**とても長い 1 行目**\n続き', 5), '**とても長い 1 行目**');
  assert.equal(headLines('改行の無いとても長い文', 5), '改行の無いとても長い文');
});

test('firstLines は、空行を数えずに n 行目までを返し、間の空行は残す', () => {
  assert.equal(firstLines('\n## 背景\n\n関連 Issue: なし\n\n1 行目\n2 行目', 3), '## 背景\n\n関連 Issue: なし\n\n1 行目');
  assert.equal(firstLines('一つ目\r\n\r\n二つ目\r\n三つ目', 2), '一つ目\n\n二つ目');
  assert.equal(firstLines('一つ目\n二つ目\n', 3), '一つ目\n二つ目');
  assert.equal(firstLines('  \n', 3), '');
});

test('askAnswer は、選んだ選択肢を質問ごとの行にし、答えていない質問を省いて、自由記述を最後に添える', () => {
  const questions = [{ question: '添付の値' }, { question: '直す画面' }];
  assert.equal(askAnswer(questions, [['ファイル'], ['受信箱', 'チャット']], ''), '「添付の値」への回答: ファイル\n「直す画面」への回答: 受信箱、チャット');
  assert.equal(askAnswer(questions, [[], ['受信箱']], '添付はおまかせ\n'), '「直す画面」への回答: 受信箱\n\n添付はおまかせ');
  assert.equal(askAnswer(questions, [], '両方おまかせ'), '両方おまかせ');
});

test('termSplitter は、用語集の語を大文字と小文字を区別せずにすべて拾い、長い語から当てる', () => {
  const api = { term: 'API', description: 'ソフトウェア同士をつなぐ取り決め' };
  const claude = { term: 'Claude', description: 'Anthropic の AI' };
  const claudeCode = { term: 'Claude Code', description: 'Claude を端末で使う道具' };
  const cache = { term: 'キャッシュ', description: '読んだ値を覚えておく仕組み' };
  assert.deepEqual(termSplitter([api, claude, claudeCode, cache])('Claude Code の api と Claude のキャッシュ。API'), [
    { text: 'Claude Code', term: claudeCode },
    { text: ' の ' },
    { text: 'api', term: api },
    { text: ' と ' },
    { text: 'Claude', term: claude },
    { text: ' の' },
    { text: 'キャッシュ', term: cache },
    { text: '。' },
    { text: 'API', term: api },
  ]);
  assert.deepEqual(termSplitter([])('Claude'), [{ text: 'Claude' }]);
});

test('termSplitter は、英数字の語の途中にある綴りと、記号を正規表現として読んだときだけ当たる綴りを拾わない', () => {
  const api = { term: 'API', description: 'ソフトウェア同士をつなぐ取り決め' };
  const node = { term: 'Node.js', description: 'JavaScript の実行環境' };
  assert.deepEqual(termSplitter([api, node])('RAPID や APIs や Nodexjs は拾わず、Node.js は拾う'), [
    { text: 'RAPID や APIs や Nodexjs は拾わず、' },
    { text: 'Node.js', term: node },
    { text: ' は拾う' },
  ]);
});

test('fmtSpan は、1 日以上なら日と時間で、1 時間以上なら時間で、それより短ければ分で書き、端数を切り捨てる', () => {
  const minute = 60_000;
  const hour = 60 * minute;
  assert.equal(fmtSpan(7 * 24 * hour + 20 * hour + 59 * minute), '7 日 20 時間');
  assert.equal(fmtSpan(2 * 24 * hour + 30 * minute), '2 日');
  assert.equal(fmtSpan(5 * hour + 59 * minute), '5 時間');
  assert.equal(fmtSpan(45 * minute + 30_000), '45 分');
  assert.equal(fmtSpan(10_000), '1 分');
});
