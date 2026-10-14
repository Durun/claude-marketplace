// 文章を前から 1 文ずつ、前提知識を持たない読み手に読ませ、7 つの軸で読み進めやすさを示す「文章校正」。メモ帳と同じく、スキンによらず XP の見た目で描く。
// 文章、用語集、校正の結果、人間のコメントと固定した文はファイルにして保存し、止めた校正は後から開いて続けられる。
// 校正を終えた版から、指摘に沿って書き直した次の版を作れる。良い文を固定しながら版を重ねると、固定した文を残したまま直しを重ねられる。
import { type ComponentChildren, Fragment } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { addTerm, applyEvent, emptyRun, findingKey, glossaryEntries, isFixed, isIgnored, newFile, newVersion, piecesOf, reviseInput, sentenceText, titleOf } from '../shared/proofread.ts';
import type { ProofAxis, ProofComment, ProofFile, ProofFileMeta, ProofFinding, ProofReading, ProofRun, ProofVersion, TokenTotals } from '../shared/types.ts';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp, setFloatSettings } from './Float.tsx';
import { fmtInt, fmtTokens, totalTokens } from './format.ts';
import { toast } from './toast.tsx';
import { copyText } from './ui.tsx';
import { XpMenu } from './XpMenu.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「proofread:開いた時刻」にし、いくつも開けるようにする。 */
export const PROOFREAD_KEY = 'proofread';
export const PROOFREAD_TITLE = '文章校正';
export const PROOFREAD_ICON = '/proofread.svg';
const SIZE = { w: 960, h: 640 };
// 打つたびに保存しないよう、手が止まってから保存する。
const SAVE_DELAY_MS = 500;

export const openProofread = (): void => floatUp(PROOFREAD_KEY, '', { id: `${PROOFREAD_KEY}:${Date.now()}` }, SIZE);

// 添付を受けた Claude は、MCP の proofread でこのアプリと同じ校正を走らせ、結果をこのアプリのファイルに残す。
export const ProofreadIcon = () => (
  <DesktopIcon
    icon={PROOFREAD_ICON}
    title={PROOFREAD_TITLE}
    hint="初めて読む人が、文章を前から読み進められるかを調べます。ダブルクリックで開きます。送信欄へドラッグすると、メッセージに添付して Claude に使わせます"
    attach="文章校正アプリ（durun-dashboard の proofread ツール）"
    onLaunch={openProofread}
  />
);

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

/** 文に付ける印。見せている指摘のうち、いちばん重いもの。 */
const levelOf = (findings: ProofFinding[]): 'fail' | 'note' | 'ok' =>
  findings.some((f) => f.level === 'fail') ? 'fail' : findings.length ? 'note' : 'ok';

const ReaderNotes = ({ reading }: { reading: ProofReading }) => (
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
);

type FindingProps = { f: ProofFinding; comment?: ProofComment; ignored: boolean; known: boolean; onComment: (c?: ProofComment) => void; onTerm: (term: string, note: string) => void };

