import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type GhInbox, type GhSearch, toGithubSearch, toInbox } from '../server/integrations.ts';

type Pr = GhInbox['mine']['nodes'][number];
const pr = (number: number, over: Partial<Pr> = {}, checks?: string): Pr => ({
  url: `https://github.com/o/r/pull/${number}`,
  number,
  title: `PR ${number}`,
  isDraft: false,
  reviewDecision: 'REVIEW_REQUIRED',
  mergeable: 'UNKNOWN',
  updatedAt: '2026-10-09T00:00:00Z',
  repository: { nameWithOwner: 'o/r' },
  author: { login: 'me' },
  commits: { nodes: [{ commit: { statusCheckRollup: checks ? { state: checks } : null } }] },
  ...over,
});

test('toInbox は、自分の PR をドラフト、要対応、マージ可、レビューやチェック待ちに分け、PR の無い区分を出さない', () => {
  const sections = toInbox({
    direct: { nodes: [pr(1, { author: null })] },
    team: { nodes: [] },
    mine: {
      nodes: [
        pr(2, { isDraft: true }, 'FAILURE'),
        pr(3, { reviewDecision: 'APPROVED' }, 'FAILURE'),
        pr(4, { reviewDecision: 'CHANGES_REQUESTED' }, 'SUCCESS'),
        pr(5, { reviewDecision: 'APPROVED', mergeable: 'CONFLICTING' }),
        pr(6, { reviewDecision: 'APPROVED' }, 'SUCCESS'),
        pr(7, { reviewDecision: null }),
        pr(8, { reviewDecision: 'APPROVED' }, 'PENDING'),
        pr(9, {}, 'SUCCESS'),
      ],
    },
  });
  assert.deepEqual(
    sections.map((s) => [s.title, s.prs.map((p) => p.number)]),
    [
      ['Needs your review', [1]],
      ['Your drafts', [2]],
      ['Waiting for review or checks', [8, 9]],
      ['Needs action', [3, 4, 5]],
      ['Ready to merge', [6, 7]],
    ],
  );
  assert.deepEqual(sections[0]?.prs[0], { url: 'https://github.com/o/r/pull/1', repo: 'o/r', number: 1, title: 'PR 1', author: 'ghost', updatedAt: Date.parse('2026-10-09T00:00:00Z') });
});

test('toGithubSearch は、検索の結果を返った順のまま並べ、PR を Inbox の区分に分け、チームへの依頼は自分が入っているチームのときだけ数え、Issue とほかの種類を見分ける', () => {
  const reviewers = (...nodes: { __typename: string; login?: string; id?: string }[]) => ({ nodes: nodes.map((requestedReviewer) => ({ requestedReviewer })) });
  const node = (number: number, over: Partial<Pr> = {}, requested = reviewers(), checks?: string) => ({ ...pr(number, over, checks), __typename: 'PullRequest' as const, reviewRequests: requested });
  const found = toGithubSearch({
    viewer: { login: 'me' },
    search: {
      issueCount: 9,
      nodes: [
        node(1, { author: { login: 'you' } }, reviewers({ __typename: 'User', login: 'me' })),
        node(2, { author: { login: 'you' } }, reviewers({ __typename: 'Team', id: 'mine' })),
        node(6, { author: { login: 'you' } }, reviewers({ __typename: 'Team', id: 'other' })),
        node(3, { isDraft: true }, reviewers(), 'FAILURE'),
        node(4, { author: { login: 'you' }, reviewDecision: 'APPROVED' }, reviewers(), 'SUCCESS'),
        { __typename: 'Issue', url: 'https://github.com/o/r/issues/5', number: 5, title: 'Issue 5', updatedAt: '2026-10-09T00:00:00Z', repository: { nameWithOwner: 'o/r' }, author: { login: 'you' } },
        { __typename: 'Discussion' } as unknown as GhSearch['search']['nodes'][number],
      ],
    },
  }, new Set(['mine']));
  assert.equal(found.total, 9);
  assert.deepEqual(
    found.items.map((i) => [i.number, i.kind, i.section, i.review, i.checks, i.isDraft]),
    [
      [1, 'pr', 'Needs your review', 'REVIEW_REQUIRED', undefined, false],
      [2, 'pr', "Needs your teams' review", 'REVIEW_REQUIRED', undefined, false],
      [6, 'pr', undefined, 'REVIEW_REQUIRED', undefined, false],
      [3, 'pr', 'Your drafts', 'REVIEW_REQUIRED', 'FAILURE', true],
      [4, 'pr', undefined, 'APPROVED', 'SUCCESS', false],
      [5, 'issue', undefined, undefined, undefined, undefined],
    ],
  );
});
