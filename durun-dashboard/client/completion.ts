// メッセージ欄の補完の候補。/ の後ろは skills から、# の後ろは PR と Issue から、@ の後ろはファイルから、JIRA のプロジェクトキーとハイフンの後ろはそのスペースの課題から選ぶ。
import { rankBy } from '../shared/match.ts';
import type { JiraIssue, RepoRef, SkillInfo } from '../shared/types.ts';

/** 打ちかけの語。mark は /、#、@、JIRA のプロジェクトキーとハイフン（CATS- など）のどれかで、word はその後ろからカーソルまで。 */
export type Typed = { mark: string; word: string };
/** 選ぶと、打ちかけの語を label に置き換える。icon は PR と Issue の候補だけが持ち、種別と状態をアイコンで示す。 */
export type Candidate = { label: string; desc: string; icon?: Pick<RepoRef, 'kind' | 'state'> };

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

/** 何も打っていなければ、自分が作ったものだけを出す。 */
const matchRefs = (refs: RepoRef[], word: string): RepoRef[] =>
  word ? rankBy(refs, word, (r) => String(r.number), (r) => r.title) : refs.filter((r) => r.mine);

/** prefix のスペースの課題。何も打っていなければ、そのスペースの課題をすべて出す。 */
const matchIssues = (issues: JiraIssue[], prefix: string, word: string): JiraIssue[] =>
  rankBy(issues.filter((j) => j.key.startsWith(prefix)), word, (j) => j.key.slice(prefix.length), (j) => j.summary ?? '');

/**
 * refs は新しい順に、issues はスペースごとに自分の課題、ほかの課題の順に、それぞれ更新が新しい順に並べて渡す。
 * files は、サーバーが打った語に合う順に並べて絞ったパスで、そのまま出す。
 */
export const candidates = (typed: Typed, skills: SkillInfo[], refs: RepoRef[], issues: JiraIssue[], files: string[]): Candidate[] => {
  if (typed.mark === '/') return matchSkills(skills, typed.word).map((sk) => ({ label: `/${sk.name}`, desc: sk.description }));
  if (typed.mark === '@') return files.map((p) => ({ label: `@${p}`, desc: '' }));
  if (typed.mark === '#') return matchRefs(refs, typed.word).map((r) => ({ label: `#${r.number}`, desc: r.title, icon: { kind: r.kind, state: r.state } }));
  return matchIssues(issues, typed.mark.toUpperCase(), typed.word).map((j) => ({ label: j.key, desc: [j.status, j.summary].filter(Boolean).join(' · ') }));
};