/** 指摘 1 件。コメントは、修正版でその指摘をどう直すかの指示になる。「コメントして無視」にした指摘は、数に入れず、修正版でも直さない。 */
const FindingItem = ({ f, comment, ignored, known, onComment, onTerm }: FindingProps) => {
  const [editing, setEditing] = useState<'comment' | 'ignore' | 'term'>();
  const [draft, setDraft] = useState('');
  const input = useRef<HTMLTextAreaElement & HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), [editing]);
  const begin = (kind: typeof editing) => {
    setEditing(kind);
    setDraft(comment?.text ?? '');
  };
  const submit = (e: Event) => {
    e.preventDefault();
    const text = draft.trim();
    if (editing === 'term') onTerm(f.quote ?? '', text);
    else onComment(text || editing === 'ignore' ? { text, ignore: editing === 'ignore' } : undefined);
    setEditing(undefined);
  };
  const field = { ref: input, value: draft, onInput: (e: { currentTarget: HTMLTextAreaElement | HTMLInputElement }) => setDraft(e.currentTarget.value) };
  return (
    <li data-level={f.level} data-ignored={ignored || undefined}>
      <b>{f.level === 'fail' ? '×' : '△'} {f.axis}</b>
      {f.quote && <q>{f.quote}</q>}
      <span>{f.message}</span>
      {!!f.suggestions?.length && <span class="proof-better">→ {f.suggestions.join('、')}</span>}
      {comment && (
        <p class="proof-comment">
          {comment.ignore ? '無視' : 'コメント'}{comment.text && `：${comment.text}`}
          <button class="proof-link" onClick={() => onComment(undefined)}>取り消す</button>
        </p>
      )}
      {editing ? (
        <form class="proof-edit" onSubmit={submit}>
          {editing === 'term' ? (
            <input {...field} aria-label={`「${f.quote}」の説明`} placeholder={`「${f.quote}」の説明（無くてもよい）`} />
          ) : (
            <textarea {...field} aria-label="コメント" rows={2} placeholder={editing === 'ignore' ? '無視する理由（無くてもよい）' : '修正版で、この指摘をどう直してほしいか'} />
          )}
          <span class="proof-actions">
            <button type="submit">{editing === 'term' ? '用語集に足す' : editing === 'ignore' ? '無視する' : '残す'}</button>
            <button type="button" onClick={() => setEditing(undefined)}>やめる</button>
          </span>
        </form>
      ) : (
        <span class="proof-actions">
          <button onClick={() => begin('comment')}>コメント</button>
          <button onClick={() => begin('ignore')}>コメントして無視</button>
          {f.quote && <button disabled={known} title={known ? '用語集にあります' : 'この語句を、説明を添えて用語集に足す'} onClick={() => begin('term')}>用語集へ</button>}
        </span>
      )}
    </li>
  );
};

type DetailProps = { ver: ProofVersion; run: ProofRun; i: number; only?: ProofAxis; glossary: string; onComment: (key: string, c?: ProofComment) => void; onTerm: (term: string, note: string) => void };

const Detail = ({ ver, run, i, only, glossary, onComment, onTerm }: DetailProps) => {
  if (!run.sentences[i]) return <div class="proof-detail" />;
  const reading = run.readings[i];
  const findings = (run.findings[i] ?? []).filter((x) => !only || x.axis === only);
  const known = new Set(glossaryEntries(glossary).map((e) => e.term));
  return (
    <div class="proof-detail">
      <blockquote>[{i + 1}] {sentenceText(run, i)}</blockquote>
      {isFixed(ver, run, i) && <p>固定した文です。指摘は数に入れず、修正版でも変えません。</p>}
      {run.errors[i] && <p>読めませんでした。{run.errors[i]}</p>}
      {!reading && !run.errors[i] && <p>まだ読んでいません。</p>}
      {reading && !findings.length && <p>指摘はありません。</p>}
      <ul class="proof-findings">
        {AXES.flatMap(({ axis }) => findings.filter((f) => f.axis === axis)).map((f) => {
          const key = findingKey(i, f);
          return <FindingItem key={key} f={f} comment={ver.comments[key]} ignored={isIgnored(ver, run, i, f)} known={known.has(f.quote ?? '')} onComment={(c) => onComment(key, c)} onTerm={onTerm} />;
        })}
      </ul>
      {reading && <ReaderNotes reading={reading} />}
    </div>
  );
};

