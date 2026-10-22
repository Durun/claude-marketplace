import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import { addTerm, applyEvent, emptyRun, findingKey, glossaryEntries, isIgnored, mergeClaudeWrites, newFile, newVersion, piecesOf, reviseInput, titleOf } from '../shared/proofread.ts';
import type { ProofEvent, ProofFinding, ProofReading, ProofRole, ReviseNote } from '../shared/types.ts';
import { closuresOf, isFar, numbered, parseProof } from '../server/proofText.ts';

// 修正版は入れてあるプラグインを見て書き方を変えるので、実際の Claude Code の設定ではなく、一時ディレクトリを読ませる。
process.env.CLAUDE_CONFIG_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-claude-'));
const { commentFindings, ledgerFinding, outlineFindings, priorReadings, proofReport, proofread, readFindings, revise, summarizeProof, wordFindings } = await import('../server/proofread.ts');

const DOC = `# 方針

決定は次のとおりである。

1. **パス**：公開 HTML が焼く値を決める。焼くとは、HTML に値を埋め込むことである。
2. **配信**：この値は CDN が配る（\`a.b\` の形。末尾は「。」）。
   なお、event-receiver も配る。
`;

test('文章は、見出し、箇条書きの項目、字下げした続きの行を区別して文に切り、コードや括弧の中の句点では切らない', () => {
  const sentences = parseProof(DOC);
  assert.deepEqual(sentences.map((s) => s.text), [
    '# 方針',
    '決定は次のとおりである。',
    '**パス**：公開 HTML が焼く値を決める。',
    '焼くとは、HTML に値を埋め込むことである。',
    '**配信**：この値は CDN が配る（`a.b` の形。末尾は「。」）。',
    'なお、event-receiver も配る。',
  ]);
  const [, intro, path1, define, deliver, aside] = sentences;
  // 同じ行の文は同じ段落、項目の中の続きの行は別の段落で、どちらも項目と最上位のまとまりは同じ。
  assert.equal(path1!.para, define!.para);
  assert.notEqual(deliver!.para, aside!.para);
  assert.equal(deliver!.item, aside!.item);
  assert.notEqual(path1!.top, deliver!.top);
  assert.equal(intro!.item, -1);
  assert.equal(DOC.slice(deliver!.start, deliver!.end), deliver!.text);
  assert.deepEqual(closuresOf(sentences, 2), [{ at: 3, level: '段落' }, { at: 5, level: '節' }]);
  assert.match(numbered(DOC, sentences, 3), /^\[1\] # 方針\n\n\[2\] 決定は次のとおりである。\n\n1\. \[3\] \*\*パス\*\*/);
});

test('指す先までの距離は、同じまとまりの中か直前の 3 文なら近く、それより前の別のまとまりなら遠い', () => {
  const s = (top: number) => ({ text: '', start: 0, end: 0, para: 0, item: -1, top, section: 0 });
  const sentences = [s(1), s(1), s(2), s(2), s(2), s(2), ...Array.from({ length: 10 }, () => s(2))];
  assert.equal(isFar(sentences, 4, 1), false);
  assert.equal(isFar(sentences, 5, 0), true);
  assert.equal(isFar(sentences, 5, 2), false);
  assert.equal(isFar(sentences, 15, 3), true);
});

const reading = (patch: Partial<ProofReading> = {}): ProofReading => ({
  refs: [], questions: [], role: { kind: 'fill', note: '' }, link: { relation: 'addition', marked: true }, premises: [], words: [], attachments: [], ...patch,
});

test('遠い指す先は、同じ語があれば指摘せず、所在の明示があれば注記、手がかりが無ければ失敗にし、読み手の答えから骨組みと連結の注記を作る', () => {
  const sentences = parseProof('1. 凍結バンドルを作る。\n2. 一。\n3. 二。\n4. 三。\n5. 四。その凍結バンドルを配る。この 2 つと決定 1 を確かめる。');
  const found = readFindings(sentences, 6, reading({
    refs: [{ quote: 'この 2 つ', antecedent: 1 }, { quote: '凍結バンドル', antecedent: 1 }, { quote: '決定 1', antecedent: 1 }],
    role: { kind: 'unclear', note: '前とつながらない' },
    link: { relation: 'none', marked: false },
    premises: [{ premise: 'CDN は配信網である', source: 'expert' }],
  }));
  assert.deepEqual(found.map((f) => [f.axis, f.level, f.quote ?? '']), [
    ['近接', 'fail', 'この 2 つ'],
    ['近接', 'note', '決定 1'],
    ['骨組み', 'note', ''],
    ['連結', 'note', ''],
    ['連結', 'note', ''],
  ]);
});

test('曖昧な語は、同じ語を文書で 1 回にまとめて最初に出た文に付け、言い換えの候補を合わせる', () => {
  const found = wordFindings([
    reading({ words: [{ word: '受ける', issue: 'vague', better: ['検知する'] }] }),
    undefined,
    reading({ words: [{ word: '受ける', issue: 'vague', better: ['捕捉する', '検知する'] }, { word: '焼く', issue: 'differs', better: [] }] }),
  ]);
  assert.deepEqual([...found], [
    [0, [{ axis: '語', level: 'note', quote: '受ける', message: 'より狭い意味の語に言い換えられる（ほか 1 か所）', suggestions: ['検知する', '捕捉する'] }]],
    [2, [{ axis: '語', level: 'note', quote: '焼く', message: '用語集や一般の意味と違う意味で使っている', suggestions: [] }]],
  ]);
});

test('疑問は解けた区切りで扱いを変え、既知のように書いた語句は同じ項目の中で分かれば注記、別のまとまりや最後まで分からなければ失敗にする', () => {
  const given = { i: 2, quote: '焼く', question: '「焼く」は何のことか、何を指すか', kind: 'given' } as const;
  assert.equal(ledgerFinding(given, { at: 2, level: '文' }), undefined);
  assert.equal(ledgerFinding(given, { at: 3, level: '段落' })?.level, 'note');
  assert.equal(ledgerFinding(given, { at: 9, level: '節' })?.level, 'fail');
  assert.equal(ledgerFinding(given, undefined)?.level, 'fail');
  const definition = { ...given, kind: 'definition', question: '焼くとは何か' } as const;
  assert.equal(ledgerFinding(definition, { at: 3, level: 'まとまり' }), undefined);
  assert.deepEqual(ledgerFinding(definition, undefined), { axis: '回収', level: 'fail', quote: '焼く', message: '何のことか分からないまま終わる：焼くとは何か' });
  assert.equal(ledgerFinding({ ...given, kind: 'forward' }, { at: 9, level: '文書' })?.axis, '近接');
});

test('指摘の付いた語句は、文の中で最初に現れた所に印を付け、見つからない語句と重なる語句には付けない', () => {
  const m = (quote: string) => ({ quote, note: `${quote}？`, fail: quote === '例の問題' });
  assert.deepEqual(piecesOf('これにより、例の問題は解消する。', [m('例の問題'), m('これ'), m('例の'), m('案B')]), [
    { text: 'これ', mark: m('これ') },
    { text: 'により、' },
    { text: '例の問題', mark: m('例の問題') },
    { text: 'は解消する。' },
  ]);
});

test('文章校正は、読み手の答えを文ごとに流し、区切りで Jev に回収を確かめ、文書全体から用語の揺れと文の役割を調べてから終わり、使ったトークン数をモデルごとに数える', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-proofread-'));
  // 読み手には、文の番号ごとに決めた答えを返す。用語の揺れを探す依頼には組を 1 つ、文の役割を問う依頼には予告と余談を 1 文ずつ含む役割を返す。
  const answers = {
    3: reading({ refs: [{ quote: '焼く', antecedent: -2 }] }),
    6: reading({ refs: [{ quote: 'event-receiver', antecedent: -2 }], questions: [{ quote: '配る', question: '配るとは何をすることか', kind: 'definition', wait: true }] }),
  };
  const variants = { groups: [{ terms: ['公開 HTML', 'HTML'], thing: '配る HTML', use: 'HTML' }] };
  const outline = {
    claim: '公開 HTML が焼く値を決め、CDN が配る',
    sentences: [[0, 'heading'], [0, 'preview'], [0, 'claim'], [3, 'detail'], [3, 'detail'], [5, 'aside']].map(([parent, role], n) => ({ n: n + 1, parent, role })),
  };
  const usage = { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 3, cache_creation_input_tokens: 4 };
  useFakeClaude(tmp, `const prompt = stdin();
const k = /<sentence>\\n\\[(\\d+)\\]/.exec(prompt)?.[1];
const out = prompt.startsWith('次の <text> の文章で') ? ${js(variants)} : prompt.startsWith('<text> の文章を読み、') ? ${js(outline)} : ${js(answers)}[k] ?? ${js(reading())};
console.log(JSON.stringify({ structured_output: out, usage: ${js(usage)} }));`);
  // Jev には、「焼くとは」まで読んでいれば焼くの疑問が解けたと答えさせる。主張との関係は、予告の [2] を確かに、余談の [6] をやや関係しないと答えさせる。
  const asked: string[] = [];
  const judged: Record<string, string>[] = [];
  let jevCalls = 0;
  const jev = http.createServer((req, res) => {
    jevCalls++;
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const { state, questions } = JSON.parse(body) as { state: Record<string, string>; questions: Record<string, { type: string; instructions: string }> };
      if (state['文']) judged.push(state);
      const unrelated = state['文']?.includes('決定は') ? 0.9 : state['文']?.includes('なお') ? 0.6 : 0.05;
      const answer = Object.entries(questions).map(([id, q]) => {
        if (q.type === 'score') return [id, { type: 'score', probabilities: { 0: unrelated, 1: 0, 2: 1 - unrelated, 3: 0 } }];
        if (state['文']) return [id, { type: 'noul', noul: 0.9 }];
        asked.push(q.instructions);
        return [id, { type: 'noul', noul: q.instructions.includes('焼く') && state['既読の部分']!.includes('焼くとは') ? 0.9 : 0.1 }];
      });
      res.end(JSON.stringify({ answers: Object.fromEntries(answer), usage: { input_tokens: 10, output_tokens: 1 } }));
    });
  });
  await new Promise<void>((resolve) => jev.listen(0, '127.0.0.1', resolve));
  process.env.DURUN_DASHBOARD_FAKE_JEV = `http://127.0.0.1:${(jev.address() as { port: number }).port}/`;
  try {
    const events: ProofEvent[] = [];
    const noMorph = () => Promise.reject(new Error('辞書がありません'));
    await proofread(DOC, parseProof(DOC), 'CDN：配信網', (e) => events.push(e), new AbortController().signal, [], noMorph);
    assert.deepEqual(events[0], { type: 'error', message: '形態素解析を使えませんでした（辞書がありません）' });
    assert.deepEqual(events[1], { type: 'plan', sentences: parseProof(DOC).map(({ start, end }) => ({ start, end })), tools: ['sonnet', 'Jev'] });
    assert.equal(events.filter((e) => e.type === 'read').length, 6);
    assert.equal(events.at(-1)?.type, 'done');
    const findingsOf = (i: number) => events.flatMap((e) => ((e.type === 'findings' || e.type === 'read') && e.i === i ? e.findings : [])).map((f) => [f.axis, f.level, f.quote]);
    assert.deepEqual(findingsOf(2).sort(), [['参照', 'note', '焼く'], ['語', 'fail', '公開 HTML']]);
    assert.deepEqual(findingsOf(5).sort(), [['参照', 'fail', 'event-receiver'], ['回収', 'fail', '配る'], ['貢献', 'note', undefined]]);
    assert.deepEqual(findingsOf(1), [['貢献', 'fail', undefined]]);
    assert.ok(asked.length > 0 && asked.every((q) => q.includes('`既読の部分` または `用語集`')));
    // 見出しは主張を支えるかを測らず、前の文にも含めない。
    const run = events.reduce(applyEvent, emptyRun(DOC));
    assert.equal(run.outline?.claim, outline.claim);
    assert.deepEqual(run.outline?.sentences[0], { role: 'heading', parent: 0 });
    assert.deepEqual(run.outline?.sentences[2], { role: 'claim', parent: 0, support: 1.9, unrelated: 0.05, fresh: 0.9 });
    assert.deepEqual(judged.map((s) => s['文']!.slice(0, 3)).sort(), ['[2]', '[3]', '[4]', '[5]', '[6]']);
    assert.equal(judged.find((s) => s['文']!.startsWith('[2]'))?.['前の文'], '（なし）');
    // sonnet には 6 文の読み手と用語の揺れと文の役割で 8 回、Jev には区切りごとと、見出しを除く文ごとに問い合わせる。
    assert.deepEqual(run.usage, {
      sonnet: { input: 8, output: 16, cacheRead: 24, cacheCreation: 32 },
      Jev: { input: 10 * jevCalls, output: jevCalls, cacheRead: 0, cacheCreation: 0 },
    });
    assert.equal(events.reduce(applyEvent, { ...emptyRun(DOC), usage: undefined }).usage, undefined);
  } finally {
    delete process.env.DURUN_DASHBOARD_FAKE_JEV;
    jev.close();
  }
});

