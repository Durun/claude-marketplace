// 文章を前から 1 文ずつ、前提知識を持たない読み手に読ませ、7 つの軸で読み進めやすさを示す「文章校正」。メモ帳と同じく、スキンによらず XP の見た目で描く。
// 文章と用語集はウィンドウの設定に持ち、閉じるまで残す。読み手の答えは、編集に戻るか閉じると捨てる。
import { Fragment } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { ProofAxis, ProofEvent, ProofFinding, ProofReading } from '../shared/types.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { piecesOf } from './proofread.ts';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「proofread:開いた時刻」にし、いくつも開けるようにする。 */
export const PROOFREAD_KEY = 'proofread';
export const PROOFREAD_TITLE = '文章校正';
export const PROOFREAD_ICON = '/proofread.svg';
const SIZE = { w: 960, h: 640 };

export const openProofread = (): void => floatUp(PROOFREAD_KEY, '', { id: `${PROOFREAD_KEY}:${Date.now()}` }, SIZE);

export const ProofreadIcon = () => <DesktopIcon icon={PROOFREAD_ICON} title={PROOFREAD_TITLE} hint="初めて読む人が、文章を前から読み進められるかを調べます。ダブルクリックで開きます" onLaunch={openProofread} />;

const AXES: { axis: ProofAxis; hint: string }[] = [
  { axis: '参照', hint: '既知のように書いた語句の指す先が、そこまでの本文か用語集で分かるか' },
  { axis: '回収', hint: '読み手が抱いた疑問が、説明の区切りまでに解けるか' },
  { axis: '近接', hint: '指す先が近くにあるか。係り方が 1 通りに読めるか' },
  { axis: '骨組み', hint: '文が、先に示された枠に入るか、枠を示すか' },
  { axis: '連結', hint: '前の文との関係が分かるか。つなぐのに専門知識が要らないか' },
  { axis: '語', hint: '曖昧な一般語、同じものの呼び分け、用語集と違う意味の語が無いか' },
  { axis: '負荷', hint: '1 文の長さ、初めて出る語や述語の数、抱えている未解決の疑問の数' },
];
const ROLE: Record<ProofReading['role']['kind'], string> = { frame: '枠を示す', fill: '枠に入る', shift: '話題を変える', unclear: '分からない' };
const RELATION: Record<ProofReading['link']['relation'], string> = {
  cause: '理由', result: '結果', example: '例', contrast: '対比', addition: '補足', restate: '言い換え', shift: '転換', none: '読み取れない', first: '最初の文',
};
const QUESTION: Record<ProofReading['questions'][number]['kind'], string> = { definition: '何のことか', detail: '中身', reason: '理由', background: '背景', prediction: 'この先' };
const antecedentOf = (n: number): string => (n > 0 ? `[${n}]` : n === 0 ? '用語集' : n === -1 ? '一般常識' : n === -3 ? 'この後' : '見つからない');

/** 校正の 1 回分。text は校正を始めた時点の文章で、文の位置はこれを指す。error は文ごとの失敗で、notices は全体の失敗。 */
type Run = {
  text: string; sentences: { start: number; end: number }[]; tools: string[]; done: boolean;
  readings: (ProofReading | undefined)[]; findings: ProofFinding[][]; error: (string | undefined)[]; notices: string[];
};

const applyEvent = (run: Run, e: ProofEvent): Run => {
  const set = <T,>(list: T[], i: number, value: T) => Object.assign([...list], { [i]: value });
  switch (e.type) {
    case 'plan':
      return { ...run, sentences: e.sentences, tools: e.tools, findings: e.sentences.map(() => []) };
    case 'read':
      return { ...run, readings: set(run.readings, e.i, e.reading), findings: set(run.findings, e.i, [...(run.findings[e.i] ?? []), ...e.findings]) };
    case 'findings':
      return { ...run, findings: set(run.findings, e.i, [...(run.findings[e.i] ?? []), ...e.findings]) };
    case 'error':
      return e.i === undefined ? { ...run, notices: [...run.notices, e.message] } : { ...run, error: set(run.error, e.i, e.message) };
    case 'done':
      return { ...run, done: true };
  }
};

/** 文に付ける印。見せている軸の指摘のうち、いちばん重いもの。 */
const levelOf = (findings: ProofFinding[]): 'fail' | 'note' | 'ok' =>
  findings.some((f) => f.level === 'fail') ? 'fail' : findings.length ? 'note' : 'ok';

const Detail = ({ run, i }: { run: Run; i: number }) => {
  const s = run.sentences[i];
  const reading = run.readings[i];
  const findings = run.findings[i] ?? [];
  if (!s) return <aside class="proof-detail" />;
  return (
    <aside class="proof-detail">
      <blockquote>[{i + 1}] {run.text.slice(s.start, s.end)}</blockquote>
      {run.error[i] && <p>読めませんでした。{run.error[i]}</p>}
      {!reading && !run.error[i] && <p>まだ読んでいません。</p>}
      {reading && !findings.length && <p>指摘はありません。</p>}
      <ul class="proof-findings">
        {AXES.flatMap(({ axis }) => findings.filter((f) => f.axis === axis)).map((f, n) => (
          <li key={n} data-level={f.level}>
            <b>{f.level === 'fail' ? '×' : '△'} {f.axis}</b>
            {f.quote && <q>{f.quote}</q>}
            <span>{f.message}</span>
            {!!f.suggestions?.length && <span class="proof-better">→ {f.suggestions.join('、')}</span>}
          </li>
        ))}
      </ul>
      {reading && (
        <details>
          <summary>読み手のメモ</summary>
          <dl>
            <dt>役割</dt>
            <dd>{ROLE[reading.role.kind]}。{reading.role.note}</dd>
            <dt>前の文との関係</dt>
            <dd>{RELATION[reading.link.relation]}{reading.link.marked ? '（文面に示されている）' : ''}</dd>
            {reading.refs.length > 0 && <dt>指す先</dt>}
            {reading.refs.map((r, n) => <dd key={n}>「{r.quote}」→ {antecedentOf(r.antecedent)}</dd>)}
            {reading.questions.length > 0 && <dt>疑問</dt>}
            {reading.questions.map((q, n) => <dd key={n}>（{QUESTION[q.kind]}）{q.question}</dd>)}
            {reading.premises.length > 0 && <dt>補った前提</dt>}
            {reading.premises.map((p, n) => <dd key={n}>{p.premise}{p.source === 'expert' ? '（専門知識）' : ''}</dd>)}
          </dl>
        </details>
      )}
    </aside>
  );
};

