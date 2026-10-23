// サーバーとブラウザで共有する API の型。

/** 止まったセッションは、Claude が応答を返し終えていれば done、返す前なら interrupted。アーカイブすると ended になる。 */
export type SessionStatus = 'busy' | 'idle' | 'done' | 'interrupted' | 'ended';

/**
 * 入力待ちのセッションが待っているもの。human は人間の回答（回答待ち）、work は自分で動かしたコマンドやサブエージェント（実行中）、
 * event は CI やほかの人の返事などの外の出来事（待機中）。
 */
export type WaitingFor = 'human' | 'work' | 'event';

/** セッション内で Claude が TaskCreate で作ったタスク。 */
export type TaskItem = {
  id: string;
  subject: string;
  activeForm?: string;
  status: 'pending' | 'in_progress' | 'completed';
  blockedBy: string[];
};

export type Subagent = {
  id: string;
  type: string;
  description: string;
  model?: string;
  running: boolean;
  outputTokens: number;
};

/** Claude がバックグラウンドで動かした shell と Monitor。セッションが止まると、終わっていないものも止まる。 */
export type BgTask = {
  id: string;
  kind: 'shell' | 'monitor';
  description: string;
  command: string;
  status: 'running' | 'completed' | 'failed' | 'killed' | 'stopped';
  summary?: string;
  startedAt?: number;
};

export type PrInfo = {
  url: string;
  number: number;
  title: string;
  state: 'OPEN' | 'CLOSED' | 'MERGED';
  isDraft: boolean;
  /** APPROVED / CHANGES_REQUESTED / REVIEW_REQUIRED / 空文字 */
  review: string;
  checks: { passed: number; failed: number; pending: number };
  baseRef?: string;
  /** 人間がバッジに登録した PR だけが持つ。PR コメントを分析するとき、このブランチを開いた作業ツリーを探す。 */
  headRef?: string;
};

/** GitHub の Issue。# の補完と同じく、予定しないものか重複として閉じたものを Canceled と呼ぶ。 */
export type IssueInfo = { url: string; number: number; title: string; state: 'Open' | 'Closed' | 'Canceled' };

/** PR そのものを指す URL。サーバーは gh の引数に渡すので、- で始まる値やほかのページを通さない。 */
export const PR_URL = /^https:\/\/[^/\s]+\/[^/\s]+\/[^/\s]+\/pull\/\d+$/;

/** PR の会話のコメントとレビュー。state はレビューの結果（APPROVED など）で、コメントには無い。at は epoch ミリ秒。 */
export type PrPost = { author: string; body: string; at: number; state?: string };

/** PR の画面に出す中身。files は変更したファイルと行数。posts は会話を古い順に並べる。 */
export type PrDetail = PrInfo & {
  author: string;
  body: string;
  headRef: string;
  additions: number;
  deletions: number;
  files: { path: string; additions: number; deletions: number }[];
  posts: PrPost[];
  updatedAt: number;
};

/** PR のレビューコメント。at は epoch ミリ秒。 */
export type PrComment = { id: string; author: string; body: string; url: string; at: number };

/**
 * PR の未解決のレビュースレッド。comments の先頭が指摘で、続きは返信。diffHunk は、指摘を付けたときの差分。
 * 指摘の後にコードが変わって行を指せなくなると outdated になり、line は無くなる。side は、line が差分の新しい側と古い側のどちらの行番号か。
 */
export type PrThread = { id: string; path: string; line?: number; side: 'old' | 'new'; outdated: boolean; diffHunk: string; comments: PrComment[] };

/** レビューのスレッドに添える、指摘の説明（explanation）、事実確認（factCheck）、対応案（proposal）。 */
export type PrThreadNote = { explanation: string; factCheck: string; proposal: string };

/** notes はスレッドの id ごと。failed は、作れなかったスレッドがあるときの理由。 */
export type PrNotes = { notes: Record<string, PrThreadNote>; failed?: string };

/** assignee から下は、JQL で探した課題だけが持つ。updatedAt は epoch ミリ秒で、dueDate は JIRA のとおり YYYY-MM-DD。 */
/** GitHub Inbox の一覧に出す PR。repo は owner/name。 */
export type InboxPr = { url: string; repo: string; number: number; title: string; author: string; updatedAt: number };
/** GitHub の PR のダッシュボード（github.com/pulls）の Inbox と同じ区分。 */
export type InboxSection = { title: string; prs: InboxPr[] };

