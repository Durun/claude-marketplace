// 外部の情報源: GitHub の PR（gh）、JIRA、git の差分。
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { promisify } from 'node:util';
import { type BranchView, type CommitInfo, type InboxPr, type InboxSection, type IssueInfo, type DiffFile, type DiffRange, type DiffResult, type JiraDetail, type JiraIssue, JIRA_ISSUE_KEY, JIRA_MAX_RESULTS, type PrDetail, PR_URL, type PrInfo, type PrPost, type PrThread, type RepoRef, type ReviewComment, WORKTREE } from '../shared/types.ts';
import { cached, forgetCached, peekCached } from './cache.ts';
import { readJson } from './claude.ts';
import { changed } from './store.ts';

const run = promisify(execFile);
const MAX_BUFFER = 64 * 1024 * 1024;
const MAX_DIFF_CHARS = 4 * 1024 * 1024;
const MAX_UNTRACKED_FILES = 50;
const MAX_UNTRACKED_BYTES = 256 * 1024;
const FULL_CONTEXT_LINES = 1_000_000;
const COMMIT_LIMIT = 30;
const COMMIT_FORMAT = '--format=%H%x1f%s';
// 何も無いツリー。最初のコミットから範囲に含めるときの比較元にする。
const EMPTY_TREE = '4b825dc642cb6eb9a060e54bf8d69288fbee4904';

// ベースブランチ上のセッションと、git 管理外やデタッチ状態（HEAD）のセッションは PR を持たないので探さない。
const BASE_BRANCH = /^(HEAD|main|master|develop|release\/.*)$/;
const PR_FIELDS = 'url,number,title,state,isDraft,reviewDecision,statusCheckRollup,baseRefName';
const FAILED = new Set(['FAILURE', 'TIMED_OUT', 'CANCELLED', 'ACTION_REQUIRED', 'STARTUP_FAILURE', 'ERROR']);

type GhCheck = { status?: string; conclusion?: string; state?: string };
type GhPr = {
  url: string;
  number: number;
  title: string;
  state: PrInfo['state'];
  isDraft: boolean;
  reviewDecision?: string;
  statusCheckRollup?: GhCheck[];
  baseRefName?: string;
};

/** CheckRun は status と conclusion、StatusContext は state で結果を表す。 */
export const countChecks = (rollup: GhCheck[]): PrInfo['checks'] => {
  const checks = { passed: 0, failed: 0, pending: 0 };
  for (const c of rollup) {
    const v = c.conclusion || c.state || '';
    if (c.status && c.status !== 'COMPLETED') checks.pending++;
    else if (FAILED.has(v)) checks.failed++;
    else if (v === 'PENDING' || v === 'EXPECTED') checks.pending++;
    else checks.passed++;
  }
  return checks;
};

const toPrInfo = (pr: GhPr): PrInfo => ({
  url: pr.url,
  number: pr.number,
  title: pr.title,
  state: pr.state,
  isDraft: pr.isDraft,
  review: pr.reviewDecision ?? '',
  checks: countChecks(pr.statusCheckRollup ?? []),
  baseRef: pr.baseRefName,
});

// カードに出すブランチの PR は、すべてのセッションのブランチの分を読むので、gh を呼びすぎないよう間隔を空けて読み直す。
const BRANCH_PR_MAX_AGE = 10 * 60_000;
// PR の画面は、開いている間に CI やレビューが進むので、短い間隔で読み直す。
const PR_MAX_AGE = 30_000;
// GitHub の Inbox は検索に数秒かかるので、PR の画面より間隔を空ける。
const INBOX_MAX_AGE = 60_000;

/** null は「PR が無い」ことを確認済みの印。gh 未認証や GitHub 以外のリポジトリでも null にして、PR を出さない。 */
const findPr = async (cwd: string, branch: string): Promise<PrInfo | null> => {
  try {
    const { stdout } = await run('gh', ['pr', 'list', '--head', branch, '--state', 'all', '--limit', '1', '--json', PR_FIELDS], { cwd });
    const [pr] = JSON.parse(stdout) as GhPr[];
    return pr ? toPrInfo(pr) : null;
  } catch (e) {
    console.error(`gh pr list failed in ${cwd}: ${(e as Error).message.split('\n')[0]}`);
    return null;
  }
};

// ブランチ名は空白を含まないので、ディレクトリとは空白で区切る。
const branchKey = (cwd: string, branch: string): string => `pr-branch:${cwd} ${branch}`;

/** PR を作ったりマージしたりしたブランチの PR を、覚えた値が新しくても読み直す。 */
export const refreshPr = async (cwd: string, branch: string): Promise<void> => {
  if (!BASE_BRANCH.test(branch)) await cached(branchKey(cwd, branch), { fresh: true }, () => findPr(cwd, branch));
};

type ClaudePrStatus = { title: string; state: PrInfo['state']; checks: PrInfo['checks']; review: string };

/** Claude Code が実行中セッションの PR について書き残した状態。URL ごとに、gh で読んだ状態より優先する。 */
let claudePrStatuses: Partial<Record<string, ClaudePrStatus>> = {};

const withClaudeStatus = (pr: PrInfo | null | undefined): PrInfo | undefined => {
  const s = pr && claudePrStatuses[pr.url];
  if (!pr || !s) return pr ?? undefined;
  return { ...pr, title: s.title, state: s.state, checks: s.checks, review: s.review ?? pr.review };
};

const peekBranchPr = (cwd: string, branch: string): PrInfo | null | undefined =>
  peekCached(branchKey(cwd, branch), { maxAge: BRANCH_PR_MAX_AGE }, () => findPr(cwd, branch));

/** 覚えた PR を返す。初めてのブランチと古くなったものは裏で読み、変わったら画面に知らせる。 */
export const cachedPr = (cwd: string, branch: string | undefined): PrInfo | undefined => {
  if (!branch || BASE_BRANCH.test(branch)) return undefined;
  return withClaudeStatus(peekBranchPr(cwd, branch));
};

/** ブランチの PR を、まだ一度も読み終えていないか。findPr は PR が無くても null を返して覚えるので、覚えた値が無ければ読んでいる途中である。 */
export const prPending = (cwd: string, branch: string): boolean => !BASE_BRANCH.test(branch) && peekBranchPr(cwd, branch) === undefined;

// バッジに登録した PR と Issue も、すべてのセッションの分を読むので、ブランチの PR と同じ間隔で読み直す。
const BADGE_POLICY = { maxAge: BRANCH_PR_MAX_AGE };

const readBadgePr = async (url: string): Promise<PrInfo> => {
  const pr = JSON.parse(await gh(['pr', 'view', url, '--json', `${PR_FIELDS},headRefName`])) as GhPr & { headRefName: string };
  return { ...toPrInfo(pr), headRef: pr.headRefName };
};

type GhIssue = Pick<GhRef, 'number' | 'title' | 'state' | 'stateReason'> & { url: string };

const ghIssue = async (ref: string, cwd?: string): Promise<GhIssue> => JSON.parse(await gh(['issue', 'view', ref, '--json', 'url,number,title,state,stateReason'], undefined, cwd)) as GhIssue;

