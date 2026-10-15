// メッセージ欄の補完の候補。/ の後ろは skills から、# の後ろは PR と Issue から選ぶ。
import type { RepoRef, SkillInfo } from '../shared/types.ts';

/** 打ちかけの語。mark は / か # で、word はその後ろからカーソルまで。 */
export type Typed = { mark: string; word: string };
/** 選ぶと、打ちかけの語を label に置き換える。 */
export type Candidate = { label: string; desc: string };

// 行の頭か空白の後の / か # から、カーソルまでの語。パスの途中の / や、URL の # では補完しない。
const TYPED = /(?:^|\s)([/#])(\S*)$/;

/** カーソルより前の文の、打ちかけの語。 */
export const typedAt = (before: string): Typed | undefined => {
  const m = TYPED.exec(before);
  return m ? { mark: m[1] ?? '', word: m[2] ?? '' } : undefined;
};

/** 名前の頭か、プラグイン名の後ろの頭に合うものを先に並べる。 */
const matchSkills = (skills: SkillInfo[], word: string): SkillInfo[] => {
  const q = word.toLowerCase();
  const hits = skills.filter((sk) => sk.name.toLowerCase().includes(q));
  const head = (sk: SkillInfo): boolean => sk.name.toLowerCase().startsWith(q) || sk.name.toLowerCase().includes(`:${q}`);
  return [...hits.filter(head), ...hits.filter((sk) => !head(sk))];
};

/** text に q の文字がこの順に現れるか。 */
const inOrder = (q: string, text: string): boolean => {
  const chars = [...q];
  let i = 0;
  for (const c of text) if (c === chars[i]) i++;
  return i === chars.length;
};

/**
 * 何も打っていなければ、自分が作ったものを新しい順に並べる。
 * 打った語には、番号の頭が合うもの、題名に語を含むもの、題名に語の文字が順に現れるものの順に並べ、それぞれの中は新しい順にする。
 */
const matchRefs = (refs: RepoRef[], word: string): RepoRef[] => {
  if (!word) return refs.filter((r) => r.mine);
  const q = word.toLowerCase();
  const rank = (r: RepoRef): number => {
    const title = r.title.toLowerCase();
    if (String(r.number).startsWith(q)) return 0;
    if (title.includes(q)) return 1;
    return inOrder(q, title) ? 2 : 3;
  };
  return refs.map((r) => ({ r, rank: rank(r) })).filter((e) => e.rank < 3).sort((a, b) => a.rank - b.rank).map((e) => e.r);
};

/** refs は新しい順に並べて渡す。 */
export const candidates = (typed: Typed, skills: SkillInfo[], refs: RepoRef[]): Candidate[] =>
  typed.mark === '/'
    ? matchSkills(skills, typed.word).map((sk) => ({ label: `/${sk.name}`, desc: sk.description }))
    : matchRefs(refs, typed.word).map((r) => ({ label: `#${r.number}`, desc: `${r.kind === 'pr' ? 'PR' : 'Issue'} ${r.state} · ${r.title}` }));
