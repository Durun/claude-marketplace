import { withBase } from '../shared/html.ts';
import type { Doc } from '../shared/types.ts';
import { safeDecode } from './router.ts';
import type { Skin } from './skin.ts';

// markdown / mermaid / text を描画する固定のテンプレート。資料の内容は JSON として渡し、HTML としては解釈させない。
const TEMPLATE_STYLE = `
:root { color-scheme: light dark; --fg: #1f2328; --muted: #59636e; --bg: #ffffff; --border: #d1d9e0; --code-bg: #f3f4f6; --link: #0b5fc5; --err: #b42318; }
@media (prefers-color-scheme: dark) {
  :root { --fg: #e6e6e3; --muted: #9a9a94; --bg: #1a1a19; --border: #3a3a37; --code-bg: #252523; --link: #6da7ec; --err: #f08a84; }
}
html { background: var(--bg); }
body { margin: 0; padding: 24px 32px 64px; color: var(--fg); background: var(--bg); font: 15px/1.8 system-ui, -apple-system, "Segoe UI", "Hiragino Sans", "Noto Sans JP", sans-serif; }
main { max-width: 860px; margin: 0 auto; overflow-wrap: anywhere; }
h1, h2, h3, h4 { line-height: 1.4; margin: 1.8em 0 0.6em; }
h1:first-child, h2:first-child { margin-top: 0; }
h1 { font-size: 1.7em; border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }
h2 { font-size: 1.35em; border-bottom: 1px solid var(--border); padding-bottom: 0.25em; }
h3 { font-size: 1.12em; }
p, ul, ol, blockquote, pre, table { margin: 0 0 1em; }
a { color: var(--link); }
code { font: 0.88em ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; background: var(--code-bg); padding: 0.15em 0.35em; border-radius: 4px; }
pre { background: var(--code-bg); padding: 12px 14px; border-radius: 6px; overflow: auto; line-height: 1.55; }
pre code { background: none; padding: 0; font-size: 0.85em; }
blockquote { margin-left: 0; padding: 0 1em; color: var(--muted); border-left: 4px solid var(--border); }
table { border-collapse: collapse; display: block; overflow: auto; }
th, td { border: 1px solid var(--border); padding: 6px 12px; }
th { background: var(--code-bg); }
img { max-width: 100%; }
hr { border: 0; border-top: 1px solid var(--border); margin: 2em 0; }
.diagram { text-align: center; margin: 1.2em 0; overflow: auto; }
.diagram svg { max-width: 100%; height: auto; }
.err { color: var(--err); white-space: pre-wrap; text-align: left; font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
.code-block { position: relative; margin: 0 0 1em; }
.code-block pre { margin: 0; }
.code-block button { position: absolute; top: 6px; right: 6px; padding: 2px 9px; border: 1px solid var(--border); border-radius: 6px; background: var(--bg); color: var(--fg); font: 12px system-ui, sans-serif; cursor: pointer; opacity: 0.45; }
.code-block:hover button, .code-block button:focus-visible { opacity: 1; }
.plain { white-space: pre-wrap; font: 13px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace; background: none; padding: 0; }
`;

/**
 * Windows XP のスキンで、markdown / mermaid / text の資料に重ねる見た目。XP の Internet Explorer で、飾りの無いページを開いたときに寄せる。
 * 文字は当時の日本語の既定だった MS P ゴシックと MS ゴシックにし、なめらかにせず描く。
 * 表は border="1" を付けた表に見立て、外枠は浮き出させ、セルはへこませて間を空ける。
 */
const XP_TEMPLATE_STYLE = `
:root { --fg: #000; --bg: #fff; --link: #00f; --err: #f00; }
body { margin: 8px; padding: 0; font: 16px/1.4 "MS PGothic", "ＭＳ Ｐゴシック", "MS UI Gothic", Osaka, sans-serif; -webkit-font-smoothing: none; font-smooth: never; text-rendering: optimizeSpeed; }
main { max-width: none; }
h1, h2, h3, h4 { margin: 0.67em 0; line-height: normal; }
h1, h2 { border-bottom: 0; padding-bottom: 0; }
h1 { font-size: 2em; }
h2 { font-size: 1.5em; }
h3 { font-size: 1.17em; }
p, ul, ol, pre, table { margin: 1em 0; }
a:visited { color: #800080; }
blockquote { margin: 1em 40px; padding: 0; color: inherit; border: 0; }
code, pre code, .plain, .err { font: 1em "MS Gothic", "ＭＳ ゴシック", Osaka-Mono, monospace; background: none; padding: 0; border-radius: 0; }
pre { background: none; padding: 0; border-radius: 0; line-height: normal; }
table { width: max-content; max-width: 100%; border: 1px solid; border-color: #c0c0c0 #808080 #808080 #c0c0c0; border-collapse: separate; border-spacing: 2px; }
th, td { padding: 1px; border: 1px solid; border-color: #808080 #c0c0c0 #c0c0c0 #808080; vertical-align: top; }
th { background: none; }
hr { border: 1px inset; margin: 0.5em 0; }
.code-block { margin: 1em 0; }
.code-block button { top: 0; right: 0; padding: 1px 6px; border: 2px outset #fff; border-radius: 0; background: #c0c0c0; color: #000; font: 12px "MS UI Gothic", "MS PGothic", sans-serif; opacity: 0.6; }
.code-block button:active { border-style: inset; }
`;

