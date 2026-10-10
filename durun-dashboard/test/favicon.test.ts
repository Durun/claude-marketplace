import assert from 'node:assert/strict';
import { test } from 'node:test';
import { findIcon } from '../server/favicon.ts';

test('findIcon は、rel に icon を含む最初の link の href を、リダイレクトの後のページから解決し、apple-touch-icon は使わない', () => {
  const base = 'https://www.notion.com/';
  assert.equal(findIcon('<link rel="apple-touch-icon" href="/ios.png"><link rel="icon" href="/front-static/favicon.ico"/>', base), 'https://www.notion.com/front-static/favicon.ico');
  assert.equal(findIcon('<link href="https://static.example.com/logo.png" rel="shortcut icon" type="image/png"/>', base), 'https://static.example.com/logo.png');
  assert.equal(findIcon('<LINK REL=icon HREF=/a.png?w=32&amp;h=32>', base), 'https://www.notion.com/a.png?w=32&h=32');
  assert.equal(findIcon('<link rel="stylesheet" href="/style.css"><title>icon</title>', base), undefined);
});