test('不要な文は、役割の読み取りと Jev の確率がそろったときだけ指摘し、確率が 0.7 以上なら失敗、0.5 以上なら注記にする', () => {
  const o = (role: ProofRole, parent: number, unrelated: number, fresh: number) => ({ role, parent, support: 1, unrelated, fresh });
  const found = outlineFindings({
    claim: '主張',
    sentences: [
      o('aside', 0, 0.9, 0.9),
      o('preview', 3, 0.6, 0.9),
      o('restate', 2, 0.1, 0.2),
      o('summary', 0, 0.1, 0.4),
      // 役割が要る文としているか、Jev が要る文としていれば指摘しない。測っていない文も指摘しない。
      o('detail', 1, 0.95, 0.05),
      o('aside', 0, 0.3, 0.9),
      o('restate', 2, 0.9, 0.6),
      { role: 'aside', parent: 0 },
      null,
    ],
  }, parseProof('一。二。三。四。五。六。七。八。九。'), []);
  assert.deepEqual([...found].map(([i, list]) => [i, list.map((f) => [f.axis, f.level, f.message])]), [
    [0, [['貢献', 'fail', '主張とも、主張を支える文ともつながらない。消しても主張は伝わる']]],
    [1, [['貢献', 'note', 'この後に書くことを予告するだけで、中身が無い。消しても主張は伝わる']]],
    [2, [['貢献', 'fail', '[2] を言い直しただけで、新しい情報が無い。消しても主張は伝わる']]],
    [3, [['貢献', 'note', '前に書いたことをまとめ直しただけで、新しい情報が無い。消しても主張は伝わる']]],
  ]);
});

