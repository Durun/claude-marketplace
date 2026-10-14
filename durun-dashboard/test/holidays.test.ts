import assert from 'node:assert/strict';
import { test } from 'node:test';
import { isOffDay } from '../client/holidays.ts';

const MONTH_DAYS = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

test('isOffDay は、2026 年の平日のうち、内閣府の一覧にある祝日と休日だけを休みにする', () => {
  const offWeekdays = MONTH_DAYS.flatMap((days, m) =>
    Array.from({ length: days }, (_, i) => new Date(2026, m, i + 1))
      .filter((d) => d.getDay() !== 0 && d.getDay() !== 6 && isOffDay(d))
      .map((d) => `${d.getMonth() + 1}/${d.getDate()}`),
  );
  // 5/6 は 5/3（日）の振替休日、9/22 は敬老の日と秋分の日に挟まれた国民の休日。
  assert.deepEqual(offWeekdays, ['1/1', '1/12', '2/11', '2/23', '3/20', '4/29', '5/4', '5/5', '5/6', '7/20', '8/11', '9/21', '9/22', '9/23', '10/12', '11/3', '11/23']);
});

test('isOffDay は、土日を休みにする', () => {
  assert.equal(isOffDay(new Date(2026, 9, 17)), true);
  assert.equal(isOffDay(new Date(2026, 9, 18)), true);
  assert.equal(isOffDay(new Date(2026, 9, 19)), false);
});