const TEMPLATE_SCRIPT = `
const data = JSON.parse(document.getElementById('doc-data').textContent);
const out = document.getElementById('out');
let diagramCount = 0;

const fail = (host, message, source) => {
  host.className = 'err';
  host.textContent = '図を描画できませんでした: ' + message + '\\n\\n' + source;
};

// 非表示のタブや大きさ 0 の iframe では、mermaid が文字の幅を測れず崩れた図になる。
// 表示されて大きさが決まってから初期化して描く。
const whenVisible = () => new Promise((resolve) => {
  const check = () => (document.visibilityState === 'visible' && innerWidth > 0 ? resolve() : requestAnimationFrame(check));
  check();
});

let prepared;
const prepare = () => prepared ??= whenVisible().then(() => {
  const dark = matchMedia('(prefers-color-scheme: dark)').matches;
  window.mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'default' });
});

const draw = async (host, source) => {
  if (!window.mermaid) return fail(host, 'mermaid を読み込めませんでした', source);
  await prepare();
  diagramCount += 1;
  try {
    const { svg } = await window.mermaid.render('diagram-' + diagramCount, source.trim());
    host.innerHTML = svg;
  } catch (e) {
    fail(host, e && e.message ? e.message : String(e), source);
  }
};

const diagramHost = () => {
  const host = document.createElement('div');
  host.className = 'diagram';
  return host;
};

// sandbox の iframe は別オリジンの扱いで、iframe に allow="clipboard-write" が無いと Clipboard API を使えない。使えなければ、選んでコピーする古い方法に落とす。
const copy = async (text) => {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const area = document.createElement('textarea');
    area.value = text;
    document.body.append(area);
    area.select();
    const ok = document.execCommand('copy');
    area.remove();
    return ok;
  }
};

// コードブロックの右上に、コピーのボタンを置く。チャットの発言のコードブロックと同じ見た目と動きにする。
const addCopyButton = (pre) => {
  const box = document.createElement('div');
  box.className = 'code-block';
  const button = document.createElement('button');
  button.textContent = 'コピー';
  button.title = 'コードをコピー';
  button.onclick = async () => {
    button.textContent = (await copy(pre.textContent)) ? 'コピーしました' : 'コピーできませんでした';
    setTimeout(() => (button.textContent = 'コピー'), 1500);
  };
  pre.replaceWith(box);
  box.append(pre, button);
};

const renderMarkdown = async () => {
  if (!window.marked) {
    out.textContent = 'marked を読み込めませんでした';
    return;
  }
  out.innerHTML = window.marked.parse(data.content);
  const blocks = [...out.querySelectorAll('pre > code.language-mermaid')];
  await Promise.all(blocks.map((code) => {
    const host = diagramHost();
    code.parentElement.replaceWith(host);
    return draw(host, code.textContent);
  }));
  out.querySelectorAll('pre').forEach(addCopyButton);
};

// ファイルへのリンクは iframe の中では開けないので、ダッシュボードに開かせる。スキームの付いた URL とページ内リンクはそのままにし、C: のようなドライブ名はスキームとみなさない。
document.addEventListener('click', (e) => {
  const link = e.target.closest && e.target.closest('a[href]');
  const href = link && link.getAttribute('href');
  if (!href || /^(?:[a-z][a-z\\d+.-]+:|#|\\/\\/)/i.test(href)) return;
  e.preventDefault();
  parent.postMessage({ openFile: href }, '*');
});

if (data.format === 'text') {
  const pre = document.createElement('pre');
  pre.className = 'plain';
  pre.textContent = data.content;
  out.append(pre);
} else if (data.format === 'mermaid') {
  const host = diagramHost();
  out.append(host);
  draw(host, data.content);
} else {
  renderMarkdown();
}
`;

/**
 * Windows XP のスキンのスクロールバー。ダッシュボード本体と資料の iframe の両方に入れる。
 * XP の Internet Explorer に寄せ、白に近い溝に、滑り止めの線を引いた水色のつまみと矢印のボタンを置く。
 * scrollbar-color を指定すると Chrome は ::-webkit-scrollbar の指定を無視するので、XP では使わない。
 */