test('後の文が指している余談と予告は不要と指摘せず、見出しが 3 つ以上ある文書では予告とまとめを指摘しない', () => {
  const o = (role: ProofRole, unrelated: number, fresh: number) => ({ role, parent: 0, support: 1, unrelated, fresh });
  const removable = [o('aside', 0.9, 0.9), o('restate', 0.1, 0.1), o('preview', 0.9, 0.9), o('summary', 0.1, 0.1), o('claim', 0, 1)];
  // [5] が、[1] の余談と [2] の言い直しを指している。言い直しは、同じ中身が前にあるので指摘する。
  const readings = [undefined, undefined, undefined, undefined, reading({ refs: [{ quote: 'それ', antecedent: 1 }, { quote: 'これ', antecedent: 2 }] })];
  assert.deepEqual([...outlineFindings({ claim: '主張', sentences: removable }, parseProof('一。二。三。四。五。'), readings).keys()], [1, 2, 3]);
  const heading = { role: 'heading', parent: 0 } as const;
  const sectioned = { claim: '主張', sentences: [heading, ...removable, heading, heading] };
  assert.deepEqual([...outlineFindings(sectioned, parseProof('# 甲\n\n一。二。三。四。五。\n\n## 乙\n\n## 丙\n'), []).keys()], [1, 2]);
});

