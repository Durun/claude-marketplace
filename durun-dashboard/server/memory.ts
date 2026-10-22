// macOS の sysctl と vm_stat の出力から、メモリプレッシャーと、アクティビティモニタのメモリの欄と同じ内訳を求める。
// Windows は、物理メモリの合計と空きから、タスクマネージャーと同じ使用率を求める。
import type { MemoryPressure, MemoryStatus, MemoryUsage } from '../shared/types.ts';

const MIB = 1024 * 1024;
// kern.memorystatus_vm_pressure_level は 1 が正常、2 が警告、4 が危険。
const LEVELS: Record<string, MemoryPressure['level']> = { 2: 'warn', 4: 'critical' };

/** vm_stat の「項目: ページ数.」の行を、項目ごとのバイト数にする。 */
const vmStatBytes = (text: string): Map<string, number> => {
  const pageSize = Number(/page size of (\d+) bytes/.exec(text)?.[1]);
  return new Map([...text.matchAll(/^"?([^":\n]+)"?:\s+(\d+)\.$/gm)].map(([, name = '', pages]) => [name, Number(pages) * pageSize]));
};

/** sysctl -n は、名前を並べた順に 1 行ずつ値を返す。 */
export const toMemoryStatus = (sysctl: string, vmStat: string): MemoryPressure => {
  const [level = '', freePercent, total, swap = ''] = sysctl.trim().split('\n');
  const bytes = vmStatBytes(vmStat);
  const get = (name: string): number => bytes.get(name) ?? 0;
  // パージできるページは、アプリが確保していても OS がいつでも捨てられるので、App メモリではなくキャッシュに数える。
  const purgeable = get('Pages purgeable');
  return {
    kind: 'pressure',
    level: LEVELS[level] ?? 'normal',
    percent: 100 - Number(freePercent),
    total: Number(total),
    app: get('Anonymous pages') - purgeable,
    wired: get('Pages wired down'),
    compressed: get('Pages occupied by compressor'),
    cached: get('File-backed pages') + purgeable,
    // vm.swapusage は「total = 2048.00M  used = 476.12M ...」の形で、常に MiB で書く。
    swapUsed: Number(/used = ([\d.]+)M/.exec(swap)?.[1] ?? 0) * MIB,
  };
};

// Windows には macOS のメモリプレッシャーに当たる段階が無いので、使用率で色を分ける。
const USAGE_WARN_PERCENT = 80;
const USAGE_CRITICAL_PERCENT = 90;

/** 物理メモリの合計と、すぐ使える量（タスクマネージャーの「利用可能」）から、使用率を求める。 */
export const toMemoryUsage = (total: number, available: number): MemoryUsage => {
  const used = total - available;
  const percent = total > 0 ? (used / total) * 100 : 0;
  return { kind: 'usage', level: percent >= USAGE_CRITICAL_PERCENT ? 'critical' : percent >= USAGE_WARN_PERCENT ? 'warn' : 'normal', percent, total, used, available };
};

/**
 * 前に読んだ値をすぐ返し、読み直しは裏で進める。sysctl と vm_stat の起動は、マシンが混んでいると数十秒かかるため。
 * 読めた値がまだ無ければ、読み終えるのを待つ。読めなかったら前の値を捨て、次は読み終えるのを待って、読めないことを画面に返す。
 */
export const latestMemory = (read: () => Promise<MemoryStatus | null>): (() => Promise<MemoryStatus | null>) => {
  let last: MemoryStatus | null | undefined;
  let reading: Promise<MemoryStatus | null> | undefined;
  return () => {
    reading ??= read()
      .then(
        (m) => (last = m),
        (e: unknown) => {
          last = undefined;
          throw e;
        },
      )
      .finally(() => (reading = undefined));
    if (last === undefined) return reading;
    // 裏で読めなかったことは、次に呼ばれたときに待って返すので、ここでは捨てる。
    reading.catch(() => {});
    return Promise.resolve(last);
  };
};
