/**
 * HTML の相対パスの起点を href に向ける。HTML が自分で base を決めていれば、そのままにする。
 * doctype より前に足すと互換モードに落ちるので、head の先頭か、head が無ければ doctype の直後に足す。
 */
export const withBase = (html: string, href: string): string => {
  if (/<base\s/i.test(html)) return html;
  const at = /<head(?:\s[^>]*)?>/i.exec(html) ?? /^\s*<!doctype[^>]*>/i.exec(html);
  const end = at ? at.index + at[0].length : 0;
  return `${html.slice(0, end)}<base href="${href}">${html.slice(end)}`;
};

/** <img> を 1 つだけ書いた HTML の src と alt。GitHub の画面で貼った画像は、Markdown の中にこの形で入る。ほかの HTML なら undefined。 */
export const imgOf = (html: string): { src: string; alt: string } | undefined => {
  const attrs = /^\s*<img\b([^>]*)>\s*$/i.exec(html)?.[1] ?? '';
  const attr = (name: string): string | undefined => new RegExp(`\\s${name}\\s*=\\s*(["'])(.*?)\\1`, 'i').exec(attrs)?.[2];
  const src = attr('src');
  return src ? { src, alt: attr('alt') ?? '' } : undefined;
};
