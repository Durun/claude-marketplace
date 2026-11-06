// 差分のファイルを、GitHub の PR のファイルツリーと同じ形に組む。

export type FileTreeNode<T> = { kind: 'dir'; name: string; path: string; children: FileTreeNode<T>[] } | { kind: 'file'; name: string; item: T };

/**
 * パスをディレクトリごとに入れ子にする。子がディレクトリ 1 つだけのディレクトリは、GitHub と同じく `a/b/c` のように 1 行にまとめる。
 * 並びは渡した順に保ち、差分の本文の並びとそろえる。
 */
export const buildFileTree = <T>(items: T[], pathOf: (item: T) => string): FileTreeNode<T>[] => {
  const root: FileTreeNode<T>[] = [];
  for (const item of items) {
    const parts = pathOf(item).split('/');
    let level = root;
    for (let i = 0; i < parts.length - 1; i++) {
      let dir = level.find((n) => n.kind === 'dir' && n.name === parts[i]);
      if (!dir) {
        dir = { kind: 'dir', name: parts[i] ?? '', path: parts.slice(0, i + 1).join('/'), children: [] };
        level.push(dir);
      }
      level = dir.kind === 'dir' ? dir.children : level;
    }
    level.push({ kind: 'file', name: parts.at(-1) ?? '', item });
  }
  const compact = (n: FileTreeNode<T>): FileTreeNode<T> => {
    if (n.kind === 'file') return n;
    const only = n.children.length === 1 ? n.children[0] : undefined;
    if (only?.kind === 'dir') return compact({ ...only, name: `${n.name}/${only.name}` });
    return { ...n, children: n.children.map(compact) };
  };
  return root.map(compact);
};