test('要約は、読み手を動かさずに文の役割から寄与度を決め、余談、予告、まとめ、言い直しの文を、Jev も要らないとみれば赤、役割の読み取りだけなら黄にする', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-summarize-'));
  const text = '# 方針\n\n甲は乙である。つまり甲は乙だ。ところで天気が良い。以下で理由を述べる。乙は丙だからである。なお丙は丁でもある。\n';
  const roles = [[0, 'heading'], [0, 'claim'], [2, 'restate'], [0, 'aside'], [0, 'preview'], [2, 'reason'], [6, 'aside']];
  const outline = { claim: '甲は乙である', sentences: roles.map(([parent, role], n) => ({ n: n + 1, parent, role })) };
  const usage = { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 };
  useFakeClaude(tmp, `console.log(JSON.stringify({ structured_output: stdin().startsWith('<text> の文章を読み、') ? ${js(outline)} : {}, usage: ${js(usage)} }));`);
  // Jev は、言い直しには前の文に無い情報がほぼ無い、予告は主張とほぼ関係しない、最初の余談はやや関係する、最後の余談は確かに関係すると答える。
  const odds: Record<string, { unrelated: number; fresh: number }> = { つまり: { unrelated: 0, fresh: 0.1 }, ところで: { unrelated: 0.3, fresh: 1 }, 以下で: { unrelated: 0.9, fresh: 1 }, なお: { unrelated: 0, fresh: 1 } };
  const jev = http.createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const { state, questions } = JSON.parse(body) as { state: Record<string, string>; questions: Record<string, { type: string }> };
      const { unrelated, fresh } = Object.entries(odds).find(([word]) => state['文']!.includes(word))?.[1] ?? { unrelated: 0, fresh: 1 };
      const answer = Object.entries(questions).map(([id, q]) => [id, q.type === 'score' ? { type: 'score', probabilities: { 0: unrelated, 1: 0, 2: 1 - unrelated, 3: 0 } } : { type: 'noul', noul: fresh }]);
      res.end(JSON.stringify({ answers: Object.fromEntries(answer), usage: { input_tokens: 10, output_tokens: 1 } }));
    });
  });
  await new Promise<void>((resolve) => jev.listen(0, '127.0.0.1', resolve));
  process.env.DURUN_DASHBOARD_FAKE_JEV = `http://127.0.0.1:${(jev.address() as { port: number }).port}/`;
  try {
    const events: ProofEvent[] = [];
    await summarizeProof(text, parseProof(text), '', (e) => events.push(e), new AbortController().signal);
    assert.deepEqual(events[0], { type: 'plan', sentences: parseProof(text).map(({ start, end }) => ({ start, end })), tools: ['sonnet', 'Jev'] });
    assert.equal(events.at(-1)?.type, 'done');
    const run = events.reduce(applyEvent, emptyRun(text));
    // sonnet には文の役割だけを 1 回問い合わせる。
    assert.equal(run.usage?.sonnet?.input, 1);
    assert.deepEqual(run.outline?.sentences.map((o) => o?.contribution?.toFixed(1)), [undefined, '3.0', '0.1', '0.7', '0.1', '2.0', '1.0']);
    assert.deepEqual(run.findings.map((list) => list.map((f) => [f.axis, f.level, f.message])), [
      [],
      [],
      [['貢献', 'fail', '[2] を言い直しただけで、新しい情報が無い。消しても主張は伝わる']],
      [['貢献', 'note', '主張とも、主張を支える文ともつながらない。消しても主張は伝わる']],
      [['貢献', 'fail', 'この後に書くことを予告するだけで、中身が無い。消しても主張は伝わる']],
      [],
      [],
    ]);
    // 役割を読めなければ、終わったことにせず、やり直せるようにする。
    useFakeClaude(tmp, 'console.log(JSON.stringify({ structured_output: {} }));');
    const failed: ProofEvent[] = [];
    await summarizeProof(text, parseProof(text), '', (e) => failed.push(e), new AbortController().signal);
    assert.deepEqual(failed.slice(1), [{ type: 'error', message: '文の役割を調べられませんでした（文章の主張が返りませんでした）' }]);
  } finally {
    delete process.env.DURUN_DASHBOARD_FAKE_JEV;
    jev.close();
  }
});