export type JiraIssue = {
  key: string;
  url: string;
  summary?: string;
  status?: string;
  statusCategory?: string;
  assignee?: string;
  updatedAt?: number;
  priority?: string;
  parent?: { key: string; summary?: string };
  labels?: string[];
  reporter?: string;
  dueDate?: string;
};

/** 課題のキーとして通す形。サーバーは JIRA の REST の URL に埋めるので、ほかの文字を通さない。 */
export const JIRA_ISSUE_KEY = /^[A-Z][A-Z0-9_]*-\d+$/;

/** 課題を今のステータスから移すトランジション。name はトランジションの名前で、status と statusCategory は移った後のステータス。 */
export type JiraTransition = { id: string; name: string; status: string; statusCategory?: string };

/** 課題の画面に出す中身。description とコメントの本文は、JIRA の形式から Markdown として読める文字にしたもの。transitions は、読んだときに移せたステータス。 */
export type JiraDetail = JiraIssue & { description: string; comments: PrPost[]; children: JiraIssue[]; transitions: JiraTransition[] };

/** Notion のページの画面に出す中身。id はハイフンの無い 32 桁で、markdown は本文のブロックを Markdown にしたもので、データベース（database が true）では行の表。truncated は、長くて途中までしか読んでいないとき。 */
export type NotionPage = { id: string; url: string; title: string; icon?: string; updatedAt?: number; markdown: string; truncated: boolean; database?: boolean };

const NOTION_HOST = /(^|\.)(notion\.so|notion\.com|notion\.site)$/;

/**
 * Notion のページの URL から、ハイフンの無い 32 桁の ID を取り出す。Notion の URL でないものと、ページを指さないものは undefined にする。
 * データベースの中のページを横に開いた URL は、p にページの ID を持つので、そちらを取る。
 */
export const notionPageId = (u: URL): string | undefined => {
  if (u.protocol !== 'https:' || !NOTION_HOST.test(u.hostname)) return undefined;
  const last = u.searchParams.get('p') ?? u.pathname.split('/').findLast(Boolean) ?? '';
  return /[0-9a-f]{32}$/i.exec(last.replaceAll('-', ''))?.[0].toLowerCase();
};

/** JIRA の検索が 1 回で返せる件数の上限。 */
export const JIRA_MAX_RESULTS = 100;

export type TokenTotals = { input: number; output: number; cacheRead: number; cacheCreation: number };

export type Usage = {
  model?: string;
  /** statusline 由来。statusline を描いたことのないセッションでは分からない。 */
  effort?: string;
  contextTokens: number;
  contextWindow: number;
  contextPercent: number;
  /** statusline 由来の正確な値なら true。会話ログとモデル既定値からの推定なら false。 */
  exact: boolean;
  /** メインスレッドとサブエージェントの合計。 */
  totals: TokenTotals;
  costUsd?: number;
  /** [epoch ms, コンテキストのトークン数] */
  timeline: [number, number][];
};

/** 人間がセッションの資料に足したリンク。題名が空なら、画面が URL から付ける。at は足した時刻。 */
export type SavedLink = { url: string; title: string; at: number };

/** 用語集の 1 語。description は、その語が何かを書いた 1 文。 */
export type GlossaryTerm = { term: string; description: string };

/** 人間がセッションの用語集に足した語。説明を書かずに足すと、haiku がやり取りから書いた説明が入る。at は足した時刻。 */
export type SavedTerm = GlossaryTerm & { at: number };

/** セッションの用語集。pending は、まだ拾っていないやり取りから、裏で用語を拾っている印。error は、やり取りの一部から用語を拾えなかった理由。 */
export type Glossary = { terms: GlossaryTerm[]; pending?: boolean; error?: string };

/** 人間が Claude に送る返答の候補。pending は、最後の発言への候補を裏で作っている印。error は、候補を作れなかった理由。 */
export type Replies = { replies: string[]; pending?: boolean; error?: string };