const toIssue = (url: string, issue: GhIssue): IssueInfo => ({ url, number: issue.number, title: issue.title, state: refState(issue) as IssueInfo['state'] });

const readIssue = async (url: string): Promise<IssueInfo> => toIssue(url, await ghIssue(url));

/**
 * 人間がバッジに登録しようとした PR と Issue を読み、登録する URL を返す。読めなければ投げ、登録させない。
 * GitHub は PR の番号の Issue の URL を PR へ送るので、gh が PR の URL を返したら PR として登録する。
 * ref は URL か、cwd のリポジトリの #番号。
 */
export const loadBadge = async (kind: 'pr' | 'issue', ref: string, cwd?: string): Promise<string> => {
  const issue = kind === 'issue' ? await ghIssue(ref, cwd) : undefined;
  const url = issue?.url ?? ref;
  if (issue && !PR_URL.test(url)) return (await cached(`issue-badge:${url}`, { ...BADGE_POLICY, fresh: true }, async () => toIssue(url, issue))).url;
  await cached(`pr-badge:${url}`, BADGE_POLICY, () => readBadgePr(url));
  return url;
};

export const badgePr = (url: string): PrInfo | undefined => withClaudeStatus(peekCached(`pr-badge:${url}`, BADGE_POLICY, () => readBadgePr(url)));

export const badgeIssue = (url: string): IssueInfo | undefined => peekCached(`issue-badge:${url}`, BADGE_POLICY, () => readIssue(url));

// 解決済みかどうかは REST の API では分からないので、GraphQL で読む。
// ponytail: スレッドは先頭の 100 件、コメントはスレッドごとに 50 件まで。超える PR が出たら、ページを送って読む。
const THREADS_QUERY = `query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100) {
        nodes {
          id isResolved isOutdated path line diffSide
          comments(first: 50) { nodes { id author { login } body url createdAt diffHunk } }
        }
      }
    }
  }
}`;

type GhComment = { id: string; author: { login: string } | null; body: string; url: string; createdAt: string; diffHunk: string };
type GhThread = { id: string; isResolved: boolean; isOutdated: boolean; path: string; line: number | null; diffSide?: 'LEFT' | 'RIGHT'; comments: { nodes: GhComment[] } };

/** 解決済みのスレッドは対応を終えているので外す。退会したユーザーのコメントは author が null になり、GitHub と同じく ghost と出す。 */
export const toThreads = (nodes: GhThread[]): PrThread[] =>
  nodes
    .filter((t) => !t.isResolved && t.comments.nodes.length > 0)
    .map((t) => ({
      id: t.id,
      path: t.path,
      line: t.line ?? undefined,
      side: t.diffSide === 'LEFT' ? 'old' : 'new',
      outdated: t.isOutdated,
      diffHunk: t.comments.nodes[0]?.diffHunk ?? '',
      comments: t.comments.nodes.map((c) => ({ id: c.id, author: c.author?.login ?? 'ghost', body: c.body, url: c.url, at: Date.parse(c.createdAt) })),
    }));

const repoOf = (pr: PrInfo): { host: string; owner: string; name: string; number: string } => {
  const [, host, owner, name, number] = /^https:\/\/([^/]+)\/([^/]+)\/([^/]+)\/pull\/(\d+)/.exec(pr.url) ?? [];
  if (!host || !owner || !name || !number) throw new Error(`PR の URL を読めません: ${pr.url}`);
  return { host, owner, name, number };
};

/** gh を呼び、標準出力を返す。input は標準入力に渡す本文で、cwd は gh がリポジトリを決めるディレクトリ。gh が失敗したときは、gh が標準エラーに書いた理由で投げる。 */
const gh = async (args: string[], input?: string, cwd?: string): Promise<string> => {
  const call = run('gh', args, { maxBuffer: MAX_BUFFER, cwd });
  call.child.stdin?.end(input);
  const { stdout } = await call.catch((e: { stderr?: string; message: string }) => {
    throw new Error(e.stderr?.trim() || e.message);
  });
  return stdout;
};

const ghApi = async <T,>(args: string[], input?: string, cwd?: string): Promise<T> => JSON.parse(await gh(['api', ...args], input, cwd)) as T;

const DETAIL_FIELDS = `${PR_FIELDS},author,body,headRefName,additions,deletions,files,comments,reviews,updatedAt`;

type GhAuthor = { login: string } | null;
type GhPrDetail = GhPr & {
  author: GhAuthor;
  body: string;
  headRefName: string;
  additions: number;
  deletions: number;
  files?: { path: string; additions: number; deletions: number }[];
  comments?: { author: GhAuthor; body: string; createdAt: string }[];
  reviews?: { author: GhAuthor; body: string; state: string; submittedAt: string }[];
  updatedAt: string;
};

/** 行へのコメントだけを付けたレビューは、総評が空の COMMENTED になる。行へのコメントは「PR コメント」で読むので、会話には出さない。 */
export const toPrDetail = (pr: GhPrDetail): PrDetail => {
  const posts: PrPost[] = [
    ...(pr.comments ?? []).map((c) => ({ author: c.author?.login ?? 'ghost', body: c.body, at: Date.parse(c.createdAt) })),
    ...(pr.reviews ?? [])
      .filter((r) => r.body.trim() || r.state !== 'COMMENTED')
      .map((r) => ({ author: r.author?.login ?? 'ghost', body: r.body, at: Date.parse(r.submittedAt), state: r.state })),
  ];
  return {
    ...toPrInfo(pr),
    author: pr.author?.login ?? 'ghost',
    body: pr.body,
    headRef: pr.headRefName,
    additions: pr.additions,
    deletions: pr.deletions,
    files: (pr.files ?? []).map(({ path, additions, deletions }) => ({ path, additions, deletions })),
    posts: posts.sort((a, b) => a.at - b.at),
    updatedAt: Date.parse(pr.updatedAt),
  };
};

// 1 区分に出す上限。GitHub の検索は 1 回で 100 件まで返す。
const INBOX_MAX = 50;
const INBOX_PR = `... on PullRequest { url number title isDraft reviewDecision mergeable updatedAt repository { nameWithOwner } author { login } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }`;
const inboxSearch = (q: string): string => `search(type: ISSUE, first: ${INBOX_MAX}, query: "is:pr is:open archived:false sort:updated-desc ${q}") { nodes { ${INBOX_PR} } }`;
// 直接の依頼は user-review-requested、チームへの依頼は review-requested から直接の依頼を除いたもの。
const INBOX_QUERY = `{
  direct: ${inboxSearch('user-review-requested:@me')}
  team: ${inboxSearch('review-requested:@me -user-review-requested:@me')}
  mine: ${inboxSearch('author:@me')}
}`;

