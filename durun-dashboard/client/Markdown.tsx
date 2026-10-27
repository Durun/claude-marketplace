import type { MarkedToken, Token } from 'marked';
import { type ComponentChildren, type ComponentType, createContext } from 'preact';
import { useContext, useState } from 'preact/hooks';
import { imgOf } from '../shared/html.ts';
import { GITHUB_ATTACHMENT } from '../shared/types.ts';
import { lexMarkdown } from './chatLinks.ts';
import { headLines } from './format.ts';
import { CopyButton, Ext } from './ui.tsx';

// Claude の発言は信頼できない内容なので、HTML として解釈させずに Preact の要素へ組み直す。
// 生の HTML は文字のまま出す。ただし <img> だけのものは、Markdown の画像と同じに扱う。リンクは http(s) のものだけを開けるようにする。

type Heading = 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';

/** インラインのコードを描く部品。ファイル名を開けるリンクにするときに渡す。 */
export type CodeLink = ComponentType<{ text: string }>;
const CodeLinkContext = createContext<CodeLink | undefined>(undefined);

const CodeSpan = ({ text }: { text: string }) => {
  const Link = useContext(CodeLinkContext);
  return Link ? <Link text={text} /> : <code>{text}</code>;
};

/** 地の文を描き直す関数。用語に印を付けるときに渡す。 */
export type MarkText = (text: string) => ComponentChildren;
const MarkTextContext = createContext<MarkText | undefined>(undefined);

const PlainText = ({ text }: { text: string }) => {
  const mark = useContext(MarkTextContext);
  return <>{mark ? mark(text) : text}</>;
};

/**
 * 外部の画像を勝手に読み込まないよう、リンクとして出す。押すと元の画像を開く。
 * GitHub の添付だけは画像で出す。非公開のリポジトリの添付も読めるよう、サーバーが gh で読んだものを出す。
 */
const ImageLink = ({ src, alt }: { src: string; alt: string }) => (
  <Ext href={src}>{GITHUB_ATTACHMENT.test(src) ? <img src={`/api/github/image?${new URLSearchParams({ url: src })}`} alt={alt} loading="lazy" /> : alt || src}</Ext>
);

const Inline = ({ tokens = [] }: { tokens?: Token[] }) => <>{tokens.map((t, i) => <InlineToken key={i} t={t as MarkedToken} />)}</>;

const InlineToken = ({ t }: { t: MarkedToken }) => {
  switch (t.type) {
    case 'strong': return <strong><Inline tokens={t.tokens} /></strong>;
    case 'em': return <em><Inline tokens={t.tokens} /></em>;
    case 'del': return <del><Inline tokens={t.tokens} /></del>;
    case 'codespan': return <CodeSpan text={t.text} />;
    case 'br': return <br />;
    // リンクの文には印を付けない。印の下線とリンクの下線が重なって見分けられないため。
    case 'link': return <Ext href={t.href}><MarkTextContext.Provider value={undefined}><Inline tokens={t.tokens} /></MarkTextContext.Provider></Ext>;
    case 'image': return <ImageLink src={t.href} alt={t.text} />;
    case 'html': {
      const img = imgOf(t.text);
      return img ? <ImageLink {...img} /> : <>{t.raw}</>;
    }
    case 'checkbox': return <input type="checkbox" checked={t.checked} disabled />;
    case 'text': return t.tokens ? <Inline tokens={t.tokens} /> : <PlainText text={t.text} />;
    case 'escape': return <>{t.text}</>;
    default: return <>{t.raw}</>;
  }
};

const CodeBlock = ({ text }: { text: string }) => (
  <div class="code-block">
    <pre><code>{text}</code></pre>
    <CopyButton text={text} label="コピー" title="コードをコピー" />
  </div>
);

const Blocks = ({ tokens }: { tokens: Token[] }) => <>{tokens.map((t, i) => <Block key={i} t={t as MarkedToken} />)}</>;

const Block = ({ t }: { t: MarkedToken }) => {
  switch (t.type) {
    case 'paragraph': return <p><Inline tokens={t.tokens} /></p>;
    case 'heading': {
      const H = `h${t.depth}` as Heading;
      return <H><Inline tokens={t.tokens} /></H>;
    }
    case 'code': return <CodeBlock text={t.text} />;
    // GitHub の画面で貼った画像は、<img> だけの段落になる。
    case 'html': return imgOf(t.text) ? <p><InlineToken t={t} /></p> : <CodeBlock text={t.text} />;
    case 'blockquote': return <blockquote><Blocks tokens={t.tokens} /></blockquote>;
    case 'list': {
      const List = t.ordered ? 'ol' : 'ul';
      return <List start={t.start || undefined}>{t.items.map((item, i) => <li key={i}><Blocks tokens={item.tokens} /></li>)}</List>;
    }
    case 'table': return (
      <table>
        <thead><tr>{t.header.map((c, i) => <th key={i}><Inline tokens={c.tokens} /></th>)}</tr></thead>
        <tbody>{t.rows.map((row, i) => <tr key={i}>{row.map((c, j) => <td key={j}><Inline tokens={c.tokens} /></td>)}</tr>)}</tbody>
      </table>
    );
    case 'hr': return <hr />;
    case 'space': case 'def': return null;
    default: return <InlineToken t={t} />;
  }
};

export const Markdown = ({ text, codeLink, markText }: { text: string; codeLink?: CodeLink; markText?: MarkText }) => (
  <CodeLinkContext.Provider value={codeLink}>
    <MarkTextContext.Provider value={markText}>
      <div class="md"><Blocks tokens={lexMarkdown(text)} /></div>
    </MarkTextContext.Provider>
  </CodeLinkContext.Provider>
);

/** 長い文は、max 文字に収まる行までを出し、「もっと読む」で全体を出す。 */
export const MarkdownPreview = ({ text, max }: { text: string; max: number }) => {
  const [open, setOpen] = useState(false);
  const head = headLines(text, max);
  return (
    <div class="md-preview">
      <Markdown text={open ? text : head} />
      {!open && head !== text && <button class="btn btn-sm" onClick={() => setOpen(true)}>もっと読む</button>}
    </div>
  );
};
