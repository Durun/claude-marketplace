import { lexer } from 'marked';

// 先頭の YAML のフロントマター。閉じる --- が、下線で書く見出しとして読まれないよう外す。
const FRONT_MATTER = /^---\r?\n[\s\S]*?\r?\n---\r?\n/;

/** Markdown の最初の見出しの文字。コードブロックの中の # は見出しとみなさない。見出しが無ければ undefined。 */
export const firstHeading = (markdown: string): string | undefined => {
  const heading = lexer(markdown.replace(FRONT_MATTER, '')).find((t) => t.type === 'heading');
  return heading?.type === 'heading' ? heading.text.trim() || undefined : undefined;
};