export type Session = {
  id: string;
  name: string;
  theme: string;
  repo: string;
  cwd: string;
  branch?: string;
  status: SessionStatus;
  kind?: string;
  /** ダッシュボードから止められる。動いているバックグラウンドのセッションと、巻き戻して送ったターンを動かしている claude -p。前に受け取ったスナップショットには無いことがある。 */
  stoppable?: boolean;
  /** claude.ai/code でこのセッションを開く URL（Remote Control） */
  remoteUrl?: string;
  /** バックグラウンドセッションをターミナルで開くコマンド */
  attachCommand?: string;
  startedAt?: number;
  updatedAt: number;
  lastPrompt?: string;
  lastMessage?: string;
  lastMessageAt?: number;
  tasks: TaskItem[];
  subagents: Subagent[];
  background: BgTask[];
  /** 今のブランチの PR。 */
  pr?: PrInfo;
  /** これまでに居たブランチの PR と、人間がバッジに登録した PR。今のブランチの PR を先頭に、最後に居た順に並べ、登録した PR を後に続ける。外したバッジは含めない。 */
  prs: PrInfo[];
  /** PR があるかを gh でまだ読み終えていないブランチがあり、prs に足りない PR があるかもしれない。前に受け取ったスナップショットには無いことがある。 */
  prsPending?: boolean;
  jira: JiraIssue[];
  /** 人間がバッジに登録した GitHub の Issue。前に受け取ったスナップショットには無いことがある。 */
  issues?: IssueInfo[];
  usage: Usage;
  archived: boolean;
  /** 人間が作業中にアーカイブした。入力待ちか停止になったら、サーバーがアーカイブする。 */
  archiveWhenIdle?: boolean;
  /** アーカイブしたが、Claude Code の一覧から消せなかった理由。 */
  removeError?: string;
  /** 人間がピン留めした。一覧の先頭にまとめて出す。 */
  pinned: boolean;
  links: SavedLink[];
  /** 人間がエクスプローラーから資料に足したファイル。 */
  files: SavedFile[];
  /** Claude が MCP の proofread で校正した文章。前に受け取ったスナップショットには無いことがある。 */
  proofFiles?: SavedProof[];
  /** 人間がほかのセッションの資料から足した present の資料。前に受け取ったスナップショットには無いことがある。 */
  sharedDocs?: SavedDoc[];
  /** 人間が資料のウィジェットで並べ替えた順と、外した資料。present の資料は id、リンクは URL、ファイルはパス、校正した文章は文章校正アプリのファイルの id で持つ。 */
  docOrder: string[];
  hiddenDocs: string[];
  /** 人間が資料のウィジェットで付け直した題名。キーは docOrder と同じ。 */
  docTitles: Record<string, string>;
  /** 人間が資料のウィジェットでピン留めした資料。ほかの資料より上に出す。キーは docOrder と同じ。 */
  pinnedDocs: string[];
  /** 人間が用語集に足した語と、外した語。外した語は比べるときの形で持つ。前に受け取ったスナップショットには無いことがある。 */
  terms?: SavedTerm[];
  hiddenTerms?: string[];
  /** 人間がチャットでブックマークした発言。前に受け取ったスナップショットには無いことがある。 */
  bookmarks?: Bookmark[];
  /** このセッションが claude.ai に公開した artifact。 */
  artifacts: Artifact[];
  /** 未配信の人間から Claude へのメッセージ数 */
  pendingMessages: number;
  /** request_review で Claude がレビューを待っている */
  awaitingReview: boolean;
  /** Claude がレビューを頼んだコミットの範囲 */
  reviewRange?: DiffRange;
  /** Claude が show_branches で並べた、コードの表示の選択肢。先頭のものを開く。 */
  branchViews?: BranchView[];
  /** 一覧のカードに出す概要。まだ作っていないセッションと、やり取りの無いセッションには無い。 */
  summary?: SessionSummary;
  /** アーカイブしていない入力待ちのセッションだけが持つ。最後の発言を分類し終えるまでは無い。 */
  waitingFor?: WaitingFor;
  /** スリープや通信の途切れで応答の途中で止まり、まだ続けていない。アーカイブしていない入力待ちのセッションだけが持つ。 */
  offline?: boolean;
  /** 「キュー」で保留しているメッセージ。古い順に並べる。前に受け取ったスナップショットには無いことがある。 */
  queued?: QueuedMessage[];
};

/**
 * ask_human か AskUserQuestion で Claude が尋ねた質問。
 * header はタブに出す短い名前で、multiSelect は選択肢をいくつでも選べる質問。
 * descriptions は AskUserQuestion の選択肢に添えた説明で、options と同じ順に並ぶ。説明の無い選択肢は空文字にする。
 */
