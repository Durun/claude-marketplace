// 文章を前から 1 文ずつ、初めて読む人の立場の haiku に読ませ、読み進められるかを文ごとに示す「文章校正」。メモ帳と同じく、スキンによらず XP の見た目で描く。
// 書いた文章はウィンドウの設定に持ち、閉じるまで残す。読み手の答えは、編集に戻るか閉じると捨てる。
import { Fragment } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { ProofReading, ProofVerdict } from '../shared/types.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { piecesOf, sentencesOf } from './proofread.ts';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「proofread:開いた時刻」にし、いくつも開けるようにする。 */
export const PROOFREAD_KEY = 'proofread';
export const PROOFREAD_TITLE = '文章校正';
export const PROOFREAD_ICON = '/proofread.svg';
const SIZE = { w: 760, h: 520 };
// 1 文ごとに claude を動かすので、同時に読ませる数を抑える。
const PARALLEL = 4;
// 貼り間違えた長いログなどで、claude を何百回も動かさないようにする。
const MAX_SENTENCES = 200;

export const openProofread = (): void => floatUp(PROOFREAD_KEY, '', { id: `${PROOFREAD_KEY}:${Date.now()}` }, SIZE);

export const ProofreadIcon = () => <DesktopIcon icon={PROOFREAD_ICON} title={PROOFREAD_TITLE} hint="初めて読む人が、文章を前から読み進められるかを調べます。ダブルクリックで開きます" onLaunch={openProofread} />;

type Result = ProofReading | { error: string };

const MARKS: Record<ProofVerdict['level'], string> = { yes: '○', partly: '△', no: '×' };
const ASPECTS = [
  ['understand', '理解できるか'],
  ['role', '立ち位置が分かるか'],
  ['foresee', 'この後を予想できるか'],
] as const;

/** 文に付ける印。3 つの答えのうちいちばん悪いもので、読めなかった文は error。 */
const levelOf = (r: Result): ProofVerdict['level'] | 'error' => {
  if ('error' in r) return 'error';
  const levels = ASPECTS.map(([key]) => r[key].level);
  return levels.includes('no') ? 'no' : levels.includes('partly') ? 'partly' : 'yes';
};

const readingOf = (r: Result | undefined): ProofReading | undefined => (r && !('error' in r) ? r : undefined);

const Detail = ({ sentence, result }: { sentence: string; result?: Result }) => {
  const reading = readingOf(result);
  return (
    <aside class="proof-detail">
      <blockquote>{sentence.trim()}</blockquote>
      {!result && <p>まだ読んでいません。</p>}
      {result && 'error' in result && <p>読めませんでした。{result.error}</p>}
      {reading && (
        <>
          <dl>
            {ASPECTS.map(([key, label]) => (
              <div key={key}>
                <dt>{MARKS[reading[key].level]} {label}</dt>
                <dd>{reading[key].note}</dd>
              </div>
            ))}
          </dl>
          {reading.questions.length > 0 && (
            <>
              <h3>引っかかった語句</h3>
              <ul>{reading.questions.map((q, i) => <li key={i}>「{q.quote}」{q.question}</li>)}</ul>
            </>
          )}
        </>
      )}
    </aside>
  );
};

const summaryOf = (results: Result[]): string => {
  const count = (level: string) => results.filter((r) => levelOf(r) === level).length;
  const failed = count('error');
  return `読み終えました。× ${count('no')} 文、△ ${count('partly')} 文${failed ? `、読めなかった文 ${failed}` : ''}`;
};

export const ProofreadFloat = ({ f }: { f: Float }) => {
  const text = f.tab.settings?.text ?? '';
  // 読ませている文章の文。無い間は、文章を書いている。
  const [sentences, setSentences] = useState<string[]>();
  const [results, setResults] = useState<Result[]>([]);
  const [picked, setPicked] = useState(0);
  // 読ませている回の番号。編集に戻るか閉じたら進め、前の回の答えを捨てて、残りの文を読ませない。
  const round = useRef(0);
  useEffect(() => () => void round.current++, []);

  const start = () => {
    const list = sentencesOf(text);
    if (list.length > MAX_SENTENCES) return alert(`文が多すぎます。${MAX_SENTENCES} 文までにしてください（${list.length} 文あります）`);
    const id = ++round.current;
    setSentences(list);
    setResults([]);
    setPicked(0);
    let next = 0;
    // 前の文から順に読ませ、答えが届いた文から色を付ける。
    const worker = async () => {
      while (round.current === id && next < list.length) {
        const i = next++;
        const result: Result = await api.proofread(list.slice(0, i).join(''), list[i]!).catch((e: Error) => ({ error: e.message }));
        if (round.current !== id) return;
        setResults((rs) => {
          const copy = [...rs];
          copy[i] = result;
          return copy;
        });
      }
    };
    for (let n = 0; n < PARALLEL; n++) void worker();
  };
  const edit = () => {
    round.current++;
    setSentences(undefined);
  };

  const done = results.filter(Boolean).length;
  const status = sentences && (done < sentences.length ? `読んでいます（${done} / ${sentences.length} 文）` : summaryOf(results));
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={PROOFREAD_TITLE} icon={PROOFREAD_ICON} controls={[close]}>
      <div class="proof">
        <div class="proof-bar">
          {sentences ? <button onClick={edit}>編集に戻る</button> : <button disabled={!text.trim()} onClick={start}>校正</button>}
          <span>{status}</span>
        </div>
        {sentences ? (
          <div class="proof-body">
            <div class="proof-text">
              {sentences.map((s, i) => {
                const reading = readingOf(results[i]);
                // 文の後ろの改行は枠の外に置き、選んだ文の枠が空いた行まで伸びないようにする。
                const body = s.trimEnd();
                return (
                  <Fragment key={i}>
                    <span
                      class="proof-sentence"
                      data-level={results[i] && levelOf(results[i])}
                      aria-current={i === picked}
                      role="button"
                      tabIndex={0}
                      onClick={() => setPicked(i)}
                      onKeyDown={(e) => e.key === 'Enter' && setPicked(i)}
                    >
                      {reading ? piecesOf(body, reading.questions).map((p, j) => (p.question ? <mark key={j} title={p.question}>{p.text}</mark> : p.text)) : body}
                    </span>
                    {s.slice(body.length)}
                  </Fragment>
                );
              })}
            </div>
            <Detail sentence={sentences[picked] ?? ''} result={results[picked]} />
          </div>
        ) : (
          <textarea
            class="proof-input"
            aria-label="校正する文章"
            placeholder="初めて読む人が前から読み進められるかを調べる文章を貼り付けて、「校正」を押してください"
            spellcheck={false}
            value={text}
            onInput={(e) => setFloatSettings(f, { ...f.tab.settings, text: e.currentTarget.value })}
          />
        )}
      </div>
    </FloatFrame>
  );
};
