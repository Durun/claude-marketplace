// セッションのやり取りから、概要と、人間が Claude に送る返答の候補と、セッション名の候補を haiku に作らせる。
import type { Session, SessionStatus, SessionSummary, TaskItem } from '../shared/types.ts';
import { cached, peekCached, refreshing } from './cache.ts';
import { UpstreamError, askHaiku, objectSchema } from './haiku.ts';

const MAX_REPLIES = 3;
// 一覧のカードの概要を同時に作る数。起動した直後などに、一覧のセッションの数だけ claude -p を一度に動かさないようにする。
const MAX_LIST_SUMMARIES = 2;
// 長い依頼を貼り付けたセッションでも数秒で返るよう、材料は 1 つあたりこの文字数までにする。
const MAX_BLOCK_CHARS = 4000;

/** 材料にするセッションのやり取り。最初の依頼は会話ログから読む。 */
export type Conversation = Pick<Session, 'id' | 'theme' | 'name' | 'status' | 'lastPrompt' | 'lastMessage' | 'tasks'> & { firstPrompt?: string };

const SUMMARY_INSTRUCTIONS = `この Claude Code のセッションの概要を、人間が一目で状況をつかめるように書いてください。
- doing: セッション全体の目的。何のために何をしているかを、終わったことの報告ではなく取り組みとして 1 文で書く。
- now: いまの状況。作業中なら何をしているか、止まっているなら何を終えて人間の何を待っているか。1〜2 文で書く。
- 主語の Claude は省き、短く書く。`;

const REPLY_INSTRUCTIONS = `Claude が最後にした発言に対して、人間が次に送りそうな返答の候補を ${MAX_REPLIES} つ作ってください。
- Claude に次にしてほしいことを頼む指示か、発言の中の質問への回答にする。
- 人間の最後の依頼と同じ口調で、1 文、40 文字以内で書く。
- ${MAX_REPLIES} つは違う方向にする（例: 提案に沿って進める、別の案を選ぶ、直しや確認を頼む）。`;

const TITLE_INSTRUCTIONS = `この Claude Code のセッションに付ける名前を 1 つ考えてください。
- 何の作業かが一覧で見分けられる名詞句にする。
- 20 文字以内で書く。`;

const STATUS_TEXT: Record<SessionStatus, string> = {
  busy: '作業中',
  idle: '入力待ち（応答を返し終え、人間の入力を待っている）',
  done: '完了（応答を返し終えて、セッションが止まっている）',
  interrupted: '中断（応答の途中で、セッションが止まっている）',
  ended: '終了（アーカイブ済み）',
};
const TASK_TEXT: Record<TaskItem['status'], string> = { pending: '未着手', in_progress: '作業中', completed: '完了' };

/** 依頼と発言はタグで囲み、中に書かれた指示ではなく材料として読ませる。 */
const promptOf = (instructions: string, blocks: Record<string, string | undefined>): string => {
  const tagged = Object.entries(blocks).flatMap(([tag, text]) => (text ? [`<${tag}>\n${text.slice(0, MAX_BLOCK_CHARS)}\n</${tag}>`] : []));
  return [instructions, ...tagged].join('\n\n');
};

/** 1 回きりのセッションでは、最初と最後の依頼が同じなので片方だけ渡す。 */
const laterPrompt = (c: Conversation): string | undefined => (c.lastPrompt !== c.firstPrompt ? c.lastPrompt : undefined);

const summaryKey = (id: string): string => `summary:${id}`;

const summaryPrompt = (c: Conversation): string => {
  const tasks = c.tasks.map((t) => `- [${TASK_TEXT[t.status]}] ${t.subject}`).join('\n');
  return promptOf(SUMMARY_INSTRUCTIONS, {
    theme: c.theme, session_name: c.name, status: STATUS_TEXT[c.status], first_prompt: c.firstPrompt, last_prompt: laterPrompt(c), tasks, last_message: c.lastMessage,
  });
};

