// 補完の候補を、打った語に合う順に並べる。送信欄が持っている候補と、サーバーが探すファイルの候補の両方で使う。

/** text に q の文字がこの順に現れるか。 */
const inOrder = (q: string, text: string): boolean => {
  const chars = [...q];
  let i = 0;
  for (const c of text) if (c === chars[i]) i++;
  return i === chars.length;
};

/** 打った語に、headOf の頭が合うもの、textOf に語を含むもの、textOf に語の文字が順に現れるものの順に並べる。それぞれの中は、渡した順のままにする。大文字と小文字は区別しない。 */
export const rankBy = <T,>(items: T[], word: string, headOf: (item: T) => string, textOf: (item: T) => string): T[] => {
  const q = word.toLowerCase();
  const rank = (item: T): number => {
    const text = textOf(item).toLowerCase();
    if (headOf(item).toLowerCase().startsWith(q)) return 0;
    if (text.includes(q)) return 1;
    return inOrder(q, text) ? 2 : 3;
  };
  return items.map((item) => ({ item, rank: rank(item) })).filter((e) => e.rank < 3).sort((a, b) => a.rank - b.rank).map((e) => e.item);
};
