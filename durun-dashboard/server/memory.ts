// macOS のメモリプレッシャーと、アクティビティモニタのメモリの欄と同じ内訳を、sysctl と vm_stat から読む。
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type { MemoryStatus } from '../shared/types.ts';

const run = promisify(execFile);
const MIB = 1024 * 1024;
const SYSCTL_NAMES = ['kern.memorystatus_vm_pressure_level', 'kern.memorystatus_level', 'hw.memsize', 'vm.swapusage'];
// kern.memorystatus_vm_pressure_level は 1 が正常、2 が警告、4 が危険。
const LEVELS: Record<string, MemoryStatus['level']> = { 2: 'warn', 4: 'critical' };

/** vm_stat の「項目: ページ数.」の行を、項目ごとのバイト数にする。 */
const vmStatBytes = (text: string): Map<string, number> => {
  const pageSize = Number(/page size of (\d+) bytes/.exec(text)?.[1]);
  return new Map([...text.matchAll(/^"?([^":\n]+)"?:\s+(\d+)\.$/gm)].map(([, name = '', pages]) => [name, Number(pages) * pageSize]));
};

/** sysctl -n は、名前を並べた順に 1 行ずつ値を返す。 */
export const toMemoryStatus = (sysctl: string, vmStat: string): MemoryStatus => {
  const [level = '', freePercent, total, swap = ''] = sysctl.trim().split('\n');
  const bytes = vmStatBytes(vmStat);
  const get = (name: string): number => bytes.get(name) ?? 0;
  // パージできるページは、アプリが確保していても OS がいつでも捨てられるので、App メモリではなくキャッシュに数える。
  const purgeable = get('Pages purgeable');
  return {
    level: LEVELS[level] ?? 'normal',
    pressurePercent: 100 - Number(freePercent),
    total: Number(total),
    app: get('Anonymous pages') - purgeable,
    wired: get('Pages wired down'),
    compressed: get('Pages occupied by compressor'),
    cached: get('File-backed pages') + purgeable,
    // vm.swapusage は「total = 2048.00M  used = 476.12M ...」の形で、常に MiB で書く。
    swapUsed: Number(/used = ([\d.]+)M/.exec(swap)?.[1] ?? 0) * MIB,
  };
};

// ponytail: macOS だけ読む。Linux に広げるなら /proc/pressure/memory と /proc/meminfo から読む。
/** macOS 以外では null を返し、画面はメモリプレッシャーを出さない。 */
export const readMemory = async (): Promise<MemoryStatus | null> => {
  if (process.platform !== 'darwin') return null;
  const [sysctl, vmStat] = await Promise.all([run('/usr/sbin/sysctl', ['-n', ...SYSCTL_NAMES]), run('/usr/bin/vm_stat')]);
  return toMemoryStatus(sysctl.stdout, vmStat.stdout);
};