const loadSummary = (prompt: string) => async (): Promise<SessionSummary> => {
  const { doing, now } = await askHaiku(prompt, objectSchema({ doing: { type: 'string' }, now: { type: 'string' } }));
  if (typeof doing !== 'string' || typeof now !== 'string' || !doing.trim() || !now.trim()) throw new UpstreamError('概要が返りませんでした');
  return { doing: doing.trim(), now: now.trim() };
};

/**
 * セッションごとに最後の概要を覚え、材料が同じなら作り直さない。失敗したときは、次に頼まれたときに作り直す。
 * やり取りが進んだら、前の概要に stale を付けてすぐ返し、裏でまとめ直す。
 */
export const summarize = async (c: Conversation): Promise<SessionSummary> => {
  const prompt = summaryPrompt(c);
  const key = summaryKey(c.id);
  const summary = await cached(key, { input: prompt }, loadSummary(prompt));
  return refreshing(key, prompt) ? { ...summary, stale: true } : summary;
};

let listSummaries = 0;
const listWaiters: (() => void)[] = [];

/** 枠が空くまで待ってから load を動かす。終えたら、待っている次の読み込みに枠を渡す。 */
const oneOfFew = <T,>(load: () => Promise<T>) => async (): Promise<T> => {
  if (listSummaries < MAX_LIST_SUMMARIES) listSummaries++;
  else await new Promise<void>((resolve) => listWaiters.push(resolve));
  try {
    return await load();
  } finally {
    const next = listWaiters.shift();
    if (next) next();
    else listSummaries--;
  }
};

/**
 * 一覧のカードに出す概要。覚えた概要をすぐ返し、無いか古ければ裏で作る。
 * 作業中は発言が次々に変わり、一覧のすべてのセッションで作り直すと haiku を何本も動かし続けるので、前に止まったときの概要を返す。止まったら作り直す。
 */
export const peekSummary = (c: Conversation): SessionSummary | undefined => {
  const prompt = summaryPrompt(c);
  return peekCached(summaryKey(c.id), { input: prompt, keep: c.status === 'busy' }, oneOfFew(loadSummary(prompt)));
};

/** 返答の候補や名前は、概要を作れなくても作る。 */
export const summaryText = async (c: Conversation): Promise<string | undefined> => {
  const s = await summarize(c).catch(() => undefined);
  return s && `やっていること: ${s.doing}\nいま: ${s.now}`;
};

/**
 * 同じ発言には作り直さない。前の発言への候補は役に立たないので、発言が変わったら作り終えるのを待つ。
 * 概要はまとめ直す途中の前のものでも材料として足りるので、概要が変わっただけでは作り直さない。
 */
export const suggestReplies = (c: Conversation): Promise<string[]> =>
  cached(`replies:${c.id}`, { input: JSON.stringify([c.theme, c.name, c.lastPrompt, c.lastMessage]), wait: true }, async () => {
    const prompt = promptOf(REPLY_INSTRUCTIONS, { theme: c.theme, session_name: c.name, summary: await summaryText(c), last_prompt: c.lastPrompt, last_message: c.lastMessage });
    const { replies: list } = await askHaiku(prompt, objectSchema({ replies: { type: 'array', items: { type: 'string' } } }));
    if (!Array.isArray(list)) throw new UpstreamError('候補が返りませんでした');
    return list
      .filter((r): r is string => typeof r === 'string' && r.trim() !== '')
      .map((r) => r.trim())
      .slice(0, MAX_REPLIES);
  });

/** 頼まれるたびに作り直す。気に入らない名前が出たら、もう一度頼めるようにするため。 */
export const suggestTitle = async (c: Conversation): Promise<string> => {
  const prompt = promptOf(TITLE_INSTRUCTIONS, { theme: c.theme, summary: await summaryText(c), first_prompt: c.firstPrompt, last_prompt: laterPrompt(c), last_message: c.lastMessage });
  const { title } = await askHaiku(prompt, objectSchema({ title: { type: 'string' } }));
  if (typeof title !== 'string' || !title.trim()) throw new UpstreamError('名前が返りませんでした');
  return title.trim();
};