type GhInboxPr = {
  url: string;
  number: number;
  title: string;
  isDraft: boolean;
  reviewDecision: string | null;
  mergeable: string;
  updatedAt: string;
  repository: { nameWithOwner: string };
  author: GhAuthor;
  commits: { nodes: { commit: { statusCheckRollup: { state: string } | null } }[] };
};
export type GhInbox = { direct: { nodes: GhInboxPr[] }; team: { nodes: GhInboxPr[] }; mine: { nodes: GhInboxPr[] } };

const MINE_SECTIONS = ['Your drafts', 'Waiting for review or checks', 'Needs action', 'Ready to merge'] as const;

/**
 * 自分の PR を Inbox の区分に分ける。GitHub は区分の条件を公開していないので、レビューの判定、チェック、競合から決める。
 * reviewDecision が null なのは、レビューを必須にしていないリポジトリで、承認と同じに扱う。
 */
export const mineSection = (pr: GhInboxPr): (typeof MINE_SECTIONS)[number] => {
  const checks = pr.commits.nodes[0]?.commit.statusCheckRollup?.state;
  if (pr.isDraft) return 'Your drafts';
  if (pr.reviewDecision === 'CHANGES_REQUESTED' || checks === 'FAILURE' || checks === 'ERROR' || pr.mergeable === 'CONFLICTING') return 'Needs action';
  if ((pr.reviewDecision ?? 'APPROVED') === 'APPROVED' && (checks ?? 'SUCCESS') === 'SUCCESS') return 'Ready to merge';
  return 'Waiting for review or checks';
};

const toInboxPr = (pr: GhInboxPr): InboxPr => ({
  url: pr.url,
  repo: pr.repository.nameWithOwner,
  number: pr.number,
  title: pr.title,
  author: pr.author?.login ?? 'ghost',
  updatedAt: Date.parse(pr.updatedAt),
});

/** 検索の結果を Inbox の区分に並べる。PR の無い区分は出さない。 */
export const toInbox = (data: GhInbox): InboxSection[] =>
  [
    { title: 'Needs your review', prs: data.direct.nodes },
    { title: "Needs your teams' review", prs: data.team.nodes },
    ...MINE_SECTIONS.map((title) => ({ title, prs: data.mine.nodes.filter((pr) => mineSection(pr) === title) })),
  ]
    .filter((s) => s.prs.length > 0)
    .map((s) => ({ title: s.title, prs: s.prs.map(toInboxPr) }));

/** GitHub の PR のダッシュボードの Inbox と同じ区分で、自分に関わる開いた PR を集める。 */
export const githubInbox = (fresh = false): Promise<InboxSection[]> =>
  cached('github-inbox', { maxAge: INBOX_MAX_AGE, fresh }, async () => toInbox((await ghApi<{ data: GhInbox }>(['graphql', '-f', `query=${INBOX_QUERY}`])).data));

/** PR の本文、変更したファイル、会話を GitHub から読む。 */
export const prDetail = (url: string, fresh = false): Promise<PrDetail> =>
  cached(`pr:${url}`, { maxAge: PR_MAX_AGE, fresh }, async () => toPrDetail(JSON.parse(await gh(['pr', 'view', url, '--json', DETAIL_FIELDS])) as GhPrDetail));

/** PR の会話にコメントを投稿し、投稿したコメントの URL を返す。 */
export const postPrComment = async (url: string, body: string): Promise<{ url: string }> => {
  const posted = (await gh(['pr', 'comment', url, '--body-file', '-'], body)).trim();
  forgetCached(`pr:${url}`);
  return { url: posted };
};

type GhPullRequest = { id: string; reviewThreads?: { nodes?: GhThread[] }; files?: { nodes?: { path: string; viewerViewedState: string }[] } };

/** PR を指す GraphQL を投げ、pullRequest を返す。query は $owner、$name、$number を受ける。 */
const prQuery = async (pr: PrInfo, query: string): Promise<GhPullRequest | undefined> => {
  const { host, owner, name, number } = repoOf(pr);
  // -F は値が @ で始まるとファイルを読むので、文字列は -f で渡す。
  const args = ['graphql', '--hostname', host, '-f', `owner=${owner}`, '-f', `name=${name}`, '-F', `number=${number}`, '-f', `query=${query}`];
  const body = await ghApi<{ data?: { repository?: { pullRequest?: GhPullRequest } } }>(args);
  return body.data?.repository?.pullRequest;
};

/** PR の未解決のレビュースレッド。 */
export const prThreads = (pr: PrInfo, fresh = false): Promise<PrThread[]> =>
  cached(`pr-threads:${pr.url}`, { maxAge: PR_MAX_AGE, fresh }, async () => toThreads((await prQuery(pr, THREADS_QUERY))?.reviewThreads?.nodes ?? []));

// ponytail: ファイルは先頭の 100 件まで。超える PR が出たら、ページを送って読む。
const FILES_QUERY = `query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) { pullRequest(number: $number) { id files(first: 100) { nodes { path viewerViewedState } } } }
}`;

/** GitHub で、自分が「Viewed」にした PR のファイル。 */
export const prViewed = (pr: PrInfo, fresh = false): Promise<string[]> =>
  cached(`pr-viewed:${pr.url}`, { maxAge: PR_MAX_AGE, fresh }, async () =>
    ((await prQuery(pr, FILES_QUERY))?.files?.nodes ?? []).filter((f) => f.viewerViewedState === 'VIEWED').map((f) => f.path));

/** GitHub の「Viewed」を切り替える。mutation は PR の node id を受けるので、先に読む。 */
export const setPrViewed = async (pr: PrInfo, file: string, viewed: boolean): Promise<void> => {
  const id = (await prQuery(pr, FILES_QUERY))?.id;
  if (!id) throw new Error(`PR が見つかりません: ${pr.url}`);
  const op = viewed ? 'markFileAsViewed' : 'unmarkFileAsViewed';
  const mutation = `mutation($id: ID!, $path: String!) { ${op}(input: { pullRequestId: $id, path: $path }) { clientMutationId } }`;
  await ghApi(['graphql', '--hostname', repoOf(pr).host, '-f', `id=${id}`, '-f', `path=${file}`, '-f', `query=${mutation}`]);
  forgetCached(`pr-viewed:${pr.url}`);
};

/** 行へのコメントと全体コメントを、1 つのレビュー（COMMENT）として PR に投稿する。commit は、行番号を数えた PR の先端。 */
export const postPrReview = async (pr: PrInfo, commit: string, comments: ReviewComment[], body: string): Promise<{ url: string }> => {
  const { host, owner, name, number } = repoOf(pr);
  const review = {
    commit_id: commit,
    event: 'COMMENT',
    body,
    comments: comments.map((c) => ({ path: c.path, line: c.line, side: c.side === 'old' ? 'LEFT' : 'RIGHT', body: c.body })),
  };
  const res = await ghApi<{ html_url: string }>(['--hostname', host, '--method', 'POST', `repos/${owner}/${name}/pulls/${number}/reviews`, '--input', '-'], JSON.stringify(review));
  forgetCached(`pr:${pr.url}`, `pr-threads:${pr.url}`);
  return { url: res.html_url };
};