export type AskedQuestion = { question: string; header?: string; options: string[]; descriptions?: string[]; multiSelect?: boolean };

/**
 * 人間の対応が必要なもの。
 * ask: ask_human の質問、question: Claude Code の AskUserQuestion の質問、review: request_review の依頼、permission: 権限確認ダイアログ、
 * reply: 回答待ちになったセッションの最後の発言、pr: CI 失敗や変更要求、doc: 未読の資料。
 */
export type AttentionKind = 'ask' | 'question' | 'review' | 'permission' | 'reply' | 'pr' | 'doc';

export type Attention = {
  id: string;
  kind: AttentionKind;
  sessionId: string;
  title: string;
  body?: string;
  options?: string[];
  /** options に添えた説明。AskUserQuestion の質問だけが持ち、options と同じ順に並ぶ。 */
  descriptions?: string[];
  /** 分けて尋ねた質問。人間は質問ごとに選択肢を選ぶ。 */
  questions?: AskedQuestion[];
  docId?: string;
  url?: string;
  createdAt: number;
};

export type DocFormat = 'markdown' | 'html' | 'mermaid' | 'svg' | 'text';

export type DocMeta = {
  id: string;
  sessionId: string;
  title: string;
  format: DocFormat;
  /** ファイルを指す資料は、開くたびに最新の内容を読む。 */
  path?: string;
  createdAt: number;
  seen: boolean;
};

export type Doc = DocMeta & { content: string };

export type OutboxMessage = {
  id: string;
  sessionId: string;
  text: string;
  createdAt: number;
  deliveredAt?: number;
  /** hook はフックで差し込んで届けたもの。state.json に残っている記録を表示するために持つ。reply は supervisor に返信を頼んで届けたもの。 */
  via?: 'socket' | 'resume' | 'reply' | 'mcp' | 'manual' | 'hook';
  /** 届けられなかった理由。未配信に戻っている。 */
  error?: string;
  /** 届けている途中。サーバーが途中で止まると付いたまま残り、次に起動したときに届け直す。 */
  sending?: boolean;
};

/** 人間が「キュー」で保留したメッセージと、作業中のバックグラウンドのセッションへ送ったメッセージ。Claude が入力待ちで質問していないときに、古い順に 1 つずつ送る。 */
export type QueuedMessage = {
  id: string;
  sessionId: string;
  text: string;
  createdAt: number;
  /** 人間が書き直している間は送らない。 */
  editing?: boolean;
};

/** 使用率が変わった時点。at は epoch ミリ秒 */
export type RatePoint = [at: number, usedPercent: number];

/** resetsAt は epoch ミリ秒。points は、今の枠の中で使用率が変わった時点を古い順に並べたもの。 */
export type RateLimit = { usedPercent: number; resetsAt?: number; points?: RatePoint[] };
export type RateLimits = { fiveHour?: RateLimit; sevenDay?: RateLimit; updatedAt: number };

/**
 * macOS のメモリの状態。level はカーネルが判定したメモリプレッシャーの段階で、アクティビティモニタのグラフの色（緑、黄、赤）に当たる。
 * percent は、カーネルがすぐ使えるとみなすメモリの割合を 100 から引いたもの。ほかはバイト数。
 */
export type MemoryPressure = {
  kind: 'pressure';
  level: 'normal' | 'warn' | 'critical';
  percent: number;
  total: number;
  app: number;
  wired: number;
  compressed: number;
  cached: number;
  swapUsed: number;
};

/**
 * Windows のメモリの状態。percent は物理メモリの使用率で、タスクマネージャーの「メモリ」の割合に当たる。
 * Windows は段階を判定しないので、level は使用率から決める。ほかはバイト数。
 */
export type MemoryUsage = {
  kind: 'usage';
  level: 'normal' | 'warn' | 'critical';
  percent: number;
  total: number;
  used: number;
  available: number;
};

export type MemoryStatus = MemoryPressure | MemoryUsage;

/** 画面から始めるセッションと、改善要望を実装するセッションに選べるモデルと effort。どちらも空なら Claude Code の設定に従う。 */
export const MODELS = ['fable', 'opus', 'sonnet'] as const;
export const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max'] as const;

export type ImproveSettings = { enabled: boolean; model?: string; effort?: string };