export const ProofreadFloat = ({ f }: { f: Float }) => {
  const text = f.tab.settings?.text ?? '';
  const glossary = f.tab.settings?.glossary ?? '';
  const save = (patch: Record<string, string>) => setFloatSettings(f, { ...f.tab.settings, ...patch });
  const [run, setRun] = useState<Run>();
  const [picked, setPicked] = useState(0);
  // 見せる軸。空なら全部を見せる。
  const [only, setOnly] = useState<ProofAxis>();
  const stop = useRef<AbortController | null>(null);
  useEffect(() => () => stop.current?.abort(), []);

  const start = () => {
    const ctrl = new AbortController();
    stop.current = ctrl;
    setRun({ text, sentences: [], tools: [], done: false, readings: [], findings: [], error: [], notices: [] });
    setPicked(0);
    setOnly(undefined);
    api.proofread(text, glossary, (e) => setRun((r) => r && applyEvent(r, e)), ctrl.signal).catch((e: Error) => {
      if (!ctrl.signal.aborted) setRun((r) => r && { ...r, done: true, notices: [...r.notices, e.message] });
    });
  };
  const edit = () => {
    stop.current?.abort();
    setRun(undefined);
  };

  const shown = (i: number) => (run?.findings[i] ?? []).filter((x) => !only || x.axis === only);
  const read = run?.readings.filter(Boolean).length ?? 0;
  const status = !run ? '' : !run.sentences.length ? '準備しています（初回は形態素解析の辞書を取得します）' : run.done ? `読み終えました（${run.tools.join('、')}）` : `読んでいます（${read} / ${run.sentences.length} 文）`;
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={PROOFREAD_TITLE} icon={PROOFREAD_ICON} controls={[close]}>
      <div class="proof">
        <div class="proof-bar">
          {run ? <button onClick={edit}>編集に戻る</button> : <button disabled={!text.trim()} onClick={start}>校正</button>}
          <span>{status}</span>
        </div>
        {run && (
          <div class="proof-axes" role="group" aria-label="見せる軸">
            {AXES.map(({ axis, hint }) => {
              const all = run.findings.flat().filter((x) => x.axis === axis);
              const fails = all.filter((x) => x.level === 'fail').length;
              return (
                <button key={axis} title={hint} aria-pressed={only === axis} onClick={() => setOnly(only === axis ? undefined : axis)}>
                  {axis} <span class="proof-count">×{fails} △{all.length - fails}</span>
                </button>
              );
            })}
          </div>
        )}
        {run?.notices.map((n, k) => <p key={k} class="proof-notice">{n}</p>)}
        {run ? (
          <div class="proof-body">
            <div class="proof-text">
              {run.sentences.map((s, i) => {
                const findings = shown(i);
                const marks = findings.filter((x) => x.quote).map((x) => ({ quote: x.quote!, note: `${x.axis}：${x.message}`, fail: x.level === 'fail' }));
                return (
                  <Fragment key={i}>
                    {run.text.slice(run.sentences[i - 1]?.end ?? 0, s.start)}
                    <span
                      class="proof-sentence"
                      data-level={run.error[i] ? 'error' : run.readings[i] ? levelOf(findings) : undefined}
                      aria-current={i === picked}
                      role="button"
                      tabIndex={0}
                      onClick={() => setPicked(i)}
                      onKeyDown={(e) => e.key === 'Enter' && setPicked(i)}
                    >
                      {piecesOf(run.text.slice(s.start, s.end), marks).map((p, j) => (p.mark ? <mark key={j} data-fail={p.mark.fail} title={p.mark.note}>{p.text}</mark> : p.text))}
                    </span>
                  </Fragment>
                );
              })}
              {run.text.slice(run.sentences.at(-1)?.end ?? 0)}
            </div>
            <Detail run={{ ...run, findings: run.findings.map((_, i) => shown(i)) }} i={picked} />
          </div>
        ) : (
          <div class="proof-body">
            <textarea
              class="proof-input"
              aria-label="校正する文章"
              placeholder="初めて読む人が前から読み進められるかを調べる文章を貼り付けて、「校正」を押してください"
              spellcheck={false}
              value={text}
              onInput={(e) => save({ text: e.currentTarget.value })}
            />
            <label class="proof-glossary">
              用語集
              <textarea
                aria-label="用語集"
                placeholder={'読み手が知っている語を 1 行に 1 つ書きます。\n「語：説明」と書くと、説明も読み手に渡します。\n\n例）\nevent-receiver：計測のイベントを受け取り、計測 JS を配るサービス\nCloud Run'}
                spellcheck={false}
                value={glossary}
                onInput={(e) => save({ glossary: e.currentTarget.value })}
              />
            </label>
          </div>
        )}
      </div>
    </FloatFrame>
  );
};