test('続きから校正すると、前に読んだ文は読み手に読ませ直さず、前の答えから指摘を作り直す。形の崩れた答えの文は読み直す', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-proofread-'));
  const asked = path.join(tmp, 'asked');
  useFakeClaude(tmp, `const prompt = stdin();
const k = /<sentence>\\n\\[(\\d+)\\]/.exec(prompt)?.[1];
if (k) fs.appendFileSync(${js(asked)}, k + '\\n');
console.log(JSON.stringify({ structured_output: k ? ${js(reading())} : { groups: [] } }));`);
  const sentences = parseProof(DOC);
  const unlinked = reading({ link: { relation: 'none', marked: false } });
  const prior = priorReadings([unlinked, unlinked, { role: {} }, unlinked, null, null], sentences.length);
  assert.deepEqual(priorReadings([unlinked], sentences.length), []);
  const events: ProofEvent[] = [];
  await proofread(DOC, sentences, '', (e) => events.push(e), new AbortController().signal, prior, () => Promise.reject(new Error('辞書がありません')));
  assert.deepEqual(fs.readFileSync(asked, 'utf8').trim().split('\n').sort(), ['3', '5', '6']);
  const reads = events.flatMap((e) => (e.type === 'read' ? [e] : []));
  assert.equal(reads.length, 6);
  assert.deepEqual(reads.find((e) => e.i === 1)?.findings, [{ axis: '連結', level: 'note', message: '前の文との関係が読み取れない' }]);
  assert.equal(events.at(-1)?.type, 'done');
});