export type ImproveState = ImproveSettings & {
  /** 改善のセッションが動いている */
  running: boolean;
  sessionId?: string;
  /** 対応中の要望の書き出し */
  current?: string;
  /** ウェーブが使用制限で止まっているときの、制限が明ける時刻（epoch ミリ秒）。明けたら同じセッションで続ける。 */
  limitResetsAt?: number;
  /** 対応中のものを除いた、未対応の要望の数 */
  pending: number;
  /** 待機している理由。新しい要望が届くか、人間が続けると消える。 */
  error?: string;
};

/** Claude が authenticate で求め、人間に確かめている認証。command か url のどちらかを持つ。 */
export type AuthRequest = {
  id: string;
  /** 何の認証か（例: Google Cloud の認証、MCP サーバー「notion」の認証、example.com のブラウザ認証） */
  title: string;
  /** 人間が OK を押すと動かす、認証のコマンド */
  command?: string;
  /** ブラウザ認証で、人間が「進む」を押すとブラウザで開くログイン画面の URL */
  url?: string;
  /** 認証を求めたセッション。求めた順。 */
  sessionIds: string[];
};

export type Snapshot = {
  now: number;
  sessions: Session[];
  attention: Attention[];
  docs: DocMeta[];
  outbox: OutboxMessage[];
  rateLimits?: RateLimits;
  /** テーマの名前。人間がホームで並べ替えた順で、並べ替えた後に現れたテーマは、並べ替えたものより前に名前の順で入る。 */
  themes: string[];
  /** 人間がホームで並べ替えたセッションの順。並べ替えた後に現れたセッションは入っていない。 */
  sessionOrder: string[];
  improve: ImproveState;
  /** 自動継続モード。スリープや通信の途切れで止まったセッションを、確認のダイアログを出さずに続けさせる。前に受け取ったスナップショットには無いことがある。 */
  autoContinue?: boolean;
  /** 人間に確かめている認証。求めた順。前に受け取ったスナップショットには無いことがある。 */
  auth?: AuthRequest[];
  /** 人間が「更新」で JIRA の情報を取り直させた時刻。JIRA の一覧のウィジェットは、変わったら探し直す。 */
  jiraRefreshedAt: number;
  /** サーバーが外部の情報源から裏で読み直した値が変わるたびに増える。画面は、変わったらサーバーのキャッシュを通す API を取り直す。前に受け取ったスナップショットには無いことがある。 */
  cacheVersion?: number;
  /** 課題の画面の URL の前に付く部分。JIRA を設定していなければ無い。画面は、ここで始まる課題へのリンクをダッシュボードで開く。 */
  jiraBase?: string;
  /** Notion のトークンを設定してあるか。画面は、設定してあるときだけ、Notion のページへのリンクをダッシュボードで開く。 */
  notion?: boolean;
  /** Jev のキーを設定してあるか。画面は、設定してあるときだけ、イルカの吹き出しにマインスイーパのヒントを出す。 */
  jev?: boolean;
  /** Claude が MCP で文章校正のファイルにコメントか修正版を書き込んだ時刻を、ファイルの id ごとに持つ。画面が前に受け取ったスナップショットから描き始めるときは無い。 */
  proofCommentedAt?: Record<string, number>;
  /** 画面のファイルの更新時刻。変わったら画面が読み込み直す。 */
  assets: { js: number; css: number };
};

export type DiffLine = { kind: 'ctx' | 'add' | 'del'; oldNo?: number; newNo?: number; text: string };
export type DiffHunk = { header: string; lines: DiffLine[] };

export type DiffFile = {
  path: string;
  oldPath?: string;
  /** unchanged は、差分に無いファイルをコードへのリンクで開いたもの。全体を文脈の行として持つ。 */
  status: 'added' | 'deleted' | 'modified' | 'renamed' | 'unchanged';
  binary: boolean;
  additions: number;
  deletions: number;
  /** このセッションの Edit / Write が触ったファイル */
  editedBySession: boolean;
  hunks: DiffHunk[];
};

/** 範囲の端で、未コミットの変更を指す値。コミットの sha と並べて使う。 */
export const WORKTREE = 'worktree';

/** ウィンドウに浮かべて見せる画像と動画の、拡張子ごとの MIME タイプ。 */
export const MEDIA_TYPES: Record<string, string> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.mp4': 'video/mp4',
  '.webm': 'video/webm',
  '.mov': 'video/quicktime',
};

