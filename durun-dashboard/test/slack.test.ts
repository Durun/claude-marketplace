import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// slack.ts は cache.ts を通して、import した時点でデータの置き場所を読む。本物のデータに触れないよう、一時ディレクトリに向けてから読み込む。
process.env.DURUN_DASHBOARD_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-slack-'));
const { parseSlackChannels, parseSlackMessages, slackKeyOf, slackLinksIn, slackTargetOf } = await import('../server/slack.ts');

test('Slack のリンクから、メッセージならそのスレッドを、返信なら thread_ts の親を、チャンネルならチャンネルを読む先にする', () => {
  assert.deepEqual(slackTargetOf('https://example.slack.com/archives/C0AAA/p1700000000123456'), { channel: 'C0AAA', thread: '1700000000.123456' });
  assert.deepEqual(slackTargetOf('https://example.slack.com/archives/C0AAA/p1700000099000001?thread_ts=1700000000.123456&cid=C0AAA'), { channel: 'C0AAA', thread: '1700000000.123456' });
  assert.deepEqual(slackTargetOf(' https://example.slack.com/archives/C0AAA '), { channel: 'C0AAA' });
  assert.equal(slackTargetOf('https://example.slack.com/team/U0AAA'), undefined);
  assert.equal(slackTargetOf('C0AAA'), undefined);
});

test('チャンネルの ID と、# で始めたチャンネルの名前も読む先にする', () => {
  assert.deepEqual(slackTargetOf('C0AAAAAAA'), { channel: 'C0AAAAAAA' });
  assert.deepEqual(slackTargetOf('#times_example'), { name: 'times_example' });
  assert.equal(slackTargetOf('#'), undefined);
  assert.equal(slackTargetOf('times_example'), undefined);
});

test('同じスレッドを指す親と返信のリンクは同じ鍵になり、チャットに出たリンクは同じ会話を 1 つにまとめる', () => {
  const parent = 'https://example.slack.com/archives/C0AAA/p1700000000123456';
  const reply = 'https://example.slack.com/archives/C0AAA/p1700000099000001?thread_ts=1700000000.123456&cid=C0AAA';
  assert.equal(slackKeyOf(parent), slackKeyOf(reply));
  assert.notEqual(slackKeyOf(parent), slackKeyOf('https://example.slack.com/archives/C0AAA'));
  const texts = [`見て ${parent}。あと（${reply}）`, `[チャンネル](https://example.slack.com/archives/C0BBB) と <${parent}>`];
  assert.deepEqual(slackLinksIn(texts), [parent, 'https://example.slack.com/archives/C0BBB']);
});

test('チャンネルの検索の結果から、名前と ID を読む', () => {
  const text = '## Channels (2 results)\n1. #example (C0AAA) - public_channel - Creator: 山田 - Not Archived\n2. #example-dev (G0BBB) - private_channel';
  assert.deepEqual(parseSlackChannels(text), [{ name: 'example', id: 'C0AAA' }, { name: 'example-dev', id: 'G0BBB' }]);
});

test('チャンネルの応答は、メッセージの見出しの行から発言者を読み、末尾の Thread と Reactions の行を本文から外し、古い順に並べる', () => {
  const text = [
    'Channel: #example (C0AAA)',
    '',
    '=== Message from 山田 <yamada@example.com> (U0AAA) at 2026-11-03 19:38:49 JST === ',
    'Message TS: 1700000200.000002',
    '<@U0BBB|佐藤> 見て &lt;b&gt; <#C0CCC|general> <@U0DDD>',
    '<https://example.com/a|資料>',
    'Reactions: kusa (5)',
    '',
    '=== Message from Slackbot (USLACKBOT) at 2026-11-03 19:00:08 JST === ',
    'Message TS: 1700000100.000001',
    'リマインダー',
    'Thread: 2 replies (latest: 2026-11-03 19:39:50 JST)',
  ].join('\n');
  assert.deepEqual(parseSlackMessages(text), [
    { ts: '1700000100.000001', user: 'Slackbot', text: 'リマインダー', replies: 2 },
    { ts: '1700000200.000002', user: '山田', text: '@佐藤 見て <b> #general @U0DDD\n[資料](https://example.com/a)' },
  ]);
});

test('スレッドの応答は、親と返信の From の行から発言者を読む', () => {
  const text = [
    '=== THREAD PARENT MESSAGE ===',
    'From: Slackbot (USLACKBOT)',
    'Time: 2026-11-03 19:00:08 JST',
    'Message TS: 1700000100.000001',
    'リマインダー',
    '',
    '=== THREAD REPLIES (1 total) ===',
    '',
    '--- Reply 1 of 1 ---',
    'From: 山田 <yamada@example.com> (U0AAA)',
    'Time: 2026-11-03 19:38:49 JST',
    'Message TS: 1700000200.000002',
    '<https://example.com/a|https://example.com/a>',
    '',
  ].join('\n');
  assert.deepEqual(parseSlackMessages(text), [
    { ts: '1700000100.000001', user: 'Slackbot', text: 'リマインダー' },
    { ts: '1700000200.000002', user: '山田', text: 'https://example.com/a' },
  ]);
});
