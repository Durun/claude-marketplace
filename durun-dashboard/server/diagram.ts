// セッションの会話ログから、作業を図解した HTML の資料を作る。セッションの会話を進めないよう、セッションには頼まず補助の Claude に作らせる。
import fs from 'node:fs';
import path from 'node:path';
import type { ChatEntry, DocMeta } from '../shared/types.ts';
import { UpstreamError, askReader, objectSchema } from './haiku.ts';
import { newId, state, touch, writeDocFile } from './store.ts';

// 長いセッションでも数分で作り終えるよう、会話は新しい側からこの文字数までを材料にする。
const MAX_CHAT_CHARS = 60_000;

// 図解技法カタログ（guide.md）と技法ごとの例ファイル、資料に入れる共通の CSS と JavaScript の置き場所。
const KIT_DIR = path.join(import.meta.dirname, 'diagram');

const kit = (file: string): string => fs.readFileSync(path.join(KIT_DIR, file), 'utf8');

const INSTRUCTIONS = `<chat> は、ある Claude Code のセッションの会話ログです。このセッションの作業を、人間が一目で把握できる図解の資料にしてください。
- 作業で扱っている仕組み、処理やデータの流れ、構造、変更点を図で示す。意思決定や未決事項をまとめた節は作らない。
- 図は <guide> の図解技法カタログに沿って描く。<guide> の中のパスは ${KIT_DIR} からの相対パスなので、使う技法の例ファイルをそこから読んで写す。
- body には資料の <body> の中身だけを書く。doctype、head、共通の CSS と JavaScript は、ダッシュボードが足す。
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

const esc = (s: string): string => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;');

/** 補助の Claude が書いた body に共通の CSS と JavaScript を入れ、1 枚で完結する HTML にする。本文の <style> を共通の CSS より後に置き、共通の指定を上書きできるようにする。 */
export const diagramPage = (title: string, body: string): string =>
  [
    '<!doctype html><html lang="ja"><head><meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<title>${esc(title)}</title>`,
    `<style>\n${kit('assets/base.css')}\n${kit('assets/preview.css')}\n</style>`,
    '</head><body>',
    body,
    // base.js は読み込んだ時点の本文から目次とホバーを組み立てるので、本文の後に置く。
    `<script>\n${kit('assets/base.js')}\n</script>`,
    `<script>\n${kit('assets/preview.js')}\n</script>`,
    '</body></html>',
  ].join('\n');

/** 作った資料は、present で出した資料と同じく、このセッションの資料の一覧に並べる。 */
export const makeDiagram = async (sessionId: string, cwd: string, entries: ChatEntry[], note: string): Promise<DocMeta> => {
  // 人間の指示は、決まりの指示より後に置いて優先させる。
  const extra = note.trim() ? `\n\n人間からの指示です。ほかの指示とぶつかるときは、こちらを優先してください。\n<note>\n${note.trim()}\n</note>` : '';
  const prompt = `${INSTRUCTIONS}\n\n<guide>\n${kit('guide.md')}\n</guide>${extra}\n\n<chat>\n${chatText(entries)}\n</chat>`;
  // 作業ツリーを消したセッションでは、そこで claude を起動できないので、ダッシュボードの場所で動かす。
  const dir = fs.existsSync(cwd) ? cwd : process.cwd();
  const { title, body } = await askReader(dir, prompt, objectSchema({ title: { type: 'string' }, body: { type: 'string' } }));
  if (typeof title !== 'string' || typeof body !== 'string' || !body.trim()) throw new UpstreamError('図解が返りませんでした');
  const name = title.trim() || '図解';
  const id = newId();
  const meta: DocMeta = { id, sessionId, title: name, format: 'html', path: writeDocFile(id, 'html', diagramPage(name, body)), createdAt: Date.now(), seen: false };
  state.docs.push(meta);
  touch();
  return meta;
};