/** エクスプローラーに並べる 1 件。path は区切りを / にそろえた絶対パス。size はファイルのバイト数で、フォルダーには付けない。 */
export type ExplorerEntry = { name: string; path: string; dir: boolean; size?: number; modifiedAt?: number };

/**
 * エクスプローラーで開いたフォルダー。parent は 1 つ上のフォルダーで、最上位には無い。home はホームディレクトリ、roots は最上位の場所（Windows ではドライブ）。
 * truncated は、中身が多くて途中までしか並べていないとき。
 */
export type ExplorerDir = { path: string; parent?: string; home: string; roots: string[]; entries: ExplorerEntry[]; truncated: boolean };

/** メモ帳でファイルを上書きした結果。modifiedAt はファイルの今の更新時刻。changed は、読んだ後にほかで書き換えられていて、上書きしなかったとき。 */
export type TextSaveResult = { modifiedAt: number; changed?: boolean };

/** 資料のウィジェットで、資料を外したり、ピン留めを切り替えたり、名前を変えたりした結果。at はサーバーが済ませた時刻で、画面は、この時刻より後に作ったスナップショットが届くまで、手元で済ませた状態を出す。 */
export type DocEditResult = { at: number };

/** 人間が資料に足したファイル。path は絶対パス。 */
export type SavedFile = { path: string; at: number };

/** Claude が校正した、文章校正アプリのファイル。title は校正したときのファイルの名前。 */
export type SavedProof = { id: string; title: string; at: number };

/** 人間がほかのセッションの資料から足した、present の資料。at は足した時刻。 */
export type SavedDoc = { id: string; at: number };

/** Claude が claude.ai に公開した artifact。path は公開した元のファイルで、claude.ai の画面は埋め込めないので、中身はこのファイルから読む。 */
export type Artifact = { url: string; title: string; path?: string };

/** チャットのファイルパスの開き先。repo はコードの画面、local はウィンドウに浮かべる。 */
export type FileWhere = 'repo' | 'local';

/** ファイルの開き先。heading は Markdown の最初の見出しで、資料の題名に添える。path は、ウィンドウに浮かべるファイルの絶対パス。 */
export type FileTarget = { where: FileWhere; heading?: string; path?: string };

/** 差分に含めるコミットの範囲。from が最も古いコミット、to が最も新しいコミットで、どちらも含める。 */
export type DiffRange = { from: string; to: string };

/** コードに表示するブランチ。branch の、base との分岐点から先の差分を出す。label は選択肢に出す名前。 */
export type BranchView = { branch: string; base?: string; label?: string };

export type CommitInfo = { sha: string; subject: string };

/** commits は範囲を選ぶための候補で、新しい順。 */
export type DiffResult = { range: DiffRange; commits: CommitInfo[]; files: DiffFile[]; truncated: boolean };

export type ReviewComment = { path: string; line: number; side: 'old' | 'new'; lineText: string; body: string };

/** queued は、作業中のバックグラウンドのセッションへ送ったので保留し、入力待ちになったら送るもの。 */
export type Delivery = 'mcp' | 'sent' | 'resumed' | 'queued';

/** メッセージ欄で / から補完する skill。description は説明の書き出し。 */
export type SkillInfo = { name: string; description: string };

/** メッセージ欄で # から補完する PR と Issue。state は候補に添える状態で、Draft と Merged は PR だけ、Canceled は Issue だけが持つ。mine は gh で認証した人が作ったもの。createdAt は epoch ミリ秒。 */
export type RepoRef = { number: number; title: string; kind: 'pr' | 'issue'; state: 'Draft' | 'Open' | 'Merged' | 'Closed' | 'Canceled'; createdAt: number; mine: boolean };

/** 会話の 1 件。prompt は人間の依頼（ダッシュボードから届いたものを含む）、message は Claude の発言。at は epoch ミリ秒。 */
/** tool は Claude が呼んだツールで、text は名前と入力の要点を 1 行にしたもの、detail は入力の全体。 */
/** rewindTo は、依頼を書き直して送るときに巻き戻す先の、会話ログのエントリの uuid。依頼の直前のエントリを指す。 */
export type ChatEntry = { kind: 'prompt' | 'message' | 'tool'; text: string; at?: number; detail?: string; rewindTo?: string };

/** 人間がチャットでブックマークした発言。at は発言の時刻、text は一覧に出す本文の先頭、createdAt はブックマークした時刻。 */
export type Bookmark = { kind: 'prompt' | 'message'; at: number; text: string; createdAt: number };