/** Claude Code が実行中セッションの PR 状態を定期更新しているキャッシュを取り込む。自前で GitHub を見に行かずに済む。 */
export const mergeClaudePrCache = (file: string): void => {
  const statuses = readJson<Record<string, ClaudePrStatus>>(file);
  if (!statuses) return;
  claudePrStatuses = statuses;
  changed();
};

// 補完に出す PR、Issue、JIRA の課題は、題名がめったに変わらず、状態は少し遅れても選ぶのに困らないので、間隔を空けて読み直す。
const REFS_MAX_AGE = 10 * 60_000;
// # の後に何も打っていないときに出す、自分の PR と Issue の件数。
const MINE_LIMIT = 30;
// 打った語で探す、新しい PR と Issue のそれぞれの件数。
const REFS_LIMIT = 100;
const REF_FIELDS = 'number title createdAt state';
const PR_REF_FIELDS = `${REF_FIELDS} isDraft`;
const ISSUE_REF_FIELDS = `${REF_FIELDS} stateReason`;
const REFS_QUERY = `query($owner: String!, $name: String!, $mine: String!) {
  mine: search(query: $mine, type: ISSUE, first: ${MINE_LIMIT}) { nodes { __typename ... on PullRequest { ${PR_REF_FIELDS} } ... on Issue { ${ISSUE_REF_FIELDS} } } }
  repository(owner: $owner, name: $name) {
    pullRequests(first: ${REFS_LIMIT}, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { ${PR_REF_FIELDS} } }
    issues(first: ${REFS_LIMIT}, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { ${ISSUE_REF_FIELDS} } }
  }
}`;

type GhRef = { number: number; title: string; createdAt: string; state: 'OPEN' | 'CLOSED' | 'MERGED'; isDraft?: boolean; stateReason?: string | null };
export type GhRefs = {
  mine: { nodes: (GhRef & { __typename: 'PullRequest' | 'Issue' })[] };
  repository: { pullRequests: { nodes: GhRef[] }; issues: { nodes: GhRef[] } };
};

// 予定しないものか重複として閉じた Issue は、やり遂げて閉じたものと分けて Canceled と呼ぶ。
const refState = ({ state, isDraft, stateReason }: Pick<GhRef, 'state' | 'isDraft' | 'stateReason'>): RepoRef['state'] => {
  if (state === 'MERGED') return 'Merged';
  if (state === 'OPEN') return isDraft ? 'Draft' : 'Open';
  return stateReason === 'NOT_PLANNED' || stateReason === 'DUPLICATE' ? 'Canceled' : 'Closed';
};

const toRef = (kind: RepoRef['kind'], n: GhRef, mine: boolean): RepoRef => ({ number: n.number, title: n.title, kind, state: refState(n), createdAt: Date.parse(n.createdAt), mine });

/** 新しい順に並べる。自分のものは後に足すので、同じ番号は自分のものとして残る。 */
export const toRefs = ({ mine, repository }: GhRefs): RepoRef[] => {
  const refs = [
    ...repository.pullRequests.nodes.map((n) => toRef('pr', n, false)),
    ...repository.issues.nodes.map((n) => toRef('issue', n, false)),
    ...mine.nodes.map((n) => toRef(n.__typename === 'PullRequest' ? 'pr' : 'issue', n, true)),
  ];
  return [...new Map(refs.map((r) => [r.number, r])).values()].sort((a, b) => b.createdAt - a.createdAt);
};

/**
 * cwd のリポジトリの、自分が作った新しい PR と Issue と、誰かが作った新しい PR と Issue。{owner} と {repo} は、gh が cwd のリポジトリに置き換える。
 * ponytail: 打った語で探すのは、新しいものから REFS_LIMIT 件ずつだけ。古いものも番号で探したくなったら、GitHub の検索を足す。
 */
export const repoRefs = (cwd: string, fresh = false): Promise<RepoRef[]> =>
  cached(`repo-refs-v2:${cwd}`, { maxAge: REFS_MAX_AGE, fresh }, async () => {
    const args = ['graphql', '-F', 'owner={owner}', '-F', 'name={repo}', '-F', 'mine=repo:{owner}/{repo} author:@me sort:created-desc', '-f', `query=${REFS_QUERY}`];
    return toRefs((await ghApi<{ data: GhRefs }>(args, undefined, cwd)).data);
  });

const JIRA_BASE = process.env.JIRA_BASE_URL?.replace(/\/$/, '');
const JIRA_PROJECTS = (process.env.JIRA_PROJECT_KEYS ?? '').split(',').map((k) => k.trim()).filter(Boolean);
// ブランチ名は小文字で書かれることが多い（proj-1102）ため、登録したプロジェクトキーに限って大文字小文字を区別せずに拾う。
const JIRA_KEY = JIRA_PROJECTS.length ? new RegExp(`\\b(${JIRA_PROJECTS.join('|')})-(\\d+)`, 'gi') : undefined;
const JIRA_AUTH =
  process.env.JIRA_EMAIL && process.env.JIRA_API_TOKEN
    ? `Basic ${Buffer.from(`${process.env.JIRA_EMAIL}:${process.env.JIRA_API_TOKEN}`).toString('base64')}`
    : undefined;

export const findJiraKeys = (texts: (string | undefined)[]): string[] => {
  if (!JIRA_KEY || !JIRA_BASE) return [];
  const keys = new Set<string>();
  for (const t of texts) for (const m of t?.matchAll(JIRA_KEY) ?? []) keys.add(`${m[1]?.toUpperCase()}-${m[2]}`);
  return [...keys];
};

const browseUrl = (key: string): string => `${JIRA_BASE ?? ''}/browse/${key}`;

// カードのバッジに出す題名とステータスは、すべてのセッションの課題の分を読むので、間隔を空けて読み直す。
const JIRA_BADGE_MAX_AGE = 10 * 60_000;
// 課題の画面と一覧は、開いている間に変わったものを出すよう、短い間隔で読み直す。
const JIRA_MAX_AGE = 30_000;

/** API トークンが無いときはリンクだけ返す。 */
export const jiraIssue = (key: string): JiraIssue => {
  const badge = JIRA_AUTH
    ? peekCached(`jira-issue-v2:${key}`, { maxAge: JIRA_BADGE_MAX_AGE }, async () => {
        const { fields } = await jiraFetch<JiraIssueBody>(`issue/${encodeURIComponent(key)}?fields=summary,status`);
        return { summary: fields?.summary, status: fields?.status?.name, statusCategory: fields?.status?.statusCategory?.key };
      })
    : undefined;
  return { key, url: browseUrl(key), ...badge };
};

let refreshedAt = 0;

export const refreshJira = (): void => {
  refreshedAt = Date.now();
  forgetCached(/^jira/);
};

/** 人間が「更新」で JIRA の情報を取り直させたか、ダッシュボードで課題のステータスを移した時刻。 */
export const jiraRefreshedAt = (): number => refreshedAt;