test('修正版は、指摘と人間のコメントを渡して文ごとに opus に書き直させ、固定した文と、文と文のあいだの記号は元のまま残す', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-revise-'));
  const promptFile = path.join(tmp, 'prompt');
  const argsFile = path.join(tmp, 'args');
  // 書き手は、固定した [2] まで書き直そうとする。
  const edits = { edits: [{ n: 2, text: '決めたことは次の 2 つである。' }, { n: 3, text: '**パス**：公開 HTML に埋め込む値を決める。' }, { n: 4, text: '' }] };
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin());\nfs.writeFileSync(${js(argsFile)}, args.join(' '));\nconsole.log(JSON.stringify({ structured_output: ${js(edits)} }));`);
  const notes: ReviseNote[] = [{ i: 2, axis: '参照', level: 'fail', quote: '焼く', message: '指す先が見つからない', comment: '埋め込むと言い換える' }];
  const text = await revise(DOC, parseProof(DOC), 'CDN：配信網', new Set([1]), notes);
  assert.equal(text, DOC.replace('**パス**：公開 HTML が焼く値を決める。焼くとは、HTML に値を埋め込むことである。', '**パス**：公開 HTML に埋め込む値を決める。'));
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.ok(prompt.includes('\n\n[2]（固定） 決定は次のとおりである。\n\n1. [3] **パス**'));
  assert.ok(prompt.includes('[3] 参照（読み進めを妨げる）：「焼く」指す先が見つからない\n  人間のコメント：埋め込むと言い換える'));
  assert.ok(prompt.includes('<glossary>\nCDN：配信網\n</glossary>'));
  assert.match(fs.readFileSync(argsFile, 'utf8'), /--safe-mode --tools  --model opus --effort medium /);
});

test('要約の修正版は、説明を足させず、寄与度の低い文を消すか前後の文にまとめて短くさせる', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-revise-'));
  const promptFile = path.join(tmp, 'prompt');
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin());\nconsole.log(JSON.stringify({ structured_output: { edits: [{ n: 4, text: '' }] } }));`);
  const notes: ReviseNote[] = [{ i: 3, axis: '貢献', level: 'note', message: '主張とも、主張を支える文ともつながらない。消しても主張は伝わる' }];
  const text = await revise(DOC, parseProof(DOC), '', new Set(), notes, 'summarize');
  assert.equal(text, DOC.replace('焼くとは、HTML に値を埋め込むことである。', ''));
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.ok(prompt.includes('指摘を受けた文を消すか前後の文にまとめて、文章を短くしてください。'));
  assert.ok(!prompt.includes('説明の文を足したりしてよい'));
  assert.ok(prompt.includes('[4] 貢献（寄与度がやや低い）：主張とも、主張を支える文ともつながらない。消しても主張は伝わる'));
});

