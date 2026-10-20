import { useState } from 'preact/hooks';
import type { Session } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { useCached } from './hooks.ts';
import { Empty } from './ui.tsx';

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
 * 発言が増えたときと、Claude が止まったときに拾い直す。拾い直す間は、前の用語集を薄く出しておく。
 */
export const Glossary = ({ s }: { s: Session }) => {
  const [adding, setAdding] = useState(false);
  const progress = `${s.status} ${s.lastMessageAt} ${s.lastPrompt}`;
  const { data, loading, error } = useCached(() => api.glossary(s.id), [s.id, progress, JSON.stringify([s.terms, s.hiddenTerms])]);
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
        {!data ? (
          <Empty>{error ? `用語を拾えませんでした（${error}）` : '用語を拾っています…'}</Empty>
        ) : data.terms.length === 0 ? (
          <Empty>用語はまだありません。</Empty>
        ) : (
          <dl class={`glossary${loading ? ' is-loading' : ''}`}>
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