type JiraSearchBody = {
  issues?: {
    key: string;
    fields?: {
      summary?: string;
      status?: { name?: string; statusCategory?: { key?: string } };
      assignee?: { displayName?: string } | null;
      updated?: string;
      priority?: { name?: string } | null;
      parent?: { key: string; fields?: { summary?: string } };
      labels?: string[];
      reporter?: { displayName?: string } | null;
      duedate?: string | null;
    };
  }[];
  errorMessages?: string[];
};

const SEARCH_FIELDS = 'summary,status,assignee,updated,priority,parent,labels,reporter,duedate';

/** JQL で課題を探す。fields で読む項目を絞れる。JQL の誤りなどで探せないときは、JIRA が返した理由を投げる。 */
export const searchJira = async (jql: string, max: number, fields = SEARCH_FIELDS): Promise<JiraIssue[]> => {
  if (!JIRA_BASE || !JIRA_AUTH) throw new Error('~/.durun-dashboard/.env.local に JIRA_BASE_URL、JIRA_EMAIL、JIRA_API_TOKEN を設定してください');
  const query = new URLSearchParams({ jql, maxResults: String(max), fields });
  const res = await fetch(`${JIRA_BASE}/rest/api/3/search/jql?${query}`, { headers: { authorization: JIRA_AUTH, accept: 'application/json' } });
  const body = (await res.json().catch(() => ({}))) as JiraSearchBody;
  if (!res.ok) throw new Error(body.errorMessages?.join(' ') || `HTTP ${res.status}`);
  return (body.issues ?? []).map(({ key, fields: f }) => ({
    key,
    url: browseUrl(key),
    summary: f?.summary,
    status: f?.status?.name,
    statusCategory: f?.status?.statusCategory?.key,
    assignee: f?.assignee?.displayName,
    updatedAt: f?.updated ? Date.parse(f.updated) : undefined,
    priority: f?.priority?.name,
    parent: f?.parent && { key: f.parent.key, summary: f.parent.fields?.summary },
    labels: f?.labels?.length ? f.labels : undefined,
    reporter: f?.reporter?.displayName,
    dueDate: f?.duedate ?? undefined,
  }));
};

/** JIRA Issues のウィジェットに並べる課題。 */
export const jiraList = (jql: string, max: number): Promise<JiraIssue[]> => cached(`jira-search:${max} ${jql}`, { maxAge: JIRA_MAX_AGE }, () => searchJira(jql, max));

const JIRA_MINE = 'assignee = currentUser() OR reporter = currentUser()';
// JIRA の検索は、キーだけなら 1 回でこの件数まで返す。古い課題しか無いスペースも数えるよう、自分の課題はキーだけをこの件数まで読む。
const JIRA_KEYS_MAX = 5000;

/**
 * JIRA のキーの補完に出す課題。自分が担当者か起票者の課題が 1 つ以上あるスペースごとに、自分の課題、ほかの課題の順に、それぞれ更新が新しい順で並べる。
 * ponytail: スペースごとに、更新が新しい JIRA_MAX_RESULTS 件ずつだけ。古い課題もキーで探したくなったら、打った番号で JIRA を探す。
 */
export const jiraRefs = (fresh = false): Promise<JiraIssue[]> =>
  cached('jira-refs', { maxAge: REFS_MAX_AGE, fresh }, async () => {
    const spaces = new Set((await searchJira(JIRA_MINE, JIRA_KEYS_MAX, 'key')).map((j) => j.key.replace(/-\d+$/, '')));
    const lists = await Promise.all(
      [...spaces].flatMap((space) => [
        searchJira(`project = "${space}" AND (${JIRA_MINE}) ORDER BY updated DESC`, JIRA_MAX_RESULTS, 'summary,status'),
        searchJira(`project = "${space}" ORDER BY updated DESC`, JIRA_MAX_RESULTS, 'summary,status'),
      ]),
    );
    // 同じキーは、先に並べた自分の課題の位置に残る。
    return [...new Map(lists.flat().map((j) => [j.key, j])).values()];
  });

/** 課題の画面の URL の前に付く部分。設定が無ければ undefined。 */
export const jiraBase = (): string | undefined => JIRA_BASE && JIRA_AUTH && JIRA_BASE;

type Adf = {
  type?: string;
  text?: string;
  attrs?: { text?: string; url?: string; shortName?: string; level?: number; state?: string };
  marks?: { type?: string; attrs?: { href?: string } }[];
  content?: Adf[];
};

// 段落や見出し、リストは空行で区切り、Markdown として読める程度の文字にする。リストの直後の段落が項目の続きと読まれないよう、リストの後にも空行を置く。
const BLOCKS = new Set(['paragraph', 'bulletList', 'orderedList', 'taskList', 'blockquote', 'rule', 'mediaSingle']);

// 太字と斜体は文字だけ残す。marked は日本語の句読点に接した ** を閉じず、記号のまま出してしまう。
const markedText = (node: Adf): string => {
  const has = (type: string) => node.marks?.find((m) => m.type === type);
  const text = has('code') ? `\`${node.text ?? ''}\`` : (node.text ?? '');
  const href = has('link')?.attrs?.href;
  return href ? `[${text}](${href})` : text;
};

// GFM の表は、表の 1 行を Markdown の 1 行に書くので、セルの中の改行は空白にし、区切りの | はエスケープする。
const cellText = (cell: Adf): string => adfText(cell).trim().replace(/\s*\n\s*/g, ' ').replaceAll('|', '\\|');

// ponytail: GFM の表は、セルの結合と、セルの中のリストを表せない。結合したセルは左に詰め、リストは 1 行につなぐ。崩れる表が出たら、ADF から <table> を直接組む。
const tableText = (table: Adf): string => {
  const rows = (table.content ?? []).map((row) => (row.content ?? []).map(cellText));
  const width = Math.max(0, ...rows.map((r) => r.length));
  const line = (cells: string[]): string => `| ${Array.from({ length: width }, (_, i) => cells[i] ?? '').join(' | ')} |`;
  // GFM の表には見出しの行が要るので、先頭の行を見出しにする。JIRA の表は既定で先頭の行が見出しになっている。
  const [head = [], ...body] = rows;
  return `${[line(head), line(Array(width).fill('---')), ...body.map(line)].join('\n')}\n\n`;
};

/** JIRA の本文やコメントの形式（Atlassian Document Format）を、画面に出す Markdown にする。 */
export const adfText = (node: Adf | null | undefined): string => {
  if (!node) return '';
  if (node.type === 'text') return markedText(node);
  if (node.type === 'hardBreak') return '\n';
  if (node.type === 'mention' || node.type === 'emoji') return node.attrs?.text ?? node.attrs?.shortName ?? '';
  if (node.type === 'inlineCard') return node.attrs?.url ?? '';
  if (node.type === 'table') return tableText(node);
  const inner = (node.content ?? []).map(adfText).join('');
  if (node.type === 'heading') return `${'#'.repeat(node.attrs?.level ?? 1)} ${inner}\n\n`;
  if (node.type === 'listItem') return `- ${inner.trim()}\n`;
  if (node.type === 'taskItem') return `- [${node.attrs?.state === 'DONE' ? 'x' : ' '}] ${inner.trim()}\n`;
  if (node.type === 'codeBlock') return `\`\`\`\n${inner}\n\`\`\`\n\n`;
  return BLOCKS.has(node.type ?? '') ? `${inner}\n\n` : inner;
};

