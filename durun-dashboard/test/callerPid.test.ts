import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ownerOfConnection } from '../server/callerPid.ts';

const NETSTAT = `
アクティブな接続

  プロトコル  ローカル アドレス      外部アドレス           状態            PID
  TCP         127.0.0.1:7337         0.0.0.0:0              LISTENING       29012
  TCP         127.0.0.1:7337         127.0.0.1:50692        ESTABLISHED     29012
  TCP         127.0.0.1:50692        127.0.0.1:7337         TIME_WAIT       0
  TCP         127.0.0.1:50692        127.0.0.1:7337         ESTABLISHED     33544
  TCP         127.0.0.1:150692       127.0.0.1:7337         ESTABLISHED     11111
  TCP         [::1]:50700            [::1]:7337             ESTABLISHED     22222
`;

test('netstat の行から、クライアント側のポートでサーバーへ張った接続を持つプロセスを引き、閉じた接続とサーバー側の行は使わない', () => {
  assert.equal(ownerOfConnection(NETSTAT, 50692, 7337), 33544);
  assert.equal(ownerOfConnection(NETSTAT, 50700, 7337), 22222);
  assert.equal(ownerOfConnection(NETSTAT, 50999, 7337), undefined);
});
