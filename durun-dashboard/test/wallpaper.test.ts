import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { clearWallpaper, readWallpaper, saveWallpaper } from '../server/wallpaper.ts';

const tmpDir = (): string => path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-wallpaper-')), 'data');

test('saveWallpaper は、画像を種類に合う拡張子で置き、URL に替えたら前の画像を外す', () => {
  const dir = tmpDir();
  assert.equal(readWallpaper(dir), undefined);
  saveWallpaper(dir, { type: 'image/png', bytes: Buffer.from('png') });
  assert.deepEqual(readWallpaper(dir), { file: path.join(dir, 'wallpaper.png'), type: 'image/png' });
  saveWallpaper(dir, { type: 'image/webp', bytes: Buffer.from('webp') });
  assert.deepEqual(fs.readdirSync(dir), ['wallpaper.webp']);
  saveWallpaper(dir, { url: 'https://example.com/a.jpg' });
  assert.deepEqual(readWallpaper(dir), { url: 'https://example.com/a.jpg' });
  assert.deepEqual(fs.readdirSync(dir), ['wallpaper.url']);
});

test('clearWallpaper は、画像も URL も外し、何も置いていなくても失敗しない', () => {
  const dir = tmpDir();
  clearWallpaper(dir);
  saveWallpaper(dir, { url: 'https://example.com/a.jpg' });
  clearWallpaper(dir);
  assert.equal(readWallpaper(dir), undefined);
});
