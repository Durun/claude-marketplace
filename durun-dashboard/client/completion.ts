// メッセージ欄の補完の候補。/ の後ろは skills から、# の後ろは PR と Issue から、@ の後ろはファイルから、JIRA のプロジェクトキーとハイフンの後ろはそのスペースの課題から選ぶ。
import { rankBy } from '../shared/match.ts';
import type { JiraIssue, PrInfo, RepoRef, Session, SkillInfo } from '../shared/types.ts';

/** 打ちかけの語。mark は /、#、@、JIRA のプロジェクトキーとハイフン（CATS- など）のどれかで、word はその後ろからカーソルまで。 */
export type Typed = { mark: string; word: string };
/** 選ぶと、打ちかけの語を label に置き換える。icon は PR と Issue の候補だけが持ち、種別と状態をアイコンで示す。badge は、セッションのバッジにある候補で真になる。 */
export type Candidate = { label: string; desc: string; icon?: Pick<RepoRef, 'kind' | 'state'>; badge?: boolean };
/** 補完を使うセッションのバッジ。 */
export type Badges = Pick<Session, 'prs' | 'issues' | 'jira'>;

type Ref = Pick<RepoRef, 'number' | 'title' | 'url' | 'kind' | 'state'>;

// 行の頭か空白の後の /、#、@ から、カーソルまでの語。パスの途中の / や、URL の #、メールアドレスの @ では補完しない。
const TYPED = /(?:^|\s)([/#@])(\S*)$/;
// JIRA の課題のキーは日本語の文に続けて書くことが多いので、英数字の後ろでなければ補完する。
const JIRA_TYPED = /(?<![A-Za-z0-9_])([A-Za-z][A-Za-z0-9_]*-)(\S*)$/;

/** カーソルより前の文の、打ちかけの語。 */
export const typedAt = (before: string): Typed | undefined => {
  const m = TYPED.exec(before) ?? JIRA_TYPED.exec(before);
  return m ? { mark: m[1] ?? '', word: m[2] ?? '' } : undefined;
};

/** 名前の頭か、プラグイン名の後ろの頭に合うものを先に並べる。 */
const matchSkills = (skills: SkillInfo[], word: string): SkillInfo[] => {
  const q = word.toLowerCase();
  const hits = skills.filter((sk) => sk.name.toLowerCase().includes(q));
  const head = (sk: SkillInfo): boolean => sk.name.toLowerCase().startsWith(q) || sk.name.toLowerCase().includes(`:${q}`);
  return [...hits.filter(head), ...hits.filter((sk) => !head(sk))];
};

const prRefState = (pr: PrInfo): RepoRef['state'] => (pr.state === 'MERGED' ? 'Merged' : pr.state === 'CLOSED' ? 'Closed' : pr.isDraft ? 'Draft' : 'Open');
const repoOf = (url: string): string => url.replace(/\/(?:pull|issues)\/\d+$/, '');

/** バッジの PR と Issue。#番号はセッションのリポジトリの PR か Issue を指すので、refs と違うリポジトリのものは除く。 */
const badgeRefs = ({ prs = [], issues = [] }: Badges, refs: RepoRef[]): Ref[] => {
  const repos = new Set(refs.map((r) => repoOf(r.url)));
  const all: Ref[] = [...prs.map((pr) => ({ ...pr, kind: 'pr' as const, state: prRefState(pr) })), ...issues.map((i) => ({ ...i, kind: 'issue' as const }))];
  return all.filter((b) => repos.has(repoOf(b.url)));
};

/** バッジを先に並べ、items からバッジと同じものを除く。refs も issues も新しいものから決まった件数だけなので、そこに無いバッジも足す。 */
const badgesFirst = <T extends { url: string }>(badges: T[], items: T[]): T[] => {
  const urls = new Set(badges.map((b) => b.url));
  return [...badges, ...items.filter((x) => !urls.has(x.url))];
};

/** 何も打っていなければ、バッジと、自分が作ったものだけを出す。 */
const matchRefs = (refs: RepoRef[], badges: Ref[], word: string): Ref[] =>
  word ? rankBy(badgesFirst(badges, refs), word, (r) => String(r.number), (r) => r.title) : badgesFirst(badges, refs.filter((r) => r.mine));

/** prefix のスペースの課題。何も打っていなければ、そのスペースの課題をすべて出す。 */
const matchIssues = (issues: JiraIssue[], badges: JiraIssue[], prefix: string, word: string): JiraIssue[] =>
  rankBy(badgesFirst(badges, issues).filter((j) => j.key.startsWith(prefix)), word, (j) => j.key.slice(prefix.length), (j) => j.summary ?? '');

/**
 * refs は新しい順に、issues はスペースごとに自分の課題、ほかの課題の順に、それぞれ更新が新しい順に並べて渡す。
 * files は、サーバーが打った語に合う順に並べて絞ったパスで、そのまま出す。
 * badges の PR、Issue、JIRA の課題は、打った語に同じくらい合う候補の中で先に並べるので、何も打っていなければ先頭に出る。
 */
export const candidates = (typed: Typed, skills: SkillInfo[], refs: RepoRef[], issues: JiraIssue[], files: string[], badges: Badges = { prs: [], jira: [] }): Candidate[] => {
  if (typed.mark === '/') return matchSkills(skills, typed.word).map((sk) => ({ label: `/${sk.name}`, desc: sk.description }));
  if (typed.mark === '@') return files.map((p) => ({ label: `@${p}`, desc: '' }));
  const urls = new Set([...(badges.prs ?? []), ...(badges.issues ?? []), ...badges.jira].map((b) => b.url));
  if (typed.mark === '#') return matchRefs(refs, badgeRefs(badges, refs), typed.word).map((r) => ({ label: `#${r.number}`, desc: r.title, icon: { kind: r.kind, state: r.state }, badge: urls.has(r.url) }));
  return matchIssues(issues, badges.jira, typed.mark.toUpperCase(), typed.word).map((j) => ({ label: j.key, desc: [j.status, j.summary].filter(Boolean).join(' · '), badge: urls.has(j.url) }));
};
