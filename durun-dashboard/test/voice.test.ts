import assert from 'node:assert/strict';
import { test } from 'node:test';
import { afterWake, answerOf } from '../client/voice.ts';

test('呼びかけで始まる発言だけを、呼びかけを除いてイルカへの頼みにする', () => {
  assert.equal(afterWake('イルカ、PR を開いて', ['イルカ']), 'PR を開いて');
  // 書き起こしは、かなの種類や空白が呼びかけと違うことがある。
  assert.equal(afterWake('いるか PR を開いて', ['イルカ']), 'PR を開いて');
  assert.equal(afterWake('Hey Dolphin, open', ['hey dolphin']), 'open');
  // 長い呼びかけを先に比べるので、短い呼びかけの続きを頼みと取り違えない。
  assert.equal(afterWake('ねえ イルカ PR', ['イルカ', 'ねえイルカ']), 'PR');
  assert.equal(afterWake('イルカちゃん、開いて', ['イルカ', 'イルカちゃん']), '開いて');
  assert.equal(afterWake('イルカ。', ['イルカ']), '');
  assert.equal(afterWake('あのイルカかわいい', ['イルカ']), undefined);
  assert.equal(afterWake('イル', ['イルカ']), undefined);
});

test('送る指示への答えを、送る、送らない、どちらでもないに分ける', () => {
  for (const yes of ['はい', 'オッケー', '送って。', 'OK']) assert.equal(answerOf(yes), true, yes);
  for (const no of ['やめて', 'キャンセル', 'いいえ', '送らないで']) assert.equal(answerOf(no), false, no);
  assert.equal(answerOf('PR を開いて'), undefined);
});
