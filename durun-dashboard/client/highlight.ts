import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import css from 'highlight.js/lib/languages/css';
import go from 'highlight.js/lib/languages/go';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import markdown from 'highlight.js/lib/languages/markdown';
import python from 'highlight.js/lib/languages/python';
import sql from 'highlight.js/lib/languages/sql';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

// 全言語を入れると重いので、よく見る言語だけを登録する。
const LANGUAGES = { bash, css, go, javascript, json, markdown, python, sql, typescript, xml, yaml };
for (const [name, lang] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, lang);

const BY_EXT: Record<string, keyof typeof LANGUAGES> = {
  ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript',
  js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
  json: 'json', css: 'css', html: 'xml', svg: 'xml', xml: 'xml', md: 'markdown',
  sh: 'bash', bash: 'bash', zsh: 'bash', py: 'python', go: 'go', sql: 'sql', yml: 'yaml', yaml: 'yaml',
};

export const languageOf = (path: string): string | undefined => BY_EXT[path.split('.').pop()?.toLowerCase() ?? ''];

/**
 * 1 行を色付けした HTML にする。highlight.js は入力の文字をすべて escape し、span だけを足す。
 * ponytail: 行ごとに色付けするので、複数行にまたがるコメントや文字列の 2 行目以降は色が付かない。
 */
export const highlightLine = (text: string, lang: string): string => hljs.highlight(text, { language: lang, ignoreIllegals: true }).value;
