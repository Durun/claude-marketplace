// 文章校正で、指摘の付いた語句が文のどこにあるかを決める。

export type Mark = { quote: string; note: string; fail: boolean };

/** 文の切れ端。mark があれば、指摘の付いた語句。 */
export type Piece = { text: string; mark?: Mark };

/** 文を、指摘の付いた語句とそれ以外に分ける。文に見つからない語句と、前の語句に重なる語句には印を付けない。 */
export const piecesOf = (sentence: string, marks: Mark[]): Piece[] => {
  const found = marks.map((m) => ({ m, at: sentence.indexOf(m.quote) })).filter(({ m, at }) => m.quote && at >= 0).sort((a, b) => a.at - b.at);
  const out: Piece[] = [];
  let pos = 0;
  for (const { m, at } of found) {
    if (at < pos) continue;
    if (at > pos) out.push({ text: sentence.slice(pos, at) });
    out.push({ text: m.quote, mark: m });
    pos = at + m.quote.length;
  }
  if (pos < sentence.length) out.push({ text: sentence.slice(pos) });
  return out;
};
