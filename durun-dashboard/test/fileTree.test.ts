import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type FileTreeNode, buildFileTree } from '../client/fileTree.ts';

const shape = (nodes: FileTreeNode<string>[]): unknown => nodes.map((n) => (n.kind === 'file' ? n.name : { [n.name]: shape(n.children) }));

test('ディレクトリを入れ子にし、子がディレクトリ 1 つだけの階層は 1 行にまとめ、渡した順を保つ', () => {
  const tree = buildFileTree(['apps/web/src/a/route.ts', 'apps/web/src/b/route.ts', 'apps/web/tests/x.test.ts', 'README.md'], (p) => p);
  assert.deepEqual(shape(tree), [{ 'apps/web': [{ src: [{ a: ['route.ts'] }, { b: ['route.ts'] }] }, { tests: ['x.test.ts'] }] }, 'README.md']);
  const dir = tree[0];
  assert.equal(dir?.kind === 'dir' && dir.path, 'apps/web');
});
