// 文章校正で、文章を文に切り、各文がどの段落、箇条書きの項目、節に属するかを決める。説明が終わるはずの区切りと、指す先までの距離に使う。
// 見出しと箇条書きは、行と字下げから読む。marked は位置を返さず、項目の中で改行した行を 1 つのテキストにまとめてしまうので使わない。

const SEGMENTER = new Intl.Segmenter('ja', { granularity: 'sentence' });
const HEADING = /^#{1,6}\s/;
const LIST_ITEM = /^(\s*)(?:[-*+]|\d+[.)])\s+/;
const FENCE = /^\s*```/;
const CODE_SPAN = /`[^`]*`/g;
const OPEN = /[（(「『]/g;
const CLOSE = /[）)」』]/g;
// 指す先が同じまとまりに無くても、この文数までは直前の話として読める。
const NEAR_SENTENCES = 3;
// 同じまとまりの中でも、この文数より前は遠い。
const FAR_SENTENCES = 10;

/**
 * 読み手に 1 つずつ見せる文。start と end は元の文章での位置で、文の前後の空白と字下げは含まない。
 * para は段落で、箇条書きの項目の中では 1 行を 1 段落とする。item はいちばん内側の項目で、項目の外なら -1。
 * top は最上位のまとまりで、最上位の項目か、項目の外なら節。section は見出しで始まる節で、見出しが無ければ 0。
 */
export type ProofSentence = { text: string; start: number; end: number; para: number; item: number; top: number; section: number };

/** 区切りの種類。疑問がどの区切りで解けたかを示す。 */
export type ClosureLevel = '段落' | '項目' | 'まとまり' | '節' | '文書';

/** 1 行を文に切った位置と長さ。Intl.Segmenter は括弧やかぎ括弧の中の句点でも切るので、閉じるまでを 1 文につなぐ。 */
const sentenceSpans = (line: string): [number, number][] => {
  const out: [number, number][] = [];
  let depth = 0;
  for (const { segment, index } of SEGMENTER.segment(line)) {
    const last = out.at(-1);
    if (last && depth > 0) last[1] += segment.length;
    else out.push([index, segment.length]);
    depth = Math.max(0, depth + (segment.match(OPEN)?.length ?? 0) - (segment.match(CLOSE)?.length ?? 0));
  }
  return out;
};

export const parseProof = (text: string): ProofSentence[] => {
  const out: ProofSentence[] = [];
  let lastId = 0;
  const newId = () => ++lastId;
  let section = 0;
  let para = 0;
  // 項目の外の段落は、空行が来るまで続く。
  let paraOpen = false;
  let items: { indent: number; id: number }[] = [];
  let fence: ProofSentence | undefined;
  let offset = 0;
  for (const line of text.split('\n')) {
    const lineStart = offset;
    offset += line.length + 1;
    if (fence) {
      fence.end = lineStart + line.length;
      fence.text = text.slice(fence.start, fence.end);
      if (FENCE.test(line)) fence = undefined;
      continue;
    }
    if (!line.trim()) {
      paraOpen = false;
      continue;
    }
    const indent = line.length - line.trimStart().length;
    const marker = LIST_ITEM.exec(line);
    if (HEADING.test(line)) {
      section = newId();
      items = [];
      para = newId();
      paraOpen = false;
    } else if (marker) {
      const depth = marker[1]?.length ?? 0;
      while (items.length && items.at(-1)!.indent >= depth) items.pop();
      items.push({ indent: depth, id: newId() });
      para = newId();
    } else if (items.length && indent > items[0]!.indent) {
      // 字下げした行は、それより浅い項目の続き。
      while (items.length && items.at(-1)!.indent >= indent) items.pop();
      para = newId();
    } else {
      items = [];
      if (!paraOpen) para = newId();
      paraOpen = true;
    }
    const place = { para, item: items.at(-1)?.id ?? -1, top: items[0]?.id ?? section, section };
    const add = (start: number, end: number) => out.push({ text: text.slice(start, end), start, end, ...place });
    // 見出しは句点で切らず、行ごと 1 つにする。コードブロックは 1 行ずつ読ませても意味が無いので丸ごと 1 つにする。
    if (HEADING.test(line) || FENCE.test(line)) {
      add(lineStart + indent, lineStart + line.length);
      if (FENCE.test(line)) fence = out.at(-1);
      continue;
    }
    // 箇条書きの記号は文に含めない。「1. 」の後ろで文が切れてしまうため。
    const skip = marker?.[0].length ?? indent;
    const content = line.slice(skip);
    // コードの中の「...」や「.」で文が切れないよう、同じ長さの文字で塞いでから切る。
    const masked = content.replace(CODE_SPAN, (code) => 'x'.repeat(code.length));
    for (const [index, length] of sentenceSpans(masked)) {
      const body = content.slice(index, index + length).trim();
      if (!body) continue;
      const start = lineStart + skip + index + content.slice(index).indexOf(body);
      add(start, start + body.length);
    }
  }
  return out;
};

/** i 番目の文で生じた疑問を確かめる区切り。区切りの最後の文の位置と、区切りの種類を、近い順に返す。 */
export const closuresOf = (sentences: ProofSentence[], i: number): { at: number; level: ClosureLevel }[] => {
  const s = sentences[i];
  if (!s) return [];
  const lastWhere = (same: (t: ProofSentence) => boolean) => sentences.findLastIndex(same);
  const all: { at: number; level: ClosureLevel }[] = [
    { at: lastWhere((t) => t.para === s.para), level: '段落' },
    { at: s.item < 0 ? -1 : lastWhere((t) => t.item === s.item), level: '項目' },
    { at: lastWhere((t) => t.top === s.top), level: 'まとまり' },
    { at: s.section ? lastWhere((t) => t.section === s.section) : -1, level: '節' },
    { at: sentences.length - 1, level: '文書' },
  ];
  // 同じ文で終わる区切りは、近い方の名前で 1 つにする。
  return all.filter((c, n) => c.at >= i && all.findIndex((d) => d.at === c.at) === n);
};

/** from 番目の文の語句が to 番目の文を指すとき、指す先が遠いか。同じまとまりの中か、直前の数文なら近い。 */
export const isFar = (sentences: ProofSentence[], from: number, to: number): boolean => {
  const distance = from - to;
  if (distance > FAR_SENTENCES) return true;
  return distance > NEAR_SENTENCES && sentences[from]?.top !== sentences[to]?.top;
};

/** upto 番目より前の文に [1] からの番号を付けた本文。文と文のあいだの改行や箇条書きの記号は、元の文章のまま残す。tag は番号の後に添える印。 */
export const numbered = (text: string, sentences: ProofSentence[], upto: number, tag = (_n: number) => ''): string => {
  let out = '';
  let pos = 0;
  sentences.slice(0, upto).forEach((s, n) => {
    out += `${text.slice(pos, s.start)}[${n + 1}]${tag(n)} ${s.text}`;
    pos = s.end;
  });
  return out;
};