test('yomiyasu が入っていれば、修正版はそのスキルを読ませた opus に、指摘の無い文も推敲させる', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-yomiyasu-'));
  const plugin = path.join(tmp, 'yomiyasu');
  fs.mkdirSync(plugin);
  const installed = path.join(process.env.CLAUDE_CONFIG_DIR!, 'plugins', 'installed_plugins.json');
  fs.mkdirSync(path.dirname(installed), { recursive: true });
  fs.writeFileSync(installed, JSON.stringify({ plugins: { 'yomiyasu@yomiyasu': [{ scope: 'user', installPath: plugin }] } }));
  const promptFile = path.join(tmp, 'prompt');
  const argsFile = path.join(tmp, 'args');
  const cwdFile = path.join(tmp, 'cwd');
  useFakeClaude(tmp, `fs.writeFileSync(${js(promptFile)}, stdin());\nfs.writeFileSync(${js(argsFile)}, args.join(' '));\nfs.writeFileSync(${js(cwdFile)}, process.cwd());\nconsole.log(JSON.stringify({ structured_output: { edits: [] } }));`);
  try {
    assert.equal(await revise(DOC, parseProof(DOC), '', new Set(), []), DOC);
  } finally {
    fs.rmSync(installed);
  }
  const prompt = fs.readFileSync(promptFile, 'utf8');
  assert.ok(prompt.startsWith('/yomiyasu:yomiyasu\n\n'));
  assert.ok(prompt.includes('\n- 指摘の無い文も、yomiyasu のスキルに沿って推敲する。\n\n<glossary>'));
  const args = fs.readFileSync(argsFile, 'utf8');
  assert.ok(args.includes(`--setting-sources  --strict-mcp-config --plugin-dir ${plugin} --tools Read `));
  assert.ok(!args.includes('--safe-mode'));
  // スキルが参照するファイルを読めるよう、プラグインの場所で動かす。
  assert.equal(fs.realpathSync(fs.readFileSync(cwdFile, 'utf8')), fs.realpathSync(plugin));
});

test('校正の進み具合を版に積み、固定した文の指摘と「コメントして無視」にした指摘を、数と修正版から外す', () => {
  const plain = (axis: ProofFinding['axis'], quote?: string): ProofFinding => ({ axis, level: 'note', quote, message: `${axis}の指摘` });
  const [a, b, c] = [plain('語', '甲'), plain('参照', '乙'), plain('連結')];
  let run = applyEvent(emptyRun('甲である。乙である。'), { type: 'plan', sentences: [{ start: 0, end: 5 }, { start: 5, end: 10 }], tools: ['sonnet'] });
  run = applyEvent(run, { type: 'read', i: 0, reading: reading(), findings: [a] });
  run = applyEvent(run, { type: 'findings', i: 1, findings: [b, c] });
  assert.deepEqual(run.readings, [reading(), null]);
  const ver = { ...newVersion(run.text), fixed: ['甲である。'], comments: { [findingKey(1, b)]: { text: '前の章で定義済み', ignore: true }, [findingKey(1, c)]: { text: 'だからでつなぐ', ignore: false } } };
  assert.deepEqual([isIgnored(ver, run, 0, a), isIgnored(ver, run, 1, b), isIgnored(ver, run, 1, c)], [true, true, false]);
  assert.deepEqual(reviseInput(ver, run), { fixed: [0], notes: [{ ...c, i: 1, comment: 'だからでつなぐ' }] });
});