export const XP_SCROLLBAR_STYLE = `
::-webkit-scrollbar { width: 17px; height: 17px; background: linear-gradient(to bottom right, #f3f1ec, #fefefb); }
::-webkit-scrollbar-corner { background: #ece9d8; }
::-webkit-scrollbar-thumb, ::-webkit-scrollbar-button {
  border: 1px solid; border-color: #fff #a0b5d3 #7da0d4 #fff; border-radius: 3px;
  background: var(--glyph) center no-repeat, linear-gradient(to bottom right, rgba(255, 255, 255, 0.6), transparent) #b5cbf8;
}
::-webkit-scrollbar-thumb:hover, ::-webkit-scrollbar-button:hover { background-color: #cddcfd; }
::-webkit-scrollbar-thumb:active, ::-webkit-scrollbar-button:active { background-color: #a0baf3; }
::-webkit-scrollbar-thumb:vertical { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='7' height='8'><path d='M0 .5h6m-6 2h6m-6 2h6m-6 2h6' stroke='%23eef4fe'/><path d='M1 1.5h6m-6 2h6m-6 2h6m-6 2h6' stroke='%238cb0f8'/></svg>"); }
::-webkit-scrollbar-thumb:horizontal { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='8' height='7'><path d='M.5 0v6m2-6v6m2-6v6m2-6v6' stroke='%23eef4fe'/><path d='M1.5 1v6m2-6v6m2-6v6m2-6v6' stroke='%238cb0f8'/></svg>"); }
/*
 * 矢印のボタンは、ボタンを出さない macOS でも出るよう display: block にする。
 * grid や flex の欄では Chrome が両端に 2 つずつ並べるので、内側の 1 つずつは消す。
 */
::-webkit-scrollbar-button { display: block; width: 17px; height: 17px; }
::-webkit-scrollbar-button:start:increment, ::-webkit-scrollbar-button:end:decrement { display: none; }
::-webkit-scrollbar-button:vertical:decrement { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='9' height='6'><path d='M1 5l3.5-3.5L8 5' fill='none' stroke='%234d6185' stroke-width='2'/></svg>"); }
::-webkit-scrollbar-button:vertical:increment { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='9' height='6'><path d='M1 1l3.5 3.5L8 1' fill='none' stroke='%234d6185' stroke-width='2'/></svg>"); }
::-webkit-scrollbar-button:horizontal:decrement { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='6' height='9'><path d='M5 1L1.5 4.5 5 8' fill='none' stroke='%234d6185' stroke-width='2'/></svg>"); }
::-webkit-scrollbar-button:horizontal:increment { --glyph: url("data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='6' height='9'><path d='M1 1l3.5 3.5L1 8' fill='none' stroke='%234d6185' stroke-width='2'/></svg>"); }
`;

// srcdoc の資料では、ページ内リンク（#見出し）の起点が base かダッシュボードのアドレスになり、押すと iframe がそこへ移ってしまう。移らずにその場でスクロールする。
const HASH_LINK_SCRIPT = `<script>
document.addEventListener('click', (e) => {
  const link = e.target.closest && e.target.closest('a[href^="#"]');
  if (!link || e.defaultPrevented) return;
  e.preventDefault();
  const name = decodeURIComponent(link.getAttribute('href').slice(1));
  const to = name ? document.getElementById(name) || document.getElementsByName(name)[0] : document.documentElement;
  if (to) to.scrollIntoView();
});
</script>`;

/**
 * 資料は Claude が書いた信頼できない内容なので、ダッシュボード本体の DOM には入れない。
 * 呼び出し側は、この文字列を sandbox="allow-scripts" の iframe の srcdoc にだけ渡す。
 * iframe には本体の CSS が届かないので、スキンのスクロールバーはここで入れる。
 * ファイルから読んだ HTML の資料は、相対パスで埋め込んだ音声や画像を、サーバーが資料のディレクトリから返すアドレスへ向ける。
 */
export const buildSrcdoc = (doc: Pick<Doc, 'format' | 'content'> & Partial<Pick<Doc, 'id' | 'path'>>, skin: Skin = 'default'): string => {
  const skinStyle = skin === 'xp' ? `<style>${XP_SCROLLBAR_STYLE}</style>` : '';
  // HTML や SVG の資料は、先頭に足すと doctype より前になって互換モードに落ちるので、末尾に足す。
  if (doc.format === 'html') {
    const html = doc.id && doc.path ? withBase(doc.content, `/api/docs/${encodeURIComponent(doc.id)}/files/`) : doc.content;
    return html + HASH_LINK_SCRIPT + skinStyle;
  }
  if (doc.format === 'svg') return doc.content + skinStyle;
  // '<' を Unicode エスケープにして、内容に含まれる </script> や <!-- で script 要素が終わらないようにする。
  const data = JSON.stringify({ format: doc.format, content: doc.content }).replace(/</g, '\\u003c');
  return [
    '<!doctype html><html lang="ja"><head><meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<style>${TEMPLATE_STYLE}</style>${skin === 'xp' ? `<style>${XP_TEMPLATE_STYLE}</style>` : ''}${skinStyle}</head><body><main id="out"></main>`,
    `<script type="application/json" id="doc-data">${data}</script>`,
    '<script src="/vendor/marked.js"></script>',
    '<script src="/vendor/mermaid.js"></script>',
    `<script>${TEMPLATE_SCRIPT}</script></body></html>`,
  ].join('');
};

/** 資料の中のリンクが指すファイル。相対パスは base のファイルがあるディレクトリから辿る。marked が URL の形に直した文字は戻し、ページ内の場所（#）は外す。 */
export const linkPath = (base: string, href: string): string => {
  const path = safeDecode(href.replace(/#.*$/, ''));
  return /^(?:[/\\]|~\/|[a-z]:)/i.test(path) ? path : base.replace(/[^/\\]*$/, '') + path;
};
