import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isAllowed } from '../server/access.ts';

const TAILNET = ['core.tail1234.ts.net', 'core', '100.88.0.1', '[fd7a:115c:a1e0::1]'];

test('手元の名前は、ダッシュボードのポートで届いたものだけを受ける', () => {
  assert.equal(isAllowed('localhost:7337', undefined, 7337, []), true);
  assert.equal(isAllowed('127.0.0.1:7337', 'http://127.0.0.1:7337', 7337, []), true);
  assert.equal(isAllowed('localhost:8000', undefined, 7337, []), false);
  assert.equal(isAllowed('localhost', undefined, 7337, []), false);
  assert.equal(isAllowed(undefined, undefined, 7337, []), false);
  // DNS リバインディングでは、攻撃者の名前が Host に入る。
  assert.equal(isAllowed('evil.example:7337', undefined, 7337, TAILNET), false);
});

test('tailnet での名前と IP は、tailscale serve が出すポートを問わずに受け、ほかの端末の名前は受けない', () => {
  assert.equal(isAllowed('core:7337', undefined, 7337, TAILNET), true);
  assert.equal(isAllowed('Core.tail1234.ts.net', 'https://core.tail1234.ts.net', 7337, TAILNET), true);
  assert.equal(isAllowed('100.88.0.1:8443', 'http://100.88.0.1:8443', 7337, TAILNET), true);
  assert.equal(isAllowed('[fd7a:115c:a1e0::1]:7337', undefined, 7337, TAILNET), true);
  assert.equal(isAllowed('other.tail1234.ts.net:7337', undefined, 7337, TAILNET), false);
  assert.equal(isAllowed('core:7337', undefined, 7337, []), false);
});

test('Origin が Host と違う相手なら、他サイトからの書き込みとして断る', () => {
  assert.equal(isAllowed('core:7337', 'https://evil.example', 7337, TAILNET), false);
  assert.equal(isAllowed('core:7337', 'http://core:8765', 7337, TAILNET), false);
  assert.equal(isAllowed('localhost:7337', 'http://core:7337', 7337, TAILNET), false);
  assert.equal(isAllowed('localhost:7337', 'null', 7337, TAILNET), false);
});
