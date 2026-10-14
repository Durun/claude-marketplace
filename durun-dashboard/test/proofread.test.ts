import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';
import { addTerm, applyEvent, emptyRun, findingKey, glossaryEntries, isIgnored, newFile, newVersion, piecesOf, reviseInput, titleOf } from '../shared/proofread.ts';
import type { ProofEvent, ProofFinding, ProofReading, ReviseNote } from '../shared/types.ts';
import { commentFindings, ledgerFinding, priorReadings, proofReport, proofread, readFindings, revise, wordFindings } from '../server/proofread.ts';
import { closuresOf, isFar, numbered, parseProof } from '../server/proofText.ts';

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

test('文章校正は、読み手の答えを文ごとに流し、区切りで Jev に回収を確かめ、文書全体から用語の揺れを探してから終わり、使ったトークン数をモデルごとに数える', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-proofread-'));
  // 読み手には、文の番号ごとに決めた答えを返す。用語の揺れを探す依頼には、組を 1 つ返す。
  const answers = {
    3: reading({ refs: [{ quote: '焼く', antecedent: -2 }] }),
    6: reading({ refs: [{ quote: 'event-receiver', antecedent: -2 }], questions: [{ quote: '配る', question: '配るとは何をすることか', kind: 'definition', wait: true }] }),
  };
  const variants = { groups: [{ terms: ['公開 HTML', 'HTML'], thing: '配る HTML', use: 'HTML' }] };
  const usage = { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 3, cache_creation_input_tokens: 4 };
  useFakeClaude(tmp, `const prompt = stdin();
const k = /<sentence>\\n\\[(\\d+)\\]/.exec(prompt)?.[1];
const out = prompt.startsWith('次の <text> の文章で') ? ${js(variants)} : ${js(answers)}[k] ?? ${js(reading())};
console.log(JSON.stringify({ structured_output: out, usage: ${js(usage)} }));`);
  // Jev には、「焼くとは」まで読んでいれば焼くの疑問が解けたと答えさせる。
  const asked: string[] = [];
  let jevCalls = 0;
  const jev = http.createServer((req, res) => {
    jevCalls++;
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const { state, questions } = JSON.parse(body) as { state: Record<string, string>; questions: Record<string, { instructions: string }> };
      const noul = Object.entries(questions).map(([id, q]) => {
        asked.push(q.instructions);
        return [id, { type: 'noul', noul: q.instructions.includes('焼く') && state['既読の部分']!.includes('焼くとは') ? 0.9 : 0.1 }];
      });
      res.end(JSON.stringify({ answers: Object.fromEntries(noul), usage: { input_tokens: 10, output_tokens: 1 } }));
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
    assert.deepEqual(findingsOf(5), [['参照', 'fail', 'event-receiver'], ['回収', 'fail', '配る']]);
    assert.ok(asked.length > 0 && asked.every((q) => q.includes('`既読の部分` または `用語集`')));
    // sonnet には 6 文の読み手と用語の揺れで 7 回、Jev には区切りごとに問い合わせる。
    assert.deepEqual(events.reduce(applyEvent, emptyRun(DOC)).usage, {
      sonnet: { input: 7, output: 14, cacheRead: 21, cacheCreation: 28 },
      Jev: { input: 10 * jevCalls, output: jevCalls, cacheRead: 0, cacheCreation: 0 },
    });
    assert.equal(events.reduce(applyEvent, { ...emptyRun(DOC), usage: undefined }).usage, undefined);
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
  assert.match(fs.readFileSync(argsFile, 'utf8'), /--model opus --effort medium /);
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
  const commented = { ...newVersion(run.text), run, comments: { [findingKey(0, a)]: { text: '人間のコメント', ignore: false } } };
  const { ver, missing } = commentFindings(commented, [{ n: 2, text: '前の章で定義済み', ignore: true }, { n: 4, text: '無い指摘', ignore: false }]);
  assert.deepEqual(ver.comments, { [findingKey(0, a)]: { text: '人間のコメント', ignore: false }, [findingKey(2, b)]: { text: '前の章で定義済み', ignore: true } });
  assert.deepEqual(missing, [4]);
});

test('用語集は「語：説明」と説明の無い「語」を読み、足した語は末尾の行にする。ファイルの名前は最後の版の最初の行にする', () => {
  assert.deepEqual(glossaryEntries('API：外から呼ぶ口\n\n  PostgreSQL  \nURL: http://a'), [{ term: 'API', note: '外から呼ぶ口' }, { term: 'PostgreSQL', note: '' }, { term: 'URL', note: 'http://a' }]);
  assert.equal(addTerm('API：外から呼ぶ口\n', '共有リンク', ' メモを見せる URL '), 'API：外から呼ぶ口\n共有リンク：メモを見せる URL');
  assert.equal(addTerm('', 'PostgreSQL', ''), 'PostgreSQL');
  const file = newFile('1', '元の文章');
  assert.equal(titleOf({ ...file, versions: [...file.versions, newVersion('\n## 直した見出し\n本文', 0)] }), '直した見出し');
});