type JiraIssueBody = {
  fields?: {
    summary?: string;
    status?: { name?: string; statusCategory?: { key?: string } };
    assignee?: { displayName?: string } | null;
    reporter?: { displayName?: string } | null;
    priority?: { name?: string } | null;
    parent?: { key: string; fields?: { summary?: string } };
    labels?: string[];
    duedate?: string | null;
    updated?: string;
    description?: Adf | null;
    comment?: { comments?: { author?: { displayName?: string }; body?: Adf; created?: string }[] };
  };
  transitions?: { id: string; name: string; to?: { name?: string; statusCategory?: { key?: string } } }[];
  errorMessages?: string[];
};

// ponytail: 子の課題は先頭の 100 件まで。超える Epic が出たら、件数とページ送りを足す。
const JIRA_CHILDREN_MAX = 100;

const jiraFetch = async <T,>(apiPath: string, init?: RequestInit): Promise<T> => {
  if (!JIRA_BASE || !JIRA_AUTH) throw new Error('~/.durun-dashboard/.env.local に JIRA_BASE_URL、JIRA_EMAIL、JIRA_API_TOKEN を設定してください');
  const res = await fetch(`${JIRA_BASE}/rest/api/3/${apiPath}`, {
    ...init,
    headers: { authorization: JIRA_AUTH, accept: 'application/json', 'content-type': 'application/json' },
  });
  // JIRA は、トランジションの画面で埋める項目が足りないときなどに、理由を errorMessages ではなく項目ごとの errors に返す。
  const body = (await res.json().catch(() => ({}))) as T & { errorMessages?: string[]; errors?: Record<string, string> };
  if (!res.ok) throw new Error([...(body.errorMessages ?? []), ...Object.values(body.errors ?? {})].join(' ') || `HTTP ${res.status}`);
  return body;
};

/** 課題の画面に出す中身。コメントは古い順に、子の課題（Epic に属する課題やサブタスク）は作った順に並べる。 */
export const jiraDetail = (key: string, fresh = false): Promise<JiraDetail> => cached(`jira:${key}`, { maxAge: JIRA_MAX_AGE, fresh }, () => readJiraDetail(key));

const readJiraDetail = async (key: string): Promise<JiraDetail> => {
  const fields = 'summary,status,assignee,reporter,priority,parent,labels,duedate,updated,description,comment';
  const [{ fields: f, transitions }, children] = await Promise.all([
    jiraFetch<JiraIssueBody>(`issue/${key}?fields=${fields}&expand=transitions`),
    searchJira(`parent = ${key} ORDER BY created ASC`, JIRA_CHILDREN_MAX),
  ]);
  return {
    key,
    url: browseUrl(key),
    summary: f?.summary,
    status: f?.status?.name,
    statusCategory: f?.status?.statusCategory?.key,
    assignee: f?.assignee?.displayName,
    updatedAt: f?.updated ? Date.parse(f.updated) : undefined,
    priority: f?.priority?.name,
    parent: f?.parent && { key: f.parent.key, summary: f.parent.fields?.summary },
    labels: f?.labels?.length ? f.labels : undefined,
    reporter: f?.reporter?.displayName,
    dueDate: f?.duedate ?? undefined,
    description: adfText(f?.description).trim(),
    comments: (f?.comment?.comments ?? []).map((c) => ({ author: c.author?.displayName ?? '', body: adfText(c.body).trim(), at: Date.parse(c.created ?? '') })),
    children,
    // どのステータスからも移せるトランジションは、今のステータスへ移すものも返るので外す。
    transitions: (transitions ?? [])
      .filter((t) => t.to?.name !== f?.status?.name)
      .map((t) => ({ id: t.id, name: t.name, status: t.to?.name ?? t.name, statusCategory: t.to?.statusCategory?.key })),
  };
};

/** 課題のステータスをトランジションで移す。その課題のバッジと JIRA Issues のウィジェットに、移した後のステータスを読み直させる。 */
export const transitionJira = async (key: string, id: string): Promise<void> => {
  await jiraFetch(`issue/${key}/transitions`, { method: 'POST', body: JSON.stringify({ transition: { id } }) });
  refreshedAt = Date.now();
  forgetCached(`jira-issue-v2:${key}`, `jira:${key}`, 'jira-refs', /^jira-search:/);
};

/** 課題にコメントを投稿する。改行で区切った行を、それぞれ段落にする。 */
export const postJiraComment = async (key: string, text: string): Promise<void> => {
  const content = text.split('\n').map((line) => ({ type: 'paragraph', content: line ? [{ type: 'text', text: line }] : [] }));
  await jiraFetch(`issue/${key}/comment`, { method: 'POST', body: JSON.stringify({ body: { type: 'doc', version: 1, content } }) });
  forgetCached(`jira:${key}`);
};

/** URL が JIRA の課題の画面なら、その課題のキー。 */
export const jiraKeyOfUrl = (url: URL): string | undefined => {
  const prefix = `${JIRA_BASE}/browse/`;
  const page = `${url.origin}${url.pathname}`.replace(/\/$/, '');
  const key = JIRA_BASE && page.startsWith(prefix) ? page.slice(prefix.length) : undefined;
  return key && JIRA_ISSUE_KEY.test(key) ? key : undefined;
};

const GITHUB_REF = /^\/([^/]+)\/([^/]+)\/(pull|issues)\/(\d+)(\/|$)/;

/** バッジに登録する URL か課題のキーを、PR、Issue、JIRA の課題そのものを指す URL にそろえる。PR の /files のような下のページと、? や # から後は落とす。 */
export const badgeOf = (text: string): { kind: 'pr' | 'issue' | 'jira'; url: string } | undefined => {
  const typed = text.toUpperCase();
  if (JIRA_BASE && JIRA_ISSUE_KEY.test(typed)) return { kind: 'jira', url: browseUrl(typed) };
  if (!URL.canParse(text)) return undefined;
  const u = new URL(text);
  const key = jiraKeyOfUrl(u);
  if (key) return { kind: 'jira', url: browseUrl(key) };
  const [, owner, name, kind, num] = (u.protocol === 'https:' && GITHUB_REF.exec(u.pathname)) || [];
  return num ? { kind: kind === 'pull' ? 'pr' : 'issue', url: `${u.origin}/${owner}/${name}/${kind}/${num}` } : undefined;
};

/** 課題のキーと題名をつないだ、資料に並べる題名。 */
export const jiraTitle = async (key: string): Promise<string> => {
  const { fields } = await jiraFetch<JiraIssueBody>(`issue/${key}?fields=summary`);
  return [key, fields?.summary].filter(Boolean).join(' ');
};

