// 文章校正で、指摘の付いた語句が文のどこにあるかを決める。校正の進み具合を版に積み、人間が無視した指摘を見分ける。
import type { ProofComment, ProofEvent, ProofFile, ProofFinding, ProofRun, ProofVersion, ReviseNote, TokenTotals } from './types.ts';

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

export const emptyRun = (text: string): ProofRun => ({ text, sentences: [], tools: [], done: false, readings: [], findings: [], errors: [], notices: [], usage: {} });

const addTokens = (a: TokenTotals | undefined, b: TokenTotals): TokenTotals => ({
  input: (a?.input ?? 0) + b.input,
  output: (a?.output ?? 0) + b.output,
  cacheRead: (a?.cacheRead ?? 0) + b.cacheRead,
  cacheCreation: (a?.cacheCreation ?? 0) + b.cacheCreation,
});

export const applyEvent = (run: ProofRun, e: ProofEvent): ProofRun => {
  const set = <T>(list: T[], i: number, value: T) => Object.assign([...list], { [i]: value });
  switch (e.type) {
    case 'plan':
      return { ...run, sentences: e.sentences, tools: e.tools, readings: e.sentences.map(() => null), errors: e.sentences.map(() => null), findings: e.sentences.map(() => []) };
    case 'read':
      return { ...run, readings: set(run.readings, e.i, e.reading), findings: set(run.findings, e.i, [...(run.findings[e.i] ?? []), ...e.findings]) };
    case 'findings':
      return { ...run, findings: set(run.findings, e.i, [...(run.findings[e.i] ?? []), ...e.findings]) };
    case 'usage':
      // 数えていない校正の続きを読んだ分だけを足すと、校正全体の数に見えてしまうので、数えないままにする。
      return run.usage ? { ...run, usage: { ...run.usage, [e.model]: addTokens(run.usage[e.model], e.usage) } } : run;
    case 'error':
      return e.i === undefined ? { ...run, notices: [...run.notices, e.message] } : { ...run, errors: set(run.errors, e.i, e.message) };
    case 'done':
      return { ...run, done: true };
  }
};

export const newVersion = (text: string, from?: number, fixed: string[] = []): ProofVersion => ({ text, from, fixed, comments: {} });

export const newFile = (id: string, text = '', glossary = ''): ProofFile => ({ id, title: '', glossary, versions: [newVersion(text)], updatedAt: 0 });

/** ファイルの名前は、最後の版の最初の行。見出しの # は外す。 */
export const titleOf = (file: ProofFile): string =>
  file.versions.at(-1)?.text.split('\n').map((line) => line.replace(/^#+\s*/, '').trim()).find(Boolean)?.slice(0, 40) ?? '';

export const sentenceText = (run: ProofRun, i: number): string => run.text.slice(run.sentences[i]?.start ?? 0, run.sentences[i]?.end ?? 0);

/** 指摘を見分ける文字列。続きから校正して指摘を作り直しても同じになるよう、文の番号と中身から作る。 */
export const findingKey = (i: number, f: ProofFinding): string => `${i}\n${f.axis}\n${f.quote ?? ''}\n${f.message}`;

export const isFixed = (ver: ProofVersion, run: ProofRun, i: number): boolean => ver.fixed.includes(sentenceText(run, i));

/** 固定した文の指摘と、「コメントして無視」にした指摘は、数に入れず、修正版でも直さない。 */
export const isIgnored = (ver: ProofVersion, run: ProofRun, i: number, f: ProofFinding): boolean => isFixed(ver, run, i) || !!ver.comments[findingKey(i, f)]?.ignore;

/** 修正版を作るときに渡す、固定した文の番号と、無視していない指摘。 */
export const reviseInput = (ver: ProofVersion, run: ProofRun): { fixed: number[]; notes: ReviseNote[] } => ({
  fixed: run.sentences.flatMap((_, i) => (isFixed(ver, run, i) ? [i] : [])),
  notes: run.findings.flatMap((list, i) => list.filter((f) => !isIgnored(ver, run, i, f)).map((f) => ({ ...f, i, comment: ver.comments[findingKey(i, f)]?.text }))),
});

/**
 * Claude が MCP でファイルに書き込んだコメントを、開いているファイル cur に取り込む。base は cur を最後に読み込んだか保存したときの中身、disk は今のファイルの中身。
 * 人間の書きかけの変更を消さないよう、base から disk で変わったコメントだけを重ねる。重ねるものが無ければ cur をそのまま返す。
 */
export const mergeComments = (cur: ProofFile, base: ProofFile, disk: ProofFile): ProofFile => {
  const same = (a: ProofComment, b?: ProofComment) => a.text === b?.text && a.ignore === b?.ignore;
  const versions = cur.versions.map((ver, k) => {
    const was = base.versions[k]?.comments ?? {};
    const added = Object.entries(disk.versions[k]?.comments ?? {}).filter(([key, c]) => !same(c, was[key]));
    return added.length ? { ...ver, comments: { ...ver.comments, ...Object.fromEntries(added) } } : ver;
  });
  return versions.some((ver, k) => ver !== cur.versions[k]) ? { ...cur, versions } : cur;
};

/** 用語集の 1 行は「語：説明」か、説明の無い「語」。サーバーの読み方と合わせる。 */
export const glossaryEntries = (glossary: string): { term: string; note: string }[] =>
  glossary.split('\n').flatMap((line) => {
    const at = line.search(/[：:]/);
    const term = (at < 0 ? line : line.slice(0, at)).trim();
    return term ? [{ term, note: at < 0 ? '' : line.slice(at + 1).trim() }] : [];
  });

export const addTerm = (glossary: string, term: string, note: string): string =>
  `${glossary.trimEnd()}${glossary.trim() ? '\n' : ''}${note.trim() ? `${term}：${note.trim()}` : term}`;
