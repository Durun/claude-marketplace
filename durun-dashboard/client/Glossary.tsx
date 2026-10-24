import { useMemo, useState } from 'preact/hooks';
import type { GlossaryTerm, Session } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { explain } from './Dolphin.tsx';
import { termSplitter } from './format.ts';
import { useCached } from './hooks.ts';
import { useLightMode } from './lightMode.ts';
import type { MarkText } from './Markdown.tsx';
import { Empty } from './ui.tsx';

/** 発言が増えたときと、Claude が止まったときに、用語集を拾い直す。on が偽なら読まない。 */
const useGlossary = (s: Session, on = true) => {
  const progress = `${s.status} ${s.lastMessageAt} ${s.lastPrompt}`;
  return useCached(() => (on ? api.glossary(s.id) : Promise.resolve(undefined)), [s.id, progress, JSON.stringify([s.terms, s.hiddenTerms]), on]);
};

/** チャットの中の用語。マウスを載せている間、イルカが吹き出しで説明する。 */
const Term = ({ text, term }: { text: string; term: GlossaryTerm }) => (
  <span class="chat-term" onMouseEnter={(e) => explain({ term, at: e.currentTarget })}>{text}</span>
);

/**
 * チャットの文に出た用語集の語に、下線を引く関数。
 * 軽量モードでは説明するイルカがいないので、下線を引かず、用語集も読まない。
 */
export const useTermMarks = (s: Session): MarkText | undefined => {
  const light = useLightMode();
  const { data } = useGlossary(s, !light);
  return useMemo(() => {
    if (!data) return undefined;
    const split = termSplitter(data.terms);
    return (text: string) => split(text).map((p, i) => (p.term ? <Term key={i} text={p.text} term={p.term} /> : p.text));
  }, [data]);
};

/** 用語を足す欄。説明を空のまま足すと、サーバーが haiku にやり取りから説明を書かせるので、書き終えるまで欄を残す。 */
const AddTerm = ({ s, onClose }: { s: Session; onClose: () => void }) => {
  const [term, setTerm] = useState('');
  const [description, setDescription] = useState('');
  const [busy, setBusy] = useState(false);
  const save = async (e: Event) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    const ok = await attempt('用語の追加', async () => (await api.addTerm(s.id, term, description), true));
    setBusy(false);
    if (ok) onClose();
  };
  return (
    <form class="card panel stack add-link" onSubmit={save}>
      <input class="input" required autoFocus readOnly={busy} aria-label="用語" placeholder="用語" value={term} onInput={(e) => setTerm(e.currentTarget.value)} />
      <input class="input" readOnly={busy} aria-label="説明" placeholder="説明（空なら、やり取りから haiku が書きます）" value={description} onInput={(e) => setDescription(e.currentTarget.value)} />
      <div class="actions">
        <span class="spacer" />
        <button type="button" class="btn btn-sm" onClick={onClose}>キャンセル</button>
        <button class="btn btn-sm btn-primary" disabled={busy}>{busy && !description.trim() ? '説明を書いています…' : '追加'}</button>
      </div>
    </form>
  );
};

/**
 * セッションの用語集のウィジェット。やり取りから haiku が拾った専門用語と、人間が「追加」で足した語を、説明を添えて並べる。
 * 拾い直す間と、まだ拾っていないやり取りを裏で拾っている間は、それまでの用語集を薄く出しておく。
 */
export const Glossary = ({ s }: { s: Session }) => {
  const [adding, setAdding] = useState(false);
  const { data, loading, error } = useGlossary(s);
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">
          用語集{data && <span class="count-pill">{data.terms.length}</span>}
        </h2>
        <button class="btn btn-sm" data-icon="add" aria-expanded={adding} onClick={() => setAdding((v) => !v)}>追加</button>
      </div>
      <div class="widget-body">
        {adding && <AddTerm s={s} onClose={() => setAdding(false)} />}
        {data?.error && <p class="help">やり取りから用語を拾えなかったところがあります（{data.error}）</p>}
        {!data || (data.pending && data.terms.length === 0) ? (
          <Empty>{error ? `用語を拾えませんでした（${error}）` : '用語を拾っています…'}</Empty>
        ) : data.terms.length === 0 ? (
          <Empty>用語はまだありません。</Empty>
        ) : (
          <dl class={`glossary${loading || data.pending ? ' is-loading' : ''}`}>
            {data.terms.map((t) => (
              <div key={t.term} class="glossary-item">
                <dt>
                  <span class="glossary-term">{t.term}</span>
                  <button class="side-doc-action" title="用語集から外す" aria-label={`${t.term}を用語集から外す`} onClick={() => attempt('用語を外す', () => api.hideTerm(s.id, t.term))}>
                    ×
                  </button>
                </dt>
                <dd>{t.description}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </>
  );
};
