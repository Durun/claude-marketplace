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
