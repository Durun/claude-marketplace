import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { baseName, toggled } from '../client/explorerFavorites.ts';
import { fmtBytes } from '../client/format.ts';
import { listDir, saveText } from '../server/explorer.ts';
import { platformFor } from '../server/platform.ts';

test('listDir は、フォルダーを先に名前の順で並べ、ファイルには大きさを付け、1 つ上のフォルダーと最上位の場所を添える', (t) => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'explorer-'));
  t.after(() => fs.rmSync(tmp, { recursive: true, force: true }));
  fs.mkdirSync(path.join(tmp, 'src'));
  fs.mkdirSync(path.join(tmp, 'docs'));
  fs.writeFileSync(path.join(tmp, 'b10.txt'), 'hello');
  fs.writeFileSync(path.join(tmp, 'b2.txt'), '');
  // 区切りを / にそろえる OS でも、そろえない OS でも、同じ形で比べる。
  const win = platformFor('win32');
  const dir = listDir(tmp, { ...win, fileRoots: () => ['C:/', 'D:/'] });
  const shown = win.displayPath(tmp);
  assert.equal(dir.path, shown);
  assert.equal(dir.parent, win.displayPath(path.dirname(tmp)));
  assert.equal(dir.home, win.displayPath(os.homedir()));
  assert.deepEqual(dir.roots, ['C:/', 'D:/']);
  assert.equal(dir.truncated, false);
  assert.deepEqual(
    dir.entries.map((e) => [e.name, e.path, e.dir, e.size]),
    [
      ['docs', `${shown}/docs`, true, undefined],
      ['src', `${shown}/src`, true, undefined],
      ['b2.txt', `${shown}/b2.txt`, false, 0],
      ['b10.txt', `${shown}/b10.txt`, false, 5],
    ],
  );
  assert.ok(dir.entries.every((e) => typeof e.modifiedAt === 'number'));
});

test('listDir は、省くとホームディレクトリを読み、最上位では 1 つ上を返さず、無いフォルダーでは例外を投げる', () => {
  const p = platformFor('darwin');
  assert.equal(listDir(undefined, p).path, listDir('~', p).path);
  assert.equal(listDir(path.parse(os.homedir()).root, p).parent, undefined);
  assert.throws(() => listDir(path.join(os.tmpdir(), 'explorer-no-such-dir'), p), /ENOENT/);
});

test('saveText は、読んだときの更新時刻のままなら上書きし、CRLF のファイルは CRLF のまま保つ。ほかで書き換えられていたら上書きしない', (t) => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'notepad-'));
  t.after(() => fs.rmSync(tmp, { recursive: true, force: true }));
  const crlf = path.join(tmp, 'crlf.txt');
  const lf = path.join(tmp, 'lf.txt');
  fs.writeFileSync(crlf, 'a\r\nb');
  fs.writeFileSync(lf, 'a\nb');
  const read = new Date('2020-01-01T00:00:00Z');
  fs.utimesSync(crlf, read, read);
  const saved = saveText(crlf, 'x\ny', read.getTime());
  assert.deepEqual(saved, { modifiedAt: fs.statSync(crlf).mtimeMs });
  assert.equal(fs.readFileSync(crlf, 'utf8'), 'x\r\ny');
  // 上書きした後の更新時刻は、読んだときのものと違うので、読んだときの更新時刻のままでは上書きしない。
  assert.deepEqual(saveText(crlf, 'z', read.getTime()), { modifiedAt: saved.modifiedAt, changed: true });
  assert.equal(fs.readFileSync(crlf, 'utf8'), 'x\r\ny');
  saveText(lf, 'x\ny', fs.statSync(lf).mtimeMs);
  assert.equal(fs.readFileSync(lf, 'utf8'), 'x\ny');
});

test('お気に入りは、登録していなければ末尾に足し、していれば外す。名前には、パスの最後の区切りを出す', () => {
  assert.deepEqual(toggled(['C:/a'], 'C:/b'), ['C:/a', 'C:/b']);
  assert.deepEqual(toggled(['C:/a', 'C:/b'], 'C:/a'), ['C:/b']);
  assert.equal(baseName('C:/Users/me/dev'), 'dev');
  assert.equal(baseName('/home/me/dev/'), 'dev');
  assert.equal(baseName('C:/'), 'C:');
  assert.equal(baseName('/'), '/');
});

test('fmtBytes は、1024 ごとに単位を上げ、KB から上は小数 1 桁までにする', () => {
  assert.equal(fmtBytes(0), '0 B');
  assert.equal(fmtBytes(1023), '1023 B');
  assert.equal(fmtBytes(1024), '1 KB');
  assert.equal(fmtBytes(1536), '1.5 KB');
  assert.equal(fmtBytes(5 * 1024 ** 3), '5 GB');
});