test('MCP で校正した Claude には、指摘の付いた文と読めなかった文ごとに番号を振った指摘を返し、コメントはその番号で指した指摘に残す', () => {
  const a: ProofFinding = { axis: '語', level: 'note', quote: '甲', message: '言い換えられる', suggestions: ['甲乙'] };
  const b: ProofFinding = { axis: '参照', level: 'fail', quote: '丙', message: '指す先が書かれていない' };
  const c: ProofFinding = { axis: '連結', level: 'note', message: '前の文との関係が読み取れない' };
  let run = applyEvent(emptyRun('甲である。乙である。丙である。'), { type: 'plan', sentences: [{ start: 0, end: 5 }, { start: 5, end: 10 }, { start: 10, end: 15 }], tools: ['sonnet'] });
  run = applyEvent(run, { type: 'read', i: 0, reading: reading(), findings: [a] });
  run = applyEvent(run, { type: 'error', i: 1, message: '答えが返りませんでした' });
  run = applyEvent(run, { type: 'findings', i: 2, findings: [b, c] });
  run = applyEvent(run, { type: 'error', message: '形態素解析を使えませんでした' });
  assert.equal(proofReport(run), [
    '文章を 3 文に分けて読み、読み進めを妨げる指摘が 1 件、気にしてほしい指摘が 2 件ありました。',
    '形態素解析を使えませんでした',
    '[1] 甲である。\n#1 語（気にしてほしい）：「甲」言い換えられる\n  言い換えの候補：甲乙',
    '[2] 乙である。\n読めませんでした。答えが返りませんでした',
    '[3] 丙である。\n#2 参照（読み進めを妨げる）：「丙」指す先が書かれていない\n#3 連結（気にしてほしい）：前の文との関係が読み取れない',
  ].join('\n\n'));
  // 文の役割を読み終えていれば、貢献の指摘の基準にした主張も返す。
  const outlined = applyEvent(run, { type: 'outline', outline: { claim: '甲は乙である', sentences: [] } });
  assert.equal(proofReport(outlined).split('\n\n')[1], '文章全体の主張は「甲は乙である」と読みました。');
  const commented = { ...newVersion(run.text), run, comments: { [findingKey(0, a)]: { text: '人間のコメント', ignore: false } } };
  const { ver, missing } = commentFindings(commented, [{ n: 2, text: '前の章で定義済み', ignore: true }, { n: 4, text: '無い指摘', ignore: false }]);
  assert.deepEqual(ver.comments, { [findingKey(0, a)]: { text: '人間のコメント', ignore: false }, [findingKey(2, b)]: { text: '前の章で定義済み', ignore: true } });
  assert.deepEqual(missing, [4]);
});

test('Claude がファイルに書き込んだコメントと修正版だけを開いているファイルに重ね、人間がまだ保存していない変更は残す', () => {
  const comment = (text: string) => ({ text, ignore: false });
  const file = newFile('1', '甲である。');
  const base = { ...file, versions: [file.versions[0]!, { ...newVersion('乙である。', 0), comments: { a: comment('人間'), b: comment('取り消す') } }] };
  // 人間は、保存する前に a を書き換えて b を取り消した。Claude は、v2 に c を足し、a を書き換えなかった。
  const cur = { ...base, versions: [base.versions[0]!, { ...base.versions[1]!, comments: { a: comment('書き換えた') } }] };
  const disk = { ...base, versions: [base.versions[0]!, { ...base.versions[1]!, comments: { ...base.versions[1]!.comments, c: { text: 'Claude', ignore: true } } }] };
  const merged = mergeClaudeWrites(cur, base, disk);
  assert.deepEqual(merged.versions[1]!.comments, { a: comment('書き換えた'), c: { text: 'Claude', ignore: true } });
  assert.equal(merged.versions[0], cur.versions[0]);
  assert.equal(mergeClaudeWrites(cur, base, base), cur);
  // Claude は、さらに v2 から v3 を作って足した。
  const revised = mergeClaudeWrites(cur, base, { ...disk, versions: [...disk.versions, newVersion('乙だ。', 1)] });
  assert.deepEqual(revised.versions.map((v) => v.text), ['甲である。', '乙である。', '乙だ。']);
  assert.deepEqual(revised.versions[1]!.comments, merged.versions[1]!.comments);
});

test('用語集は「語：説明」と説明の無い「語」を読み、足した語は末尾の行にする。ファイルの名前は最後の版の最初の行にする', () => {
  assert.deepEqual(glossaryEntries('API：外から呼ぶ口\n\n  PostgreSQL  \nURL: http://a'), [{ term: 'API', note: '外から呼ぶ口' }, { term: 'PostgreSQL', note: '' }, { term: 'URL', note: 'http://a' }]);
  assert.equal(addTerm('API：外から呼ぶ口\n', '共有リンク', ' メモを見せる URL '), 'API：外から呼ぶ口\n共有リンク：メモを見せる URL');
  assert.equal(addTerm('', 'PostgreSQL', ''), 'PostgreSQL');
  const file = newFile('1', '元の文章');
  assert.equal(titleOf({ ...file, versions: [...file.versions, newVersion('\n## 直した見出し\n本文', 0)] }), '直した見出し');
});
