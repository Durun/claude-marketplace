import type { Doc } from '../shared/types.ts';

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
 * 資料は Claude が書いた信頼できない内容なので、ダッシュボード本体の DOM には入れない。
 * 呼び出し側は、この文字列を sandbox="allow-scripts" の iframe の srcdoc にだけ渡す。
 */
export const buildSrcdoc = (doc: Pick<Doc, 'format' | 'content'>): string => {
  if (doc.format === 'html' || doc.format === 'svg') return doc.content;
  // '<' を Unicode エスケープにして、内容に含まれる </script> や <!-- で script 要素が終わらないようにする。
  const data = JSON.stringify({ format: doc.format, content: doc.content }).replace(/</g, '\\u003c');
  return [
    '<!doctype html><html lang="ja"><head><meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<style>${TEMPLATE_STYLE}</style></head><body><main id="out"></main>`,
    `<script type="application/json" id="doc-data">${data}</script>`,
    '<script src="/vendor/marked.js"></script>',
    '<script src="/vendor/mermaid.js"></script>',
    `<script>${TEMPLATE_SCRIPT}</script></body></html>`,
  ].join('');
};
