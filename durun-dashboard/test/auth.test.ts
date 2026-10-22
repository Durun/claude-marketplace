import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態に触れないよう、データの置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-auth-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
const { answerAuth, authProvider, authRequests, requestAuth } = await import('../server/auth.ts');

const until = async (ok: () => boolean): Promise<void> => {
  for (let i = 0; i < 100 && !ok(); i++) await new Promise((r) => setTimeout(r, 50));
};

/** 確かめと認証の回数を数える、偽の相手。 */
const fake = (authed: boolean) => {
  const calls = { check: 0, login: 0 };
  const provider = {
    key: `fake-${Math.random()}`,
    title: '偽物の認証',
    command: 'fake login',
    check: async () => (calls.check++, authed),
    login: async () => void calls.login++,
  };
  return { provider, calls };
};

test('認証済みなら、尋ねずに認証済みと返す', async () => {
  const { provider, calls } = fake(true);
  assert.equal(await requestAuth(provider, 's1'), '偽物の認証は済んでいます。');
  assert.deepEqual(authRequests(), []);
  assert.deepEqual(calls, { check: 1, login: 0 });
});

test('同じ相手の認証を複数のセッションが求めても尋ねるのは 1 回で、OK の答えと認証の結果をすべてに返す', async () => {
  const { provider, calls } = fake(false);
  const results = [requestAuth(provider, 's1'), requestAuth(provider, 's2'), requestAuth(provider, 's1')];
  await until(() => authRequests().length > 0);
  const [request] = authRequests();
  assert.deepEqual(request, { id: request?.id, title: '偽物の認証', command: 'fake login', sessionIds: ['s1', 's2'] });
  answerAuth(request!.id, true);
  assert.deepEqual(await Promise.all(results), Array(3).fill('偽物の認証を終えました。'));
  assert.deepEqual(calls, { check: 1, login: 1 });
  assert.deepEqual(authRequests(), []);
});

test('却下すると、認証せずにすべてに却下を返し、次に求められたらまた尋ねる', async () => {
  const { provider, calls } = fake(false);
  const results = [requestAuth(provider, 's1'), requestAuth(provider, 's2')];
  await until(() => authRequests().length > 0);
  answerAuth(authRequests()[0]!.id, false);
  assert.deepEqual(await Promise.all(results), Array(2).fill('偽物の認証を、人間が却下しました。'));
  assert.equal(calls.login, 0);
  const again = requestAuth(provider, 's1');
  await until(() => authRequests().length > 0);
  answerAuth(authRequests()[0]!.id, true);
  assert.equal(await again, '偽物の認証を終えました。');
});

test('Google Cloud は、確かめのコマンドが成功すれば認証済みとみなし、認証のコマンドが失敗したら出力を添えて失敗にする', async () => {
  process.env.GCLOUD_CHECK_COMMAND = 'exit 1';
  process.env.GCLOUD_LOGIN_COMMAND = 'exit 0';
  const gcloud = authProvider('gcloud', undefined, tmp)!;
  assert.equal(gcloud.command, 'exit 0');
  assert.equal(await gcloud.check(), false);
  await gcloud.login();
  process.env.GCLOUD_CHECK_COMMAND = 'exit 0';
  process.env.GCLOUD_LOGIN_COMMAND = 'echo failed && exit 3';
  assert.equal(await authProvider('gcloud', undefined, tmp)!.check(), true);
  await assert.rejects(authProvider('gcloud', undefined, tmp)!.login(), /failed/);
});

test('MCP サーバーは、claude mcp get の Status で確かめて claude mcp login で認証し、登録されていない名前は失敗にする', async () => {
  const log = path.join(tmp, 'claude-args.log');
  useFakeClaude(tmp, `
    fs.appendFileSync(${js(log)}, args.join(' ') + '\\n');
    const [, verb, name] = args;
    if (name === 'missing') {
      console.log('No MCP server named "missing".');
      process.exitCode = 1;
    } else if (verb === 'get') console.log(name === 'notion' ? '  Status: ! Needs authentication' : '  Status: ✔ Connected');
  `);
  assert.equal(await authProvider('mcp', 'claude.ai Slack', tmp)!.check(), true);
  assert.equal(await authProvider('mcp', 'notion', tmp)!.check(), false);
  await assert.rejects(authProvider('mcp', 'missing', tmp)!.check(), /No MCP server named "missing"/);
  await authProvider('mcp', 'notion', tmp)!.login();
  assert.deepEqual(fs.readFileSync(log, 'utf8').trim().split('\n'), ['mcp get claude.ai Slack', 'mcp get notion', 'mcp get missing', 'mcp login notion']);
  assert.equal(authProvider('mcp', undefined, tmp), undefined);
  assert.equal(authProvider('slack', 'notion', tmp), undefined);
});