/** 用語集を表にし、それぞれの語が文章に何回出てくるかを添える。出てこない語は薄くする。 */
const Glossary = ({ glossary, text }: { glossary: string; text: string }) => {
  const entries = glossaryEntries(glossary);
  return (
    <details class="proof-gloss" open>
      <summary>用語集（{entries.length} 語）</summary>
      {entries.length ? (
        <table>
          <tbody>
            {entries.map((e) => {
              const count = text.split(e.term).length - 1;
              return (
                <tr key={e.term} data-unused={count === 0 || undefined}>
                  <th>{e.term}</th>
                  <td>{e.note}</td>
                  <td title="文章に出てくる回数">{count}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      ) : (
        <p>まだありません。「編集」で書くか、指摘の「用語集へ」で足せます。</p>
      )}
    </details>
  );
};

const TOKEN_KINDS: [keyof TokenTotals, string][] = [['input', '入力'], ['output', '出力'], ['cacheRead', 'キャッシュ読取'], ['cacheCreation', 'キャッシュ作成']];

/** 内訳は、使った種類だけを並べる。Jev はキャッシュの数を返さない。 */
const breakdown = (t: TokenTotals, fmt: (n: number) => string): string =>
  TOKEN_KINDS.filter(([k]) => t[k] > 0).map(([k, label]) => `${label} ${fmt(t[k])}`).join('、');

/** 「表示」のメニューで出す、ウィンドウの下端の欄。選んでいる版の校正で、モデルごとに使ったトークン数を出す。 */
const UsageBar = ({ run }: { run?: ProofRun }) => {
  const models = Object.entries(run?.usage ?? {});
  const note = !run ? 'まだ校正していません' : !run.usage ? '記録がありません' : !models.length && 'まだ使っていません';
  return (
    <div class="proof-status">
      <span>消費トークン</span>
      {note ? <span>{note}</span> : models.map(([model, t]) => (
        <span key={model} title={breakdown(t, fmtInt)}>{model} 合計 {fmtTokens(totalTokens(t))}（{breakdown(t, fmtTokens)}）</span>
      ))}
    </div>
  );
};

/** 結果を見ているときも、校正した時点の文章ではなく、版の今の文章をコピーする。k は版の番号（0 から）。 */
const copyVersion = async (ver: ProofVersion, k: number): Promise<void> => {
  if (await copyText(ver.text)) toast(`v${k + 1} をコピーしました`);
  else toast('クリップボードにコピーできませんでした', 'error');
};

export const ProofreadFloat = ({ f }: { f: Float }) => {
  // ファイルに保存する前から開いていたウィンドウは、タブの id の時刻をファイルの id にし、設定に残る文章と用語集を引き継ぐ。
  const fileId = f.tab.settings?.file ?? f.tab.id.slice(PROOFREAD_KEY.length + 1);
  const [file, setFile] = useState<ProofFile>();
  const [failed, setFailed] = useState<string>();
  const [v, setV] = useState(0);
  const [editing, setEditing] = useState(false);
  // 校正している版の番号。
  const [running, setRunning] = useState<number>();
  const [revising, setRevising] = useState(false);
  const [picked, setPicked] = useState(0);
  // 見せる軸。空なら全部を見せる。
  const [only, setOnly] = useState<ProofAxis>();
  const [recent, setRecent] = useState<ProofFileMeta[]>([]);
  const showUsage = f.tab.settings?.usage === 'on';
  const stop = useRef<AbortController | null>(null);
  const latest = useRef<ProofFile>();
  const saved = useRef<ProofFile>();
  latest.current = file;

  const flush = () => {
    const cur = latest.current;
    if (!cur || cur === saved.current) return;
    saved.current = cur;
    api.saveProofFile({ ...cur, title: titleOf(cur) }).catch((e: Error) => toast(`文章校正のファイルを保存できませんでした（${e.message}）`, 'error'));
  };
  const select = (n: number) => {
    setV(n);
    setEditing(false);
    setPicked(0);
    setOnly(undefined);
  };
  useEffect(() => {
    const timer = setTimeout(flush, SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [file]);
  useEffect(() => () => {
    stop.current?.abort();
    flush();
  }, []);
  useEffect(() => {
    // 開いていたファイルの書きかけを保存し、読んでいる途中なら止めてから、別のファイルを開く。止めた校正は、続きから再開できる。
    flush();
    stop.current?.abort();
    setFile(undefined);
    setFailed(undefined);
    let live = true;
    api.proofFile(fileId).then(
      (loaded) => {
        if (!live) return;
        const next = loaded ?? newFile(fileId, f.tab.settings?.text, f.tab.settings?.glossary);
        saved.current = next;
        setFile(next);
        select(next.versions.length - 1);
      },
      (e: Error) => live && setFailed(e.message),
    );
    return () => {
      live = false;
    };
  }, [fileId]);

  const changeVersion = (n: number, fn: (ver: ProofVersion) => ProofVersion) =>
    setFile((cur) => cur && { ...cur, versions: cur.versions.map((x, k) => (k === n ? fn(x) : x)) });
  const setGlossary = (glossary: string) => setFile((cur) => cur && { ...cur, glossary });
  // 前のファイルの文章と用語集を新規のファイルに引き継がないよう、設定は表示の設定だけを残して作り直す。
  const openFile = (id: string) => setFloatSettings(f, { file: id, usage: showUsage ? 'on' : 'off' });

  /** resume なら、前に止めた校正の続きから読む。読み終えた文は読み直さず、指摘だけを作り直す。 */
  const start = (n: number, resume: boolean) => {
    if (!file) return;
    const ver = file.versions[n]!;
    const text = resume && ver.run ? ver.run.text : ver.text;
    stop.current?.abort();
    const ctrl = new AbortController();
    stop.current = ctrl;
    // 読んでいる途中で別のファイルを開いたら、届いた結果をそのファイルに書かない。
    const onRun = (fn: (run: ProofRun) => ProofRun) =>
      setFile((cur) => (cur?.id === file.id ? { ...cur, versions: cur.versions.map((x, k) => (k === n && x.run ? { ...x, run: fn(x.run) } : x)) } : cur));
    // 続きから校正するときは、前に読んだ分の消費トークンに足していく。
    const fresh = emptyRun(text);
    changeVersion(n, (x) => ({ ...x, run: resume ? { ...fresh, usage: x.run?.usage } : fresh }));
    setRunning(n);
    setEditing(false);
    setPicked(0);
    api.proofread(text, file.glossary, resume ? (ver.run?.readings ?? []) : [], (e) => onRun((r) => applyEvent(r, e)), ctrl.signal)
      .catch((e: Error) => !ctrl.signal.aborted && onRun((r) => ({ ...r, notices: [...r.notices, e.message] })))
      .finally(() => stop.current === ctrl && setRunning(undefined));
  };

  const makeVersion = async (n: number) => {
    const ver = file?.versions[n];
    if (!file || !ver?.run) return;
    const { fixed, notes } = reviseInput(ver, ver.run);
    const at = file.versions.length;
    setRevising(true);
    try {
      const { text } = await api.reviseProof(ver.run.text, file.glossary, fixed, notes);
      setFile((cur) => (cur?.id === file.id ? { ...cur, versions: [...cur.versions, newVersion(text, n, ver.fixed)] } : cur));
      if (latest.current?.id === file.id) select(at);
    } catch (e) {
      toast(`修正版を作れませんでした（${(e as Error).message}）`, 'error');
    } finally {
      setRevising(false);
    }
  };

  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  const menuId = (name: string) => `proofread-${f.tab.id}-${name}`;
  const n = file ? Math.min(v, file.versions.length - 1) : 0;
  const ver = file?.versions[n];
  const frame = (title: string, body: ComponentChildren) => (
    <FloatFrame f={f} title={`${title} - ${PROOFREAD_TITLE}`} icon={PROOFREAD_ICON} controls={[close]}>
      <div class="proof">
        <nav class="proof-menubar">
          <button class="xp-menu" popovertarget={menuId('file')}>ファイル(F)</button>
          {/* 最近使ったファイルと版のエクスポートは、ファイルのメニューの中から開く入れ子の popover にする。項目を押したら、入れ子ごと閉じる。 */}
          <div
            id={menuId('file')}
            class="xp-dropdown"
            popover
            onToggle={(e) => e.newState === 'open' && api.proofFiles().then(setRecent, () => setRecent([]))}
            onClick={(e) => {
              const item = (e.target as Element).closest('button');
              if (item && !item.hasAttribute('popovertarget')) e.currentTarget.hidePopover();
            }}
          >
            <button class="xp-item" onClick={() => openFile(String(Date.now()))}><span>新規(N)</span></button>
            <button class="xp-item proof-submenu" popovertarget={menuId('recent')}><span>最近使ったファイル(F)</span></button>
            <div id={menuId('recent')} class="xp-dropdown" popover>
              {recent.map((r) => (
                <button key={r.id} class="xp-item" role="menuitemcheckbox" aria-checked={r.id === fileId} onClick={() => openFile(r.id)}>
                  <span>{r.title || '無題'}</span>
                  <kbd>{new Date(r.updatedAt).toLocaleDateString('ja-JP')}</kbd>
                </button>
              ))}
              {!recent.length && <button class="xp-item" disabled><span>（なし）</span></button>}
            </div>
            {ver && (
              <>
                <button class="xp-item proof-submenu" popovertarget={menuId('export')}><span>v{n + 1} をエクスポート(E)</span></button>
                <div id={menuId('export')} class="xp-dropdown" popover>
                  <button class="xp-item" onClick={() => copyVersion(ver, n)}><span>クリップボードにコピー(C)</span></button>
                </div>
              </>
            )}
          </div>
          <XpMenu
            id={menuId('view')}
            class="xp-menu"
            label="表示(V)"
            items={[{ label: '消費トークン(T)', checked: showUsage, onClick: () => setFloatSettings(f, { ...f.tab.settings, usage: showUsage ? 'off' : 'on' }) }]}
          />
        </nav>
        {body}
        {showUsage && ver && <UsageBar run={ver.run} />}
      </div>
    </FloatFrame>
  );

  if (!file || !ver) return frame('無題', <p class="proof-notice">{failed ? `ファイルを開けませんでした（${failed}）` : '開いています'}</p>);
  const run = ver.run;
  const tabs = (
    <div class="proof-tabs" role="tablist" aria-label="版">
      {file.versions.map((x, k) => (
        <button key={k} role="tab" aria-selected={k === n} title={x.from === undefined ? '元の文章' : `v${x.from + 1} の指摘から作った版`} onClick={() => select(k)}>
          v{k + 1}{running === k ? '（校正中）' : ''}
        </button>
      ))}
    </div>
  );
  const copy = <button title="この版の文章をクリップボードにコピーします" onClick={() => copyVersion(ver, n)}>コピー</button>;

  if (editing || !run) {
    const fixed = ver.fixed.filter((s) => ver.text.includes(s)).length;
    return frame(titleOf(file) || '無題', (
      <>
        {tabs}
        <div class="proof-bar">
          <button disabled={!ver.text.trim()} onClick={() => start(n, false)}>校正</button>
          {run && <button onClick={() => setEditing(false)}>結果を見る</button>}
          {copy}
          {fixed > 0 && <span>固定した文が {fixed} 文あります。修正版でも変えません。</span>}
        </div>
        <div class="proof-body">
          <textarea
            class="proof-input"
            aria-label="校正する文章"
            placeholder="初めて読む人が前から読み進められるかを調べる文章を貼り付けて、「校正」を押してください"
            spellcheck={false}
            value={ver.text}
            onInput={(e) => {
              const text = e.currentTarget.value;
              changeVersion(n, (x) => ({ ...x, text }));
            }}
          />
          <label class="proof-glossary">
            用語集
            <textarea
              aria-label="用語集"
              placeholder={'読み手が知っている語を 1 行に 1 つ書きます。\n「語：説明」と書くと、説明も読み手に渡します。\n\n例）\n共有リンク：メモを他人に見せるための URL\nPostgreSQL'}
              spellcheck={false}
              value={file.glossary}
              onInput={(e) => setGlossary(e.currentTarget.value)}
            />
          </label>
        </div>
      </>
    ));
  }

  const shown = (i: number) => (run.findings[i] ?? []).filter((x) => !isIgnored(ver, run, i, x) && (!only || x.axis === only));
  const read = run.readings.filter(Boolean).length;
  const status = running === n
    ? (!run.sentences.length ? '準備しています（初回は形態素解析の辞書を取得します）' : `読んでいます（${read} / ${run.sentences.length} 文）`)
    : revising ? '修正版を作っています'
    : run.done ? `読み終えました（${run.tools.join('、')}）` : `途中で止まっています（${read} / ${run.sentences.length} 文）`;
  const terms = glossaryEntries(file.glossary).map((t) => ({ quote: t.term, note: t.note || '用語集にある語', fail: false }));
  return frame(titleOf(file) || '無題', (
    <>
      {tabs}
      <div class="proof-bar">
        {running === n ? (
          <button onClick={() => stop.current?.abort()}>中断</button>
        ) : run.done ? (
          <button disabled={revising} title="無視していない指摘とコメントに沿って書き直し、次の版を作ります。固定した文は変えません" onClick={() => makeVersion(n)}>修正版を作る</button>
        ) : (
          <button onClick={() => start(n, true)}>続きから校正</button>
        )}
        <button onClick={() => setEditing(true)}>編集</button>
        {copy}
        <span>{status}</span>
      </div>
      <div class="proof-axes" role="group" aria-label="見せる軸">
        {AXES.map(({ axis, hint }) => {
          const all = run.findings.flatMap((list, i) => list.filter((x) => x.axis === axis && !isIgnored(ver, run, i, x)));
          const fails = all.filter((x) => x.level === 'fail').length;
          return (
            <button key={axis} title={hint} aria-pressed={only === axis} onClick={() => setOnly(only === axis ? undefined : axis)}>
              {axis} <span class="proof-count">×{fails} △{all.length - fails}</span>
            </button>
          );
        })}
      </div>
      {run.notices.map((notice, k) => <p key={k} class="proof-notice">{notice}</p>)}
      <div class="proof-body">
        <div class="proof-text">
          {run.sentences.map((s, i) => {
            const fixed = isFixed(ver, run, i);
            const findings = shown(i);
            const marks = findings.filter((x) => x.quote).map((x) => ({ quote: x.quote!, note: `${x.axis}：${x.message}`, fail: x.level === 'fail' }));
            return (
              <Fragment key={i}>
                {run.text.slice(run.sentences[i - 1]?.end ?? 0, s.start)}
                <input
                  type="checkbox"
                  class="proof-fix"
                  title="この文を固定します。固定した文は指摘を数に入れず、修正版でも変えません"
                  aria-label={`[${i + 1}] を固定する`}
                  checked={fixed}
                  onChange={() => {
                    const text = sentenceText(run, i);
                    changeVersion(n, (x) => ({ ...x, fixed: x.fixed.includes(text) ? x.fixed.filter((t) => t !== text) : [...x.fixed, text] }));
                  }}
                />
                <span
                  class="proof-sentence"
                  data-level={fixed ? 'fixed' : run.errors[i] ? 'error' : run.readings[i] ? levelOf(findings) : undefined}
                  aria-current={i === picked}
                  role="button"
                  tabIndex={0}
                  onClick={() => setPicked(i)}
                  onKeyDown={(e) => e.key === 'Enter' && setPicked(i)}
                >
                  {/* 指摘の付いた語句を先に決め、残りの部分で用語集の語に点線を引く。 */}
                  {piecesOf(sentenceText(run, i), fixed ? [] : marks).map((p, j) =>
                    p.mark ? (
                      <mark key={j} data-fail={p.mark.fail} title={p.mark.note}>{p.text}</mark>
                    ) : (
                      piecesOf(p.text, terms).map((q, k) => (q.mark ? <span key={`${j}-${k}`} class="proof-term" title={q.mark.note}>{q.text}</span> : q.text))
                    ),
                  )}
                </span>
              </Fragment>
            );
          })}
          {run.text.slice(run.sentences.at(-1)?.end ?? 0)}
        </div>
        <aside class="proof-side">
          <Detail
            ver={ver}
            run={run}
            i={picked}
            only={only}
            glossary={file.glossary}
            onComment={(key, comment) =>
              changeVersion(n, (x) => {
                const { [key]: _, ...rest } = x.comments;
                return { ...x, comments: comment ? { ...rest, [key]: comment } : rest };
              })}
            onTerm={(term, note) => setGlossary(addTerm(file.glossary, term, note))}
          />
          <Glossary glossary={file.glossary} text={run.text} />
        </aside>
      </div>
    </>
  ));
};
