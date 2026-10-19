// Notion のページを、ウィンドウに浮かべて読む。
import { api } from './api.ts';
import { type Float, FloatFrame, NOTION_ICON, dock } from './Float.tsx';
import { useCached } from './hooks.ts';
import { Markdown } from './Markdown.tsx';
import { Empty, Loading, Time } from './ui.tsx';

/** ウィンドウに浮かべたページの保存キー。タブの id に、ハイフンの無い 32 桁のページの ID を入れる。 */
export const NOTION_KEY = 'notion';

/** Notion で開くリンクは、ページへのリンクを横取りする処理に拾わせないよう data-external を付ける。 */
const OpenInNotion = ({ url, class: cls, children }: { url: string; class?: string; children: string }) => (
  <a class={cls} href={url} target="_blank" rel="noopener noreferrer" data-external>{children}</a>
);

/** 題名はページを読むまで分からないので、それまでは「Notion」を見出しにする。読めないときも、Notion で開けるようにする。 */
export const NotionFloat = ({ f }: { f: Float }) => {
  const fetched = useCached((fresh) => api.notionPage(f.tab.id, fresh), [f.tab.id]);
  const page = fetched.data;
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={page?.title ?? 'Notion'} icon={NOTION_ICON} controls={[close]}>
      <div class="widget-body">
        {page ? (
          <div class="stack pr-view">
            <div class="pr-view-head">
              <OpenInNotion class="pr-view-title" url={page.url}>{`${page.icon ? `${page.icon} ` : ''}${page.title}`}</OpenInNotion>
              <span class="spacer" />
              <button class="btn btn-sm" data-icon="refresh" disabled={fetched.loading} title="Notion から読み直します" onClick={fetched.reload}>更新</button>
            </div>
            {page.updatedAt !== undefined && <div class="pr-view-meta muted"><span>更新 <Time at={page.updatedAt} /></span></div>}
            {fetched.error && <p class="error-note">読み直せませんでした（{fetched.error}）</p>}
            {page.markdown ? <div class={page.database ? 'notion-sheet' : undefined}><Markdown text={page.markdown} /></div> : <Empty>本文はありません。</Empty>}
            {page.truncated && <p class="muted">長いので、途中までを出しています。続きは <OpenInNotion url={page.url}>Notion で開く</OpenInNotion> と読めます。</p>}
          </div>
        ) : fetched.error ? (
          <Empty>ページを読めませんでした（{fetched.error}）。<OpenInNotion url={`https://www.notion.so/${f.tab.id}`}>Notion で開く</OpenInNotion></Empty>
        ) : (
          <Loading />
        )}
      </div>
    </FloatFrame>
  );
};
