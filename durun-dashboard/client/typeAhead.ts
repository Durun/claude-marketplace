// 一覧にフォーカスがある間に打った文字で、その文字で始まる名前へ移る。Windows のエクスプローラーと同じ動き。

/** 打つ間隔がこれより空いたら、打った文字を捨てて新しく探し始める。 */
export const TYPE_AHEAD_RESET_MS = 1000;

/**
 * names の中で query で始まる名前の位置を返す。大文字と小文字は区別しない。無ければ -1。from は今選んでいる位置。
 * 同じ 1 文字だけを重ねて打つと、その文字で始まる名前を from の次から順に巡り、末尾まで行ったら先頭に戻る。
 */
export const nextMatch = (names: string[], query: string, from: number): number => {
  const q = query.toLowerCase();
  const cycle = [...q].every((c) => c === q[0]);
  const key = cycle ? q.slice(0, 1) : q;
  // 文字を打ち足したときは、今の名前がまだ合うならそこに留まる。
  const start = cycle ? from + 1 : Math.max(from, 0);
  for (let i = 0; i < names.length; i++) {
    const j = (start + i) % names.length;
    if (names[j]?.toLowerCase().startsWith(key)) return j;
  }
  return -1;
};
