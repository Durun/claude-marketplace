// セッションの会話ログから、作業を図解した HTML の資料を作る。セッションの会話を進めないよう、セッションには頼まず補助の Claude に作らせる。
import fs from 'node:fs';
import type { ChatEntry, DocMeta } from '../shared/types.ts';
import { UpstreamError, askReader, objectSchema } from './haiku.ts';
import { newId, state, touch } from './store.ts';

// 長いセッションでも数分で作り終えるよう、会話は新しい側からこの文字数までを材料にする。
const MAX_CHAT_CHARS = 60_000;

const INSTRUCTIONS = `<chat> は、ある Claude Code のセッションの会話ログです。このセッションの作業を、人間が一目で把握できる図解の資料にしてください。
- 目的、全体の流れ、いまの状況、残っている判断を、図を中心にまとめる。
- 図は HTML と CSS、または inline SVG で描く。外部のスクリプト、スタイルシート、画像は読み込まない。
- html には <!doctype html> から始まる 1 枚の HTML を書く。明るい背景と暗い背景のどちらでも読めるようにする。
- 会話に出てくるファイルは、必要なら作業ディレクトリから読んで確かめてよい。
- 会話に書かれていないことは足さない。
- title には資料の題名を 30 文字以内で書く。`;

/** 依頼と発言だけを材料にする。ツールの呼び出しは量が多く、作業の流れは発言から追える。 */
const chatText = (entries: ChatEntry[]): string =>
  entries
    .filter((e) => e.kind !== 'tool')
    .map((e) => `${e.kind === 'prompt' ? '人間' : 'Claude'}: ${e.text}`)
    .join('\n\n')
    .slice(-MAX_CHAT_CHARS);

/** 作った資料は、present で出した資料と同じく、このセッションの資料の一覧に並べる。 */
export const makeDiagram = async (sessionId: string, cwd: string, entries: ChatEntry[]): Promise<DocMeta> => {
  const prompt = `${INSTRUCTIONS}\n\n<chat>\n${chatText(entries)}\n</chat>`;
  // 作業ツリーを消したセッションでは、そこで claude を起動できないので、ダッシュボードの場所で動かす。
  const dir = fs.existsSync(cwd) ? cwd : process.cwd();
  const { title, html } = await askReader(dir, prompt, objectSchema({ title: { type: 'string' }, html: { type: 'string' } }));
  if (typeof title !== 'string' || typeof html !== 'string' || !html.trim()) throw new UpstreamError('図解が返りませんでした');
  const meta: DocMeta = { id: newId(), sessionId, title: title.trim() || '図解', format: 'html', createdAt: Date.now(), seen: false };
  state.docs.push({ ...meta, content: html });
  touch();
  return meta;
};
