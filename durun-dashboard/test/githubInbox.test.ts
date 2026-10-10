import assert from 'node:assert/strict';
import { test } from 'node:test';
import { type GhInbox, toInbox } from '../server/integrations.ts';

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