/** 会話の中の発言をブックマークと照らし合わせる鍵。発言の時刻はミリ秒まで持つので、種類と合わせれば発言を見分けられる。 */
export const bookmarkKey = (e: { kind: string; at?: number }): string => `${e.kind}:${e.at}`;

/** 過去のセッションの検索で当たった箇所。text は、当たった語の前後を切り出したもの。 */
export type SearchHit = { kind: 'title' | 'prompt' | 'message'; text: string; at?: number };

export type SearchResult = {
  id: string;
  cwd: string;
  repo: string;
  title: string;
  updatedAt: number;
  /** 会話の先頭から数件だけ。全体の数は hitCount。 */
  hits: SearchHit[];
  hitCount: number;
};

/** results は新しい順に上限まで。total は当たったセッションの数。 */
export type SearchResponse = { results: SearchResult[]; total: number };

/** セッションの概要。doing は全体で取り組んでいること、now はいまの状況。stale は、まとめ直している間に返した前のやり取りの概要の印。 */
export type SessionSummary = { doing: string; now: string; stale?: boolean };

/** イルカの吹き出しでの会話。dolphin はイルカの答え。 */
export type AssistantMessage = { role: 'human' | 'dolphin'; text: string };

/** links は、答えに関係するダッシュボードの画面（#/ から始まる）。 */
export type AssistantReply = { answer: string; links: { label: string; href: string }[] };

/** 文章校正の評価軸。順に、指す先、疑問の回収、指す先までの距離と係り方、文の役割、前の文とのつながり、文章の主張を支えているか、語の選び方、1 文の負担。 */
export type ProofAxis = '参照' | '回収' | '近接' | '骨組み' | '連結' | '貢献' | '語' | '負荷';

/** 文章校正で文に付ける指摘。fail は読み進めを妨げるもの、note は気にしてほしいもの。quote は文の中の語句で、suggestions は言い換えの候補。 */
export type ProofFinding = { axis: ProofAxis; level: 'fail' | 'note'; quote?: string; message: string; suggestions?: string[] };

/**
 * 文章校正の読み手が、文ごとに答えたこと。指摘のもとになる。
 * refs は既知として書かれた語句と指す先で、antecedent は指す先の文の番号（1 から）、0 は用語集、-1 は一般常識、-2 は見つからない、-3 はこの後に書かれると示されている。
 * questions の wait は、この後に説明が続くと読み取れたか。premises は前の文とつなぐのに補った前提。
 */
export type ProofReading = {
  refs: { quote: string; antecedent: number }[];
  questions: { quote: string; question: string; kind: 'definition' | 'detail' | 'reason' | 'background' | 'prediction'; wait: boolean }[];
  role: { kind: 'frame' | 'fill' | 'shift' | 'unclear'; note: string };
  link: { relation: 'cause' | 'result' | 'example' | 'contrast' | 'addition' | 'restate' | 'shift' | 'none' | 'first'; marked: boolean };
  premises: { premise: string; source: 'common' | 'expert' }[];
  words: { word: string; issue: 'vague' | 'differs'; better: string[] }[];
  attachments: { quote: string; readings: string[] }[];
};

/** 文章全体の中で文が果たす役割。parent の文に対する役割で、claim は文章全体の主張、request は読み手への依頼や問い。 */
export type ProofRole =
  | 'claim' | 'reason' | 'detail' | 'example' | 'background' | 'condition' | 'step' | 'contrast' | 'effect' | 'request'
  | 'preview' | 'summary' | 'restate' | 'aside' | 'heading';

/**
 * 文章校正で、文章全体を読んで決めた主張と、文ごとの役割。sentences は文ごとで、役割が返らなかった文は null。
 * parent は、その文が説明したり根拠づけたりしている文の番号（1 から）で、主張を述べた文と見出しは 0。
 * support は主張との関係で、0 は関係しない、1 は主張を支える文を補う、2 は主張を直接支える、3 は主張そのもの。unrelated は関係しない確率、fresh は前の文に無い情報がある確率。
 * 見出し、コードブロック、表の行と、確かめられなかった文には、support と unrelated と fresh が無い。
 * contribution は要約で役割から決めた寄与度（0〜3）で、要約した結果にだけある。
 */
