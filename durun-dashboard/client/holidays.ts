// 日本の休みの日。国民の祝日は「国民の祝日に関する法律」の決まりから計算し、春分と秋分の日は 1980〜2099 年に合う近似式で出す。
// ponytail: 即位の日のような 1 回だけの祝日と、法改正による移動は入らない。ずれたら、その年の日付を足す。

/** y 年 m 月（0 始まり）の第 n 月曜日。 */
const nthMonday = (y: number, m: number, n: number): number => 1 + ((8 - new Date(y, m, 1).getDay()) % 7) + (n - 1) * 7;

/** 国民の祝日の [月, 日]。月は 0 始まり。 */
const nationalHolidays = (y: number): [number, number][] => {
  // 春分と秋分の近似式で、1980 年から積もるずれ。
  const drift = 0.242194 * (y - 1980) - Math.floor((y - 1980) / 4);
  return [
    [0, 1], [0, nthMonday(y, 0, 2)], [1, 11], [1, 23], [2, Math.floor(20.8431 + drift)], [3, 29], [4, 3], [4, 4], [4, 5],
    [6, nthMonday(y, 6, 3)], [7, 11], [8, nthMonday(y, 8, 3)], [8, Math.floor(23.2488 + drift)], [9, nthMonday(y, 9, 2)], [10, 3], [10, 23],
  ];
};

const isNationalHoliday = (date: Date): boolean =>
  nationalHolidays(date.getFullYear()).some(([m, d]) => date.getMonth() === m && date.getDate() === d);

const addDays = (date: Date, days: number): Date => new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);

/** 土日と国民の祝日に加え、祝日に挟まれた日（国民の休日）と、日曜の祝日の後で最初の祝日でない日（振替休日）を休みとする。日付は画面の時刻で見る。 */
export const isOffDay = (date: Date): boolean => {
  if (date.getDay() === 0 || date.getDay() === 6 || isNationalHoliday(date)) return true;
  if (isNationalHoliday(addDays(date, -1)) && isNationalHoliday(addDays(date, 1))) return true;
  for (let d = addDays(date, -1); isNationalHoliday(d); d = addDays(d, -1)) if (d.getDay() === 0) return true;
  return false;
};