const newFile = (header: string): DiffFile => {
  // パスは後続の ---/+++ 行で上書きする。バイナリなどそれらが無いときだけ diff --git 行の値を使う。
  const m = /^diff --git a\/(.*) b\/(.*)$/.exec(header);
  return { path: m?.[2] ?? '', status: 'modified', binary: false, additions: 0, deletions: 0, editedBySession: false, hunks: [] };
};

const applyFileHeader = (file: DiffFile, line: string): void => {
  if (line.startsWith('new file mode')) file.status = 'added';
  else if (line.startsWith('deleted file mode')) file.status = 'deleted';
  else if (line.startsWith('rename from ')) {
    file.status = 'renamed';
    file.oldPath = line.slice('rename from '.length);
  } else if (line.startsWith('rename to ')) file.path = line.slice('rename to '.length);
  else if (line.startsWith('Binary files ')) file.binary = true;
  else if (line.startsWith('+++ b/')) file.path = line.slice('+++ b/'.length);
  else if (line.startsWith('--- a/') && file.status === 'deleted') file.path = line.slice('--- a/'.length);
};

/** git diff の unified 形式を、行番号つきの構造に変換する。 */
export const parseDiff = (text: string): DiffFile[] => {
  const files: DiffFile[] = [];
  let file: DiffFile | undefined;
  let lines: DiffFile['hunks'][number]['lines'] | undefined;
  let oldNo = 0;
  let newNo = 0;
  for (const line of text.split('\n')) {
    if (line.startsWith('diff --git ')) {
      file = newFile(line);
      files.push(file);
      lines = undefined;
    } else if (file && line.startsWith('@@')) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      oldNo = Number(m?.[1] ?? 0);
      newNo = Number(m?.[2] ?? 0);
      lines = [];
      file.hunks.push({ header: line, lines });
    } else if (file && !lines) applyFileHeader(file, line);
    else if (file && lines && line[0] === '+') {
      lines.push({ kind: 'add', newNo: newNo++, text: line.slice(1) });
      file.additions++;
    } else if (file && lines && line[0] === '-') {
      lines.push({ kind: 'del', oldNo: oldNo++, text: line.slice(1) });
      file.deletions++;
    } else if (lines && line[0] === ' ') lines.push({ kind: 'ctx', oldNo: oldNo++, newNo: newNo++, text: line.slice(1) });
  }
  return files;
};

const git = async (cwd: string, args: string[]): Promise<string> =>
  (await run('git', ['-c', 'core.quotePath=false', ...args], { cwd, maxBuffer: MAX_BUFFER })).stdout;

/** cwd を含むリポジトリの直下。 */
const repoRoot = async (cwd: string): Promise<string> => (await git(cwd, ['rev-parse', '--show-toplevel'])).trim();

/** 作業ツリーの直下。onBranch が false なら、求めたブランチを開いた worktree が無く、dir は別のコードを開いている。 */
export type Tree = { dir: string; onBranch: boolean };

/**
 * branch を開いている worktree の直下。セッションが作業ディレクトリとは別の worktree で作業していても、ブランチのコードを読めるようにする。
 * どこにも開いていなければ、cwd を含む worktree の直下を返す。消えた worktree（prunable）は使わない。
 */
export const treeOf = async (cwd: string, branch: string): Promise<Tree> => {
  const blocks = (await git(cwd, ['worktree', 'list', '--porcelain'])).split('\n\n').map((b) => b.split('\n'));
  const open = blocks.find((lines) => lines.includes(`branch refs/heads/${branch}`) && !lines.some((l) => l.startsWith('prunable')));
  const dir = open?.[0]?.replace(/^worktree /, '');
  return dir ? { dir, onBranch: true } : { dir: await repoRoot(cwd), onBranch: false };
};

/** bases の順に、head との分岐点を探す。 */
const findBranchPoint = async (root: string, head: string, bases: string[]): Promise<string | undefined> => {
  for (const ref of bases) {
    try {
      return (await git(root, ['merge-base', head, ref])).trim();
    } catch {
      // 次の候補を試す。
    }
  }
  return undefined;
};

/** HEAD~2 や短い sha を、40 桁の sha にする。git のオプションと取り違えないよう、- で始まるものは受けない。 */
export const resolveCommit = async (cwd: string, ref: string): Promise<string> => {
  const sha = ref.startsWith('-') ? '' : await git(cwd, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]).then((out) => out.trim(), () => '');
  if (!sha) throw new Error(`コミットが見つかりません: ${ref}`);
  return sha;
};

/** Claude がレビューを頼む範囲。後のコミットで HEAD が動いてもずれないよう、sha にして持つ。 */
export const resolveRange = async (cwd: string, from: string, to: string): Promise<DiffRange> => {
  const range = { from: await resolveCommit(cwd, from), to: await resolveCommit(cwd, to) };
  try {
    await git(cwd, ['merge-base', '--is-ancestor', range.from, range.to]);
  } catch {
    throw new Error(`from（${from}）には、to（${to}）と同じか、それより古いコミットを渡してください`);
  }
  return range;
};

/** 範囲の比較元。from の親で、from が最初のコミットなら何も無いツリー。 */
const parentOf = async (root: string, sha: string): Promise<string> => {
  try {
    return (await git(root, ['rev-parse', '--verify', '--quiet', `${sha}^`])).trim();
  } catch {
    return EMPTY_TREE;
  }
};

const parseCommits = (out: string): CommitInfo[] =>
  out.split('\n').filter(Boolean).map((line) => {
    const [sha = '', subject = ''] = line.split('\x1f');
    return { sha, subject };
  });

/** 先端を git に渡せる参照にする。未コミットの変更は HEAD の上に載っているので、HEAD から数える。 */
const headOf = (tip: string): string => (tip === WORKTREE ? 'HEAD' : tip);

/** 範囲を選ぶ候補。先端から first-parent でさかのぼった最近のコミットに、入っていない範囲の端を古い側に足す。 */
const listCommits = async (root: string, tip: string, range: DiffRange): Promise<CommitInfo[]> => {
  const commits = parseCommits(await git(root, ['log', '--first-parent', `-n${COMMIT_LIMIT}`, COMMIT_FORMAT, headOf(tip)]));
  for (const end of [range.to, range.from]) {
    if (end !== WORKTREE && !commits.some((c) => c.sha === end)) commits.push(...parseCommits(await git(root, ['log', '-1', COMMIT_FORMAT, end])));
  }
  return commits;
};

/** 分岐点を探す相手を、試す順に返す。Claude がブランチの比べる相手を渡していれば、それだけにする。 */
const branchBases = async (root: string, baseRef: string | undefined, view: BranchView | undefined): Promise<string[]> => {
  if (view?.base) return [await resolveCommit(root, view.base)];
  return [view ? undefined : baseRef, 'HEAD', 'main', 'master'].filter((r): r is string => !!r).map((r) => `origin/${r}`);
};