export type ProofOutline = {
  claim: string;
  sentences: ({ role: ProofRole; parent: number; support?: number; unrelated?: number; fresh?: number; contribution?: number } | null)[];
};

/**
 * 文章校正の進み具合として、サーバーが 1 行に 1 つずつ送る出来事。
 * plan は文の位置。read はその文を読んだ結果。findings は、区切りや文書全体を見てから足す指摘。outline は、文章全体を読んで決めた主張と文の役割。
 * usage は、モデルに 1 回問い合わせるたびに送る、その問い合わせで使ったトークン数。model は tools と同じ名前にする。
 */
export type ProofEvent =
  | { type: 'plan'; sentences: { start: number; end: number }[]; tools: string[] }
  | { type: 'read'; i: number; reading: ProofReading; findings: ProofFinding[] }
  | { type: 'outline'; outline: ProofOutline }
  | { type: 'findings'; i: number; findings: ProofFinding[] }
  | { type: 'usage'; model: string; usage: TokenTotals }
  | { type: 'error'; i?: number; message: string }
  | { type: 'done' };

/**
 * 文章校正の 1 回分。text は校正を始めた時点の文章で、文の位置はこれを指す。readings と errors は文ごとで、まだ読んでいない文は null。notices は全体の失敗。
 * outline は、文章全体を読み終えるまで無い。usage はモデルごとに使ったトークン数で、数えていない校正には無い。
 */
export type ProofRun = {
  text: string; sentences: { start: number; end: number }[]; tools: string[]; done: boolean;
  readings: (ProofReading | null)[]; findings: ProofFinding[][]; errors: (string | null)[]; notices: string[];
  outline?: ProofOutline;
  usage?: Record<string, TokenTotals>;
};

/** 文章校正の実行の種類。clarify は明確化で、初めて読む人が前から読み進められるかを調べる。summarize は要約で、文ごとに文章への寄与度を調べる。 */
export type ProofMode = 'clarify' | 'summarize';

/** 人間が指摘に付けたコメント。ignore なら、その指摘を数に入れず、修正版でも直さない。 */
export type ProofComment = { text: string; ignore: boolean };

/**
 * 文章校正の版。text は編集中の文章で、run は最後に明確化した結果、summary は最後に要約した結果。from は、どの版の指摘から作ったか。
 * fixed は人間が固定した文の本文で、その文の指摘は数に入れず、修正版でも一字も変えない。修正版にも引き継ぐ。
 * comments のキーは、指摘を見分ける文字列。
 */
export type ProofVersion = { text: string; from?: number; run?: ProofRun; summary?: ProofRun; fixed: string[]; comments: Record<string, ProofComment> };

/** 文章校正のファイル。版を重ねて文章を直す過程を残す。title は「最近使ったファイル」に出す名前。 */
export type ProofFile = { id: string; title: string; glossary: string; versions: ProofVersion[]; updatedAt: number };

export type ProofFileMeta = Pick<ProofFile, 'id' | 'title' | 'updatedAt'>;

/** 修正版を作るときに渡す指摘。i は文の番号（0 から）で、comment は人間がその指摘に付けたコメント。 */
export type ReviseNote = ProofFinding & { i: number; comment?: string };

/** 改善要望で、人間が画面上で指した要素。 */
export type FeedbackTarget = {
  /** body からの CSS セレクタ。クラス名から、描画しているコンポーネントを探せる。 */
  selector: string;
  /** 要素の文字。文字を持たない要素では、文字を持つ最も近い祖先の文字。 */
  text: string;
  /** outerHTML の先頭 */
  html: string;
  /** 選んだ時点のビューポート上の位置 */
  rect: { x: number; y: number; width: number; height: number };
  /** 選んだときの画面（location.hash） */
  route: string;
};

/** サーバーが動いている OS。version は「Windows 11 Home」のような OS の名前、release はカーネルの版。 */
export type HostOs = { platform: string; version: string; release: string; arch: string };

/** 改善要望を送ったときの画面の状態。 */
export type FeedbackContext = {
  url: string;
  viewport: { width: number; height: number; devicePixelRatio: number };
  colorScheme: 'light' | 'dark';
  userAgent: string;
  /** OS やブラウザが、動きを減らす設定になっているか。アニメーションはこれを見て止める。 */
  reducedMotion: boolean;
  /** 画面が表示していたデータ */
  snapshot: Snapshot | null;
};
