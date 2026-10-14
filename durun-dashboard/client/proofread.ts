// 文章校正で、文章を読み手に 1 文ずつ見せるための区切りと、読み手が引っかかった語句の位置。

const SEGMENTER = new Intl.Segmenter('ja', { granularity: 'sentence' });
const FENCE = '```';

/**
 * 文章を、読み手に 1 つずつ見せる文に切る。つなぐと、前後の空白を除いた元の文章に戻る。
 * 空白だけの部分は前の文につなげる。コードブロックは 1 行ずつ読ませても意味が無いので、丸ごと 1 つにする。
 */
export const sentencesOf = (text: string): string[] => {
  const out: string[] = [];
  let inFence = false;
  for (const { segment } of SEGMENTER.segment(text.trim())) {
    if (out.length && (inFence || !segment.trim())) out[out.length - 1] += segment;
    else out.push(segment);
    if (segment.trimStart().startsWith(FENCE)) inFence = !inFence;
  }
  return out;
};

/** 文の切れ端。question があれば、読み手が引っかかった語句。 */
export type Piece = { text: string; question?: string };

/** 文を、読み手が引っかかった語句とそれ以外に分ける。文に見つからない語句と、前の語句に重なる語句には印を付けない。 */
export const piecesOf = (sentence: string, questions: { quote: string; question: string }[]): Piece[] => {
  const found = questions.map((q) => ({ ...q, at: sentence.indexOf(q.quote) })).filter((q) => q.at >= 0).sort((a, b) => a.at - b.at);
  const out: Piece[] = [];
  let pos = 0;
  for (const q of found) {
    if (q.at < pos) continue;
    if (q.at > pos) out.push({ text: sentence.slice(pos, q.at) });
    out.push({ text: q.quote, question: q.question });
    pos = q.at + q.quote.length;
  }
  if (pos < sentence.length) out.push({ text: sentence.slice(pos) });
  return out;
};