/** 範囲を渡されないときの差分の比較元と、範囲の始まり。分岐点が無ければ、作業ツリーは未コミットの変更だけに、ブランチは先端のコミットだけにする。 */
const defaultStart = async (root: string, tip: string, bases: string[]): Promise<{ base: string; from: string }> => {
  const point = await findBranchPoint(root, headOf(tip), bases);
  if (!point) return tip === WORKTREE ? { base: 'HEAD', from: WORKTREE } : { base: await parentOf(root, tip), from: tip };
  const branch = (await git(root, ['rev-list', '--first-parent', `${point}..${headOf(tip)}`])).trim().split('\n');
  return { base: point, from: branch.at(-1) || tip };
};

/** ファイルの中身を、まるごと 1 つの hunk にする。新規ファイルは追加の行に、差分に無いファイルは文脈の行にする。中身を渡されない大きなファイルとバイナリは、中身を出さない。 */
const oneHunk = (name: string, content: string | undefined, kind: 'add' | 'ctx'): DiffFile => {
  const file: DiffFile = { path: name, status: kind === 'add' ? 'added' : 'unchanged', binary: false, additions: 0, deletions: 0, editedBySession: false, hunks: [] };
  if (content === undefined || content.includes('\0')) return { ...file, binary: true };
  const body = content.endsWith('\n') ? content.slice(0, -1) : content;
  const lines = body.split('\n').map((text, i) => ({ kind, oldNo: kind === 'ctx' ? i + 1 : undefined, newNo: i + 1, text }));
  const n = lines.length;
  const header = kind === 'add' ? `@@ -0,0 +1,${n} @@ 新規ファイル` : `@@ -1,${n} +1,${n} @@ ファイル全体`;
  return { ...file, additions: kind === 'add' ? n : 0, hunks: [{ header, lines }] };
};

const readWhole = (root: string, name: string, kind: 'add' | 'ctx'): DiffFile | undefined => {
  const full = path.join(root, name);
  try {
    const stat = fs.statSync(full);
    if (!stat.isFile()) return undefined;
    return oneHunk(name, stat.size > MAX_UNTRACKED_BYTES ? undefined : fs.readFileSync(full, 'utf8'), kind);
  } catch {
    // 壊れたシンボリックリンクなど、読めないものは出さない。
    return undefined;
  }
};

const untrackedFiles = async (root: string): Promise<DiffFile[]> => {
  const names = (await git(root, ['ls-files', '--others', '--exclude-standard', '-z'])).split('\0').filter(Boolean);
  return names.slice(0, MAX_UNTRACKED_FILES).flatMap((name) => readWhole(root, name, 'add') ?? []);
};

/**
 * セッションの作業ディレクトリについて、コミットの範囲の差分を返す。範囲の端の WORKTREE は、未コミットの変更を指す。
 * 範囲を渡されなければ、分岐点から作業ツリーまでを出す。view を渡されたら、作業ツリーの代わりに、そのブランチの先端までを出す。
 */
export const gitDiff = async (cwd: string, baseRef: string | undefined, full: boolean, edited: Set<string>, want: Partial<DiffRange> = {}, view?: BranchView): Promise<DiffResult> => {
  const root = await repoRoot(cwd);
  const tip = view ? await resolveCommit(root, view.branch) : WORKTREE;
  const end = async (ref: string): Promise<string> => (ref === WORKTREE ? tip : resolveCommit(root, ref));
  const to = await end(want.to || WORKTREE);
  const from = want.from && (await end(want.from));
  const start = from ? { from, base: from === WORKTREE ? 'HEAD' : await parentOf(root, from) } : await defaultStart(root, tip, await branchBases(root, baseRef, view));
  const ends = to === WORKTREE ? [start.base] : [start.base, to];
  const range = { from: start.from, to };
  // 差分、未追跡のファイル、コミットの一覧は互いの結果を使わないので、git を同時に動かして待つ時間を縮める。
  const [raw, untracked, commits] = await Promise.all([
    git(root, ['diff', '--no-color', '--no-ext-diff', '--find-renames', `-U${full ? FULL_CONTEXT_LINES : 3}`, ...ends]),
    to === WORKTREE ? untrackedFiles(root) : [],
    listCommits(root, tip, range),
  ]);
  let text = raw;
  const truncated = text.length > MAX_DIFF_CHARS;
  if (truncated) {
    const cut = text.lastIndexOf('\ndiff --git ', MAX_DIFF_CHARS);
    text = text.slice(0, cut > 0 ? cut : MAX_DIFF_CHARS);
  }
  const files = [...parseDiff(text), ...untracked];
  for (const f of files) f.editedBySession = edited.has(path.join(root, f.path));
  return { range, commits, files, truncated };
};

/** ブランチのファイルを読む。作業ツリーに無いファイルも指せるよう、パスは実在を確かめずに、リポジトリの直下からの位置にする。 */
const branchFile = async (root: string, cwd: string, name: string, branch: string): Promise<DiffFile | undefined> => {
  const sha = await resolveCommit(root, branch);
  const candidates = [path.resolve(fs.realpathSync(cwd), name), path.resolve(root, name)]
    .map((full) => path.relative(root, full))
    .filter((p) => p && !p.startsWith('..') && !path.isAbsolute(p));
  for (const p of candidates) {
    const content = await git(root, ['cat-file', 'blob', `${sha}:${p}`]).catch(() => undefined);
    if (content !== undefined) return oneHunk(p, content.length > MAX_UNTRACKED_BYTES ? undefined : content, 'ctx');
  }
  return undefined;
};

/**
 * 差分に無いファイルを、まるごと読む。ref は作業ディレクトリかリポジトリの直下からのパスか、絶対パス。
 * branch を渡されたら、そのブランチから読む。作業ツリーからは、git が無視するファイル（.env など）と、リポジトリの外のファイルは読まない。
 */
export const repoFile = async (cwd: string, ref: string, branch?: string): Promise<DiffFile | undefined> => {
  const root = await repoRoot(cwd);
  const name = ref.replace(/:[\d-]+$/, '');
  if (branch) return branchFile(root, cwd, name, branch);
  const found = await repoPath(root, cwd, name);
  return found ? readWhole(root, found, 'ctx') : undefined;
};

/** 作業ツリーにあり、git が無視していないファイルの、リポジトリの直下からのパス。 */
const repoPath = async (root: string, cwd: string, name: string): Promise<string | undefined> => {
  // git が返すリポジトリの場所はシンボリックリンクを解いたものなので、ファイルの場所も解いてから比べる。
  const candidates = [path.resolve(cwd, name), path.resolve(root, name)]
    .flatMap((full) => (fs.existsSync(full) ? [path.relative(root, fs.realpathSync(full))] : []))
    .filter((p) => p && !p.startsWith('..') && !path.isAbsolute(p));
  if (!candidates.length) return undefined;
  const known = (await git(root, ['ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', ...candidates])).split('\0');
  return candidates.find((p) => known.includes(p));
};

/** コードの画面で開けるファイルか。中身は読まずに確かめる。 */
export const inRepo = (cwd: string, ref: string): Promise<boolean> =>
  repoRoot(cwd).then((root) => repoPath(root, cwd, ref.replace(/:[\d-]+$/, ''))).then(Boolean, () => false);
