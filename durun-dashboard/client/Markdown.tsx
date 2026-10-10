import { Marked, type MarkedToken, type Token } from 'marked';
import { type ComponentType, createContext } from 'preact';
import { useContext, useState } from 'preact/hooks';
import { bareUrls } from './chatLinks.ts';
import { headLines } from './format.ts';
import { CopyButton, Ext } from './ui.tsx';

// Claude の発言は信頼できない内容なので、HTML として解釈させずに Preact の要素へ組み直す。
// 生の HTML は文字のまま出し、リンクは http(s) のものだけを開けるようにする。
// 発言は 1 文ごとに改行して書かれることが多いので、改行はそのまま改行として扱う。
const md = new Marked({ breaks: true }, bareUrls);

type Heading = 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';

/** インラインのコードを描く部品。ファイル名を開けるリンクにするときに渡す。 */
export type CodeLink = ComponentType<{ text: string }>;
const CodeLinkContext = createContext<CodeLink | undefined>(undefined);

const CodeSpan = ({ text }: { text: string }) => {
  const Link = useContext(CodeLinkContext);
  return Link ? <Link text={text} /> : <code>{text}</code>;
};

const Inline = ({ tokens = [] }: { tokens?: Token[] }) => <>{tokens.map((t, i) => <InlineToken key={i} t={t as MarkedToken} />)}</>;

const InlineToken = ({ t }: { t: MarkedToken }) => {
  switch (t.type) {
    case 'strong': return <strong><Inline tokens={t.tokens} /></strong>;
    case 'em': return <em><Inline tokens={t.tokens} /></em>;
    case 'del': return <del><Inline tokens={t.tokens} /></del>;
    case 'codespan': return <CodeSpan text={t.text} />;
    case 'br': return <br />;
    case 'link': return <Ext href={t.href}><Inline tokens={t.tokens} /></Ext>;
    // 外部の画像を勝手に読み込まないよう、リンクとして出す。
    case 'image': return <Ext href={t.href}>{t.text || t.href}</Ext>;
    case 'checkbox': return <input type="checkbox" checked={t.checked} disabled />;
    case 'text': return t.tokens ? <Inline tokens={t.tokens} /> : <>{t.text}</>;
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
    case 'code': case 'html': return <CodeBlock text={t.text} />;
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

export const Markdown = ({ text, codeLink }: { text: string; codeLink?: CodeLink }) => (
  <CodeLinkContext.Provider value={codeLink}>
    <div class="md"><Blocks tokens={md.lexer(text)} /></div>
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
