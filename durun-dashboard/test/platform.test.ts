import assert from 'node:assert/strict';
import type { ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import type { IncomingMessage } from 'node:http';
import path from 'node:path';
import { test } from 'node:test';
import { fillCallerPid } from '../server/callerPid.ts';
import { toMemoryUsage } from '../server/memory.ts';
import { claudeCommand, type Files, hostsInTailscaleStatus, ownerInNetstat, parseFlProjectHints, type Platform, platformFor, type Run } from '../server/platform.ts';

const NETSTAT = `
アクティブな接続

  プロトコル  ローカル アドレス      外部アドレス           状態            PID
  TCP         127.0.0.1:7337         0.0.0.0:0              LISTENING       29012
  TCP         127.0.0.1:7337         127.0.0.1:50692        ESTABLISHED     29012
  TCP         127.0.0.1:50692        127.0.0.1:7337         TIME_WAIT       0
  TCP         127.0.0.1:50692        127.0.0.1:7337         ESTABLISHED     33544
  TCP         127.0.0.1:150692       127.0.0.1:7337         ESTABLISHED     11111
  TCP         [::1]:50700            [::1]:7337             ESTABLISHED     22222
`;

/** 呼ばれたコマンドを記録し、コマンドごとに決めた出力を返す。 */
const fakeRun = (outputs: Record<string, string>): { run: Run; calls: string[] } => {
  const calls: string[] = [];
  const run: Run = async (file, args) => {
    calls.push([file, ...args].join(' '));
    return outputs[file] ?? '';
  };
  return { run, calls };
};

test('netstat の行から、クライアント側のポートでサーバーへ張った接続を持つプロセスを引き、閉じた接続とサーバー側の行は使わない', () => {
  assert.equal(ownerInNetstat(NETSTAT, 50692, 7337), 33544);
  assert.equal(ownerInNetstat(NETSTAT, 50700, 7337), 22222);
  assert.equal(ownerInNetstat(NETSTAT, 50999, 7337), undefined);
});

test('Windows では、シェルに書くパスを / 区切りにし、MCP の呼び出し元を netstat から引く', async () => {
  const win = platformFor('win32');
  assert.equal(win.shellPath('C:\\Users\\me\\.durun-dashboard'), 'C:/Users/me/.durun-dashboard');
  assert.equal(win.defaultChromePath({ PROGRAMFILES: 'D:\\Apps' }), 'D:\\Apps\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.defaultChromePath({}), 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.localSocketPath('C:\\Temp\\test-abc', 'inbox'), '\\\\.\\pipe\\test-abc-inbox');
  assert.equal(win.headersHelperSendsPid, false);

  const { run, calls } = fakeRun({ netstat: NETSTAT });
  assert.equal(await win.connectionOwner(50692, 7337, run), 33544);
  assert.deepEqual(calls, ['netstat -ano']);
});

test('Windows では、物理メモリの合計と空きから使用率を求め、使用率で段階を分ける', async () => {
  const GIB = 1024 ** 3;
  assert.deepEqual(toMemoryUsage(32 * GIB, 12 * GIB), { kind: 'usage', level: 'normal', percent: 62.5, total: 32 * GIB, used: 20 * GIB, available: 12 * GIB });
  assert.equal(toMemoryUsage(100, 21).level, 'normal');
  assert.equal(toMemoryUsage(100, 20).level, 'warn');
  assert.equal(toMemoryUsage(100, 10).level, 'critical');
  assert.equal(toMemoryUsage(0, 0).percent, 0);

  // 合計と空きは Node がどの OS でも返すので、外のコマンドを呼ばずに読める。
  const { run, calls } = fakeRun({});
  const memory = await platformFor('win32').readMemory(run);
  assert.deepEqual(calls, []);
  assert.equal(memory?.kind, 'usage');
  assert.ok(memory && memory.total > 0 && memory.percent >= 0 && memory.percent <= 100);
});

test('Windows では、受信ソケットのトークンを、送り先のセッションの鍵から読み、読めなければ理由を返す', () => {
  const win = platformFor('win32');
  const dir = 'C:\\Users\\me\\.claude\\sessions';
  const files = (contents: Record<string, string>): Files => ({
    list: () => Object.keys(contents),
    read: (file) => contents[file.slice(dir.length + 1)] ?? assert.fail(`無いファイルを読んだ: ${file}`),
  });
  // PID が前方一致するだけのほかのセッションの鍵と、セッションファイルは選ばない。
  const sessions = files({
    '123.json': '{}',
    '1234.aaaa.key': JSON.stringify({ peerToken: 'other' }),
    '123.bbbb.key': JSON.stringify({ peerToken: 'mine', procStartFt: '1', pidDomain: 'win32:core' }),
  });
  assert.equal(win.inboxToken(123, dir, sessions), 'mine');
  assert.throws(() => win.inboxToken(999, dir, sessions), /鍵がありません/);
  assert.throws(() => win.inboxToken(123, dir, files({ '123.bbbb.key': JSON.stringify({ token: 'renamed' }) })), /鍵の形が変わって/);
});

test('supervisor の制御ソケットは、POSIX では設定の場所のハッシュで分けた /tmp の下に、Windows では pipe.key の値で名付けた名前付きパイプにある', () => {
  for (const name of ['darwin', 'linux']) assert.equal(platformFor(name).supervisorSocket('/home/me/.claude'), `/tmp/cc-daemon-${process.getuid?.() ?? 0}/cb67f5c9/control.sock`);
  const pipeKey = (key: string): Files => ({ list: () => [], read: (file) => (file === 'C:\\Users\\me\\.claude\\daemon\\pipe.key' ? key : assert.fail(`無いファイルを読んだ: ${file}`)) });
  const win = platformFor('win32');
  assert.equal(win.supervisorSocket('C:\\Users\\me\\.claude', pipeKey('0123456789abcdef\n')), '\\\\.\\pipe\\cc-daemon-0123456789abcdef-control');
  assert.throws(() => win.supervisorSocket('C:\\Users\\me\\.claude', pipeKey('')), /pipe\.key の形が変わって/);
});

test('macOS では、パスをそのまま使い、PID は headersHelper に任せ、メモリを sysctl と vm_stat から読む', async () => {
  const mac = platformFor('darwin');
  assert.equal(mac.shellPath('/Users/me/.durun-dashboard'), '/Users/me/.durun-dashboard');
  assert.equal(mac.defaultChromePath({}), '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome');
  assert.equal(mac.localSocketPath('/tmp/test-abc', 'inbox'), '/tmp/test-abc/inbox.sock');
  assert.equal(mac.headersHelperSendsPid, true);
  // Unix ソケットには認証が要らないので、鍵を読みに行かない。
  assert.equal(mac.inboxToken(123, '/sessions', { list: () => assert.fail('読まない'), read: () => assert.fail('読まない') }), undefined);

  const { run, calls } = fakeRun({
    '/usr/sbin/sysctl': '2\n40\n17179869184\ntotal = 2048.00M  used = 512.00M  free = 1536.00M  (encrypted)\n',
    '/usr/bin/vm_stat': 'Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages wired down:                        100.\n',
  });
  const memory = await mac.readMemory(run);
  assert.deepEqual(calls.map((c) => c.split(' ')[0]).sort(), ['/usr/bin/vm_stat', '/usr/sbin/sysctl']);
  assert.equal(memory?.level, 'warn');
  assert.equal(memory?.kind, 'pressure');
  assert.equal(memory?.percent, 60);
  assert.equal(memory?.wired, 100 * 16384);
  assert.equal(memory?.swapUsed, 512 * 1024 * 1024);
});

test('load average は、POSIX では直近 1 分の値を返し、Node が負荷によらず 0 を返す Windows では測れないものとする', () => {
  for (const name of ['darwin', 'linux']) assert.equal(typeof platformFor(name).loadAverage(), 'number');
  assert.equal(platformFor('win32').loadAverage(), undefined);
});

test('ほかの OS では、Chrome の場所とメモリを分からないものとして返す', async () => {
  const linux = platformFor('linux');
  assert.equal(linux.name, 'other');
  assert.equal(linux.defaultChromePath({}), undefined);
  assert.equal(await linux.readMemory(), null);
  assert.equal(linux.headersHelperSendsPid, true);
});

test('画面に渡すパスは、Windows では区切りを / にそろえ、エクスプローラーの最上位には、Windows ではつながっているドライブを、ほかの OS では / を並べる', () => {
  const win = platformFor('win32');
  assert.equal(win.displayPath('C:\\Users\\me\\dev'), 'C:/Users/me/dev');
  const asked: string[] = [];
  const exists = (dir: string) => {
    asked.push(dir);
    return dir === 'C:\\' || dir === 'E:\\';
  };
  assert.deepEqual(win.fileRoots(exists), ['C:/', 'E:/']);
  assert.equal(asked.length, 26);

  for (const name of ['darwin', 'linux']) {
    const p = platformFor(name);
    assert.equal(p.displayPath('/Users/me/dev'), '/Users/me/dev');
    assert.deepEqual(p.fileRoots(() => assert.fail('確かめない')), ['/']);
  }
});

const TAILSCALE_STATUS =JSON.stringify({ BackendState: 'Running', Self: { HostName: 'Core PC', DNSName: 'core.tail1234.ts.net.', TailscaleIPs: ['100.88.0.1', 'fd7a:115c:a1e0::1'] } });
const TAILNET_HOSTS = ['core.tail1234.ts.net', 'core', '100.88.0.1', '[fd7a:115c:a1e0::1]'];

test('tailscale の状態から、この端末の MagicDNS の名前と IP を取り出し、つながっていなければ空にする', () => {
  assert.deepEqual(hostsInTailscaleStatus(TAILSCALE_STATUS), TAILNET_HOSTS);
  assert.deepEqual(hostsInTailscaleStatus(JSON.stringify({ BackendState: 'Stopped', Self: { DNSName: 'core.tail1234.ts.net.', TailscaleIPs: [] } })), []);
  assert.deepEqual(hostsInTailscaleStatus(JSON.stringify({ BackendState: 'Running', Self: { DNSName: '', TailscaleIPs: ['100.88.0.1'] } })), ['100.88.0.1']);
});

test('tailnet での名前は、どの OS でも tailscale の CLI に聞き、macOS ではアプリの中の CLI も試し、CLI が無ければ空にする', async () => {
  const MAC_APP = '/Applications/Tailscale.app/Contents/MacOS/Tailscale';
  for (const name of ['win32', 'darwin', 'linux']) {
    const { run, calls } = fakeRun({ tailscale: TAILSCALE_STATUS });
    assert.deepEqual(await platformFor(name).tailnetHosts(run), TAILNET_HOSTS);
    assert.deepEqual(calls, ['tailscale status --json --peers=false']);
  }
  const tried: string[] = [];
  const onlyAt =
    (found: string): Run =>
    async (file) => {
      tried.push(file);
      if (file !== found) throw new Error('ENOENT');
      return TAILSCALE_STATUS;
    };
  assert.deepEqual(await platformFor('darwin').tailnetHosts(onlyAt(MAC_APP)), TAILNET_HOSTS);
  assert.deepEqual(tried, ['tailscale', MAC_APP]);
  assert.deepEqual(await platformFor('win32').tailnetHosts(onlyAt(MAC_APP)), []);
  assert.deepEqual(await platformFor('linux').tailnetHosts(onlyAt(MAC_APP)), []);
});

test('MCP の呼び出しには、headersHelper が PID を送れない OS でだけ、接続の持ち主の PID を補う', async () => {
  const request = (): IncomingMessage => ({ headers: { 'x-claude-pid': '$PPID' }, socket: { remotePort: 50692, localPort: 7337 } }) as unknown as IncomingMessage;
  const lookups: number[][] = [];
  const withOwner = (headersHelperSendsPid: boolean, owner: number | undefined): Platform => ({
    ...platformFor('win32'),
    headersHelperSendsPid,
    connectionOwner: async (clientPort, serverPort) => {
      lookups.push([clientPort, serverPort]);
      return owner;
    },
  });

  const filled = request();
  await fillCallerPid(filled, withOwner(false, 33544));
  // 同じ接続の 2 回目は、持ち主を引き直さない。
  await fillCallerPid(filled, withOwner(false, 1));
  assert.equal(filled.headers['x-claude-pid'], '33544');
  assert.deepEqual(lookups, [[50692, 7337]]);

  const unknown = request();
  await fillCallerPid(unknown, withOwner(false, undefined));
  assert.equal(unknown.headers['x-claude-pid'], '$PPID');

  const sent = request();
  await fillCallerPid(sent, withOwner(true, 33544));
  assert.equal(sent.headers['x-claude-pid'], '$PPID');
});

test('設定に書いたコマンドの行は、Windows では cmd.exe に行のまま渡し、ほかの OS では sh -c で動かす', () => {
  const line = 'tmux new-session -A -s "$(basename "$PWD")"';
  assert.deepEqual(platformFor('darwin').shellCommand(line), { file: '/bin/sh', args: ['-c', line] });
  assert.deepEqual(platformFor('other').shellCommand(line), { file: '/bin/sh', args: ['-c', line] });
  assert.deepEqual(platformFor('win32').shellCommand('wsl tmux new -A -s "a b"'), { file: 'cmd.exe', args: '/d /s /c "wsl tmux new -A -s "a b""' });
});

test('シェルで動かしたコマンドは、Windows では taskkill でプロセスの木ごと止め、ほかの OS ではシェルを止める', () => {
  const fakeChild = (pid: number | undefined) => {
    const signals: string[] = [];
    const child = { pid, kill: (signal: string = 'SIGTERM') => (signals.push(signal), true) } as unknown as ChildProcess;
    return { child, signals };
  };
  const win = fakeRun({});
  const started = fakeChild(4321);
  platformFor('win32').killTree(started.child, win.run);
  assert.deepEqual(win.calls, ['taskkill /pid 4321 /T /F']);
  assert.deepEqual(started.signals, []);
  // 起動できず PID の無いプロセスには、何もしない。
  platformFor('win32').killTree(fakeChild(undefined).child, win.run);
  assert.equal(win.calls.length, 1);
  for (const name of ['darwin', 'other']) {
    const { child, signals } = fakeChild(4321);
    platformFor(name).killTree(child);
    assert.deepEqual(signals, ['SIGTERM']);
  }
});

test('FL Connect は、Windows では FL のユーザーデータをドキュメントの下と各ドライブの直下から探し、FL 2026 の起動とループバックの作成を Program Files の道具で行う', async () => {
  const win = platformFor('win32');
  assert.deepEqual(win.flUserDataCandidates('C:\\Users\\me\\OneDrive\\ドキュメント', (d) => d === 'C:\\' || d === 'D:\\'), [
    'C:\\Users\\me\\OneDrive\\ドキュメント\\Image-Line\\FL Studio',
    'C:\\Image-Line\\FL Studio',
    'D:\\Image-Line\\FL Studio',
  ]);
  assert.equal(win.flStudioPath({ PROGRAMFILES: 'C:\\Program Files' }), 'C:\\Program Files\\Image-Line\\FL Studio 2026\\FL64.exe');
  assert.equal(win.flStudioPath({ FL_STUDIO_PATH: 'E:\\FL\\FL64.exe' }), 'E:\\FL\\FL64.exe');
  assert.deepEqual(win.midiLoopbackCommand({ PROGRAMFILES: 'C:\\Program Files' }), {
    file: 'C:\\Program Files\\Windows MIDI Services\\Tools\\midi.exe',
    args: ['loopback', 'create', '--root-name', 'FL Connect', '--unique-identifier', 'flconnect', '--save-to-config'],
  });

  const running = fakeRun({ tasklist: '\r\nFL64.exe                     12345 Console                    1    812,345 K\r\n' });
  assert.equal(await win.isFlRunning(running.run), true);
  assert.deepEqual(running.calls, ['tasklist /FI IMAGENAME eq FL64.exe /NH']);
  const stopped = fakeRun({ tasklist: '情報: 指定された条件に一致するタスクは実行されていません。\r\n' });
  assert.equal(await win.isFlRunning(stopped.run), false);

  const docs = fakeRun({ 'powershell.exe': 'C:\\Users\\me\\OneDrive\\ドキュメント\r\n' });
  assert.equal(await win.documentsDir(docs.run), 'C:\\Users\\me\\OneDrive\\ドキュメント');
  assert.equal(await win.documentsDir(async () => Promise.reject(new Error('no powershell'))), undefined);
});

test('FL Connect は、Windows 以外では FL のユーザーデータをドキュメントの下だけで探し、② の道のための FL の起動とループバックの作成はしない', async () => {
  for (const name of ['darwin', 'other']) {
    const p = platformFor(name);
    assert.deepEqual(p.flUserDataCandidates('/Users/me/Documents'), ['/Users/me/Documents/Image-Line/FL Studio']);
    assert.equal(p.flStudioPath({ FL_STUDIO_PATH: '/x' }), undefined);
    assert.equal(p.midiLoopbackCommand({}), undefined);
    assert.equal(await p.isFlRunning(), undefined);
    assert.match((await p.documentsDir()) ?? '', /Documents$/);
  }
});

test('FL Connect は、Windows では開いているプロジェクトの手がかりを、FL のウィンドウの題名とレジストリの最近のファイルから読み、ほかの OS では読まない', async () => {
  const hints = fakeRun({ 'powershell.exe': '{"recent":["D:\\\\FL\\\\曲.flp","D:\\\\FL\\\\old.flp"],"title":"曲 - FL Studio 2026"}\r\n' });
  assert.deepEqual(await platformFor('win32').flProjectHints(26, hints.run), { title: '曲 - FL Studio 2026', recent: ['D:\\FL\\曲.flp', 'D:\\FL\\old.flp'] });
  assert.match(hints.calls[0] ?? '', /HKCU:\\Software\\Image-Line\\FL Studio 26\\MRU/);
  assert.equal(await platformFor('win32').flProjectHints(26, async () => Promise.reject(new Error('no powershell'))), undefined);
  assert.equal(await platformFor('win32').flProjectHints(Number.NaN, hints.run), undefined);
  // PowerShell は、最近のファイルが 1 つなら配列にしないことがある。
  assert.deepEqual(parseFlProjectHints('{"recent":"D:\\\\a.flp","title":"a - FL Studio 2026"}'), { title: 'a - FL Studio 2026', recent: ['D:\\a.flp'] });
  assert.deepEqual(parseFlProjectHints('{"title":"FL Studio 2026"}'), { title: 'FL Studio 2026', recent: [] });
  for (const name of ['darwin', 'other']) assert.equal(await platformFor(name).flProjectHints(26), undefined);
});

test('Claude Code の認証情報は、Mac ではキーチェーンから、ほかの OS では ~/.claude/.credentials.json から読み、読めなければ undefined を返す', async () => {
  const { run, calls } = fakeRun({ '/usr/bin/security': '{"mcpOAuth":{}}' });
  assert.equal(await platformFor('darwin').claudeCredentials(run), '{"mcpOAuth":{}}');
  assert.deepEqual(calls, ['/usr/bin/security find-generic-password -s Claude Code-credentials -w']);
  const failing: Run = async () => {
    throw new Error('not found');
  };
  assert.equal(await platformFor('darwin').claudeCredentials(failing), undefined);
  const read: string[] = [];
  const files: Files = { list: () => [], read: (file) => (read.push(file), '{"x":1}') };
  for (const name of ['win32', 'other']) assert.equal(await platformFor(name).claudeCredentials(run, files), '{"x":1}');
  assert.ok(read.every((f) => f.endsWith('.credentials.json')));
  const missing: Files = { list: () => [], read: () => { throw new Error('ENOENT'); } };
  assert.equal(await platformFor('win32').claudeCredentials(run, missing), undefined);
});

test('Slack のアプリの状態は、OS ごとの置き場所の root-state.json から読み、読めなければ undefined を返す', () => {
  const read: string[] = [];
  const files: Files = { list: () => [], read: (file) => (read.push(file.replaceAll('\\', '/')), '{}') };
  assert.equal(platformFor('darwin').slackAppState(files), '{}');
  assert.equal(platformFor('other').slackAppState(files), '{}');
  assert.equal(platformFor('win32').slackAppState(files, { APPDATA: 'C:\\Users\\me\\AppData\\Roaming' }), '{}');
  assert.equal(platformFor('win32').slackAppState(files, {}), undefined);
  assert.deepEqual(
    read.map((f) => f.split('/').slice(-4).join('/')),
    ['Application Support/Slack/storage/root-state.json', '.config/Slack/storage/root-state.json', 'Roaming/Slack/storage/root-state.json'],
  );
  const missing: Files = { list: () => [], read: () => { throw new Error('ENOENT'); } };
  assert.equal(platformFor('darwin').slackAppState(missing), undefined);
});

test('claude は、偽物が指されていれば node でそれを動かし、なければ PATH の claude を起動する', () => {
  assert.deepEqual(claudeCommand(['--bg', '--', '依頼'], {}), { file: 'claude', args: ['--bg', '--', '依頼'] });
  assert.deepEqual(claudeCommand(['stop', 'j1'], { DURUN_DASHBOARD_FAKE_CLAUDE: '/tmp/fake.mjs' }), { file: process.execPath, args: ['/tmp/fake.mjs', 'stop', 'j1'] });
});

test('OS の判定と claude の起動は、互換レイヤーの外に書かない', () => {
  // 外に書くと、その OS でしか通らないコードが、もう片方の OS のテストをすり抜ける。
  const OS_CHECK = /process\.platform|os\.(platform|type|release)\(|path\.sep\b|process\.env\.(OS|OSTYPE)\b/;
  const CLAUDE_SPAWN = /(spawn|execFile)\(\s*'claude'/;
  const root = path.join(import.meta.dirname, '..');
  const offenders: string[] = [];
  for (const dir of ['server', 'test', 'shared']) {
    for (const name of fs.readdirSync(path.join(root, dir)).filter((f) => f.endsWith('.ts'))) {
      const file = `${dir}/${name}`;
      if (file === 'server/platform.ts' || file === 'test/platform.test.ts') continue;
      fs.readFileSync(path.join(root, file), 'utf8').split('\n').forEach((line, i) => {
        if (OS_CHECK.test(line) || CLAUDE_SPAWN.test(line)) offenders.push(`${file}:${i + 1}: ${line.trim()}`);
      });
    }
  }
  assert.deepEqual(offenders, []);
});

test('光学ドライブを、Windows は PowerShell の JSON から（1 台でも配列にそろえて）、macOS は drutil list から、Linux は lsblk から読む', async () => {
  const win = fakeRun({ 'powershell.exe': '{"Drive":"F:","Name":"PIONEER  BD-RW   BDR-UD04 1.11"}' });
  assert.deepEqual(await platformFor('win32').opticalDrives(win.run), [{ id: 'F:', model: 'PIONEER BD-RW BDR-UD04 1.11' }]);
  assert.match(win.calls[0] ?? '', /Win32_CDROMDrive/);
  const two = fakeRun({ 'powershell.exe': '[{"Drive":"D:","Name":"A"},{"Drive":"E:","Name":"B"}]' });
  assert.deepEqual((await platformFor('win32').opticalDrives(two.run)).map((d) => d.id), ['D:', 'E:']);
  assert.deepEqual(await platformFor('win32').opticalDrives(fakeRun({}).run), []);

  const mac = fakeRun({ drutil: ' Vendor   Product           Rev   Bus       SupportLevel\n 1  MATSHITA DVD-R   UJ-868    HA13  USB       Unsupported\n' });
  assert.deepEqual(await platformFor('darwin').opticalDrives(mac.run), [{ id: '1', model: 'MATSHITA DVD-R UJ-868' }]);

  const lsblk = JSON.stringify({ blockdevices: [{ name: 'sda', type: 'disk', model: 'SSD' }, { name: 'sr0', type: 'rom', model: 'BDR-UD04       ' }] });
  assert.deepEqual(await platformFor('linux').opticalDrives(fakeRun({ lsblk }).run), [{ id: '/dev/sr0', model: 'BDR-UD04' }]);
});

test('ディスクが入っているかを、Windows は MediaLoaded、macOS は drutil status の Type:、Linux は udevadm の ID_CDROM_MEDIA から読み、読めなければ undefined', async () => {
  const win = fakeRun({ 'powershell.exe': 'True\r\n' });
  assert.equal(await platformFor('win32').mediaLoaded('F:', win.run), true);
  assert.match(win.calls[0] ?? '', /Drive='F:'/);
  assert.equal(await platformFor('win32').mediaLoaded('F:', fakeRun({ 'powershell.exe': 'False\r\n' }).run), false);
  const failing: Run = async () => {
    throw new Error('x');
  };
  assert.equal(await platformFor('win32').mediaLoaded('F:', failing), undefined);

  const mac = platformFor('darwin');
  assert.equal(await mac.mediaLoaded('1', fakeRun({ drutil: '           Type: CD-ROM               Name: /dev/disk4\n' }).run), true);
  assert.equal(await mac.mediaLoaded('1', fakeRun({ drutil: '           Type: No Media Inserted\n' }).run), false);
  assert.equal(await mac.cdDevice('1', fakeRun({ drutil: '           Name: /dev/disk4\n' }).run), '/dev/disk4');
  assert.equal(await mac.cdDevice('/dev/disk9'), '/dev/disk9');

  const other = platformFor('linux');
  assert.equal(await other.mediaLoaded('/dev/sr0', fakeRun({ udevadm: 'E: ID_CDROM=1\nE: ID_CDROM_MEDIA=1\n' }).run), true);
  assert.equal(await other.mediaLoaded('/dev/sr0', fakeRun({ udevadm: 'E: ID_CDROM=1\n' }).run), false);
});

test('排出は OS ごとのコマンドで行い、失敗しても投げず、cyanrip に渡すデバイスは Windows と Linux ではドライブのまま', async () => {
  const win = fakeRun({});
  await platformFor('win32').ejectDisc('F:', win.run);
  assert.match(win.calls[0] ?? '', /ParseName\('F:'\)\.InvokeVerb\('Eject'\)/);
  const mac = fakeRun({});
  await platformFor('darwin').ejectDisc('1', mac.run);
  assert.deepEqual(mac.calls, ['drutil -drive 1 eject']);
  const linux = fakeRun({});
  await platformFor('linux').ejectDisc('/dev/sr0', linux.run);
  assert.deepEqual(linux.calls, ['eject /dev/sr0']);
  await platformFor('linux').ejectDisc('/dev/sr0', async () => {
    throw new Error('busy');
  });
  assert.equal(await platformFor('win32').cdDevice('F:'), 'F:');
  assert.equal(await platformFor('linux').cdDevice('/dev/sr0'), '/dev/sr0');
});

test('音楽フォルダーを、Windows は既知のフォルダーから UTF-8 で引き、読めなければホームの Music にする', async () => {
  const win = fakeRun({ 'powershell.exe': 'D:\\Music\r\n' });
  assert.equal(await platformFor('win32').musicDir(win.run), 'D:\\Music');
  assert.match(win.calls[0] ?? '', /GetFolderPath\('MyMusic'\)/);
  assert.match(await platformFor('win32').musicDir(fakeRun({}).run), /Music$/);
  assert.match(await platformFor('darwin').musicDir(), /Music$/);
});

/** winget のパッケージの置き場所を、ファイルの一覧で表す偽物。 */
const wingetDirs = (files: string[]): { exists: (f: string) => boolean; list: (d: string) => string[] } => {
  const all = new Set(files);
  return {
    exists: (f) => all.has(f),
    list: (dir) => [...new Set(files.filter((f) => f.startsWith(`${dir}\\`)).map((f) => f.slice(dir.length + 1).split('\\')[0] ?? ''))],
  };
};

test('Windows では、winget のパッケージの下から、版の階層を * で広げて依存ツールを探し、新しい版を先に返す', () => {
  const root = 'C:\\L\\Microsoft\\WinGet\\Packages';
  const cyan = `${root}\\cyanreg.cyanrip_Microsoft.Winget.Source_8wekyb3d8bbwe`;
  const flac = `${root}\\Xiph.FLAC_Microsoft.Winget.Source_8wekyb3d8bbwe`;
  const dirs = wingetDirs([
    `${cyan}\\cyanrip.exe`,
    `${flac}\\flac-1.4.0-win\\Win64\\flac.exe`,
    `${flac}\\flac-1.5.0-win\\Win64\\flac.exe`,
    `${flac}\\flac-1.5.0-win\\Win64\\metaflac.exe`,
    `${root}\\yt-dlp.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe\\ffmpeg-N-1-win64-gpl\\bin\\ffmpeg.exe`,
  ]);
  const win = platformFor('win32');
  const env = { LOCALAPPDATA: 'C:\\L' };
  assert.deepEqual(win.toolCandidates('cyanrip', env, dirs), [`${cyan}\\cyanrip.exe`]);
  assert.deepEqual(win.toolCandidates('flac', env, dirs), [`${flac}\\flac-1.5.0-win\\Win64\\flac.exe`, `${flac}\\flac-1.4.0-win\\Win64\\flac.exe`]);
  assert.equal(win.toolCandidates('metaflac', env, dirs).length, 1);
  assert.equal(win.toolCandidates('ffmpeg', env, dirs).length, 1);
  assert.deepEqual(win.toolCandidates('ffprobe', env, dirs), []);
  assert.equal(win.executableName('flac'), 'flac.exe');
  assert.equal(platformFor('darwin').executableName('flac'), 'flac');
});

test('macOS と Linux では、パッケージ管理が置くディレクトリから依存ツールを探す', () => {
  const dirs = { exists: (f: string) => f === '/opt/homebrew/bin/flac' || f === '/usr/bin/flac', list: () => [] };
  assert.deepEqual(platformFor('darwin').toolCandidates('flac', {}, dirs), ['/opt/homebrew/bin/flac']);
  assert.deepEqual(platformFor('linux').toolCandidates('flac', {}, dirs), ['/usr/bin/flac']);
});

test('依存ツールを入れるコマンドを、Windows は winget、macOS は brew、Linux は apt（管理者権限が要るので実行せず見せるだけ）で返し、metaflac は flac のパッケージを入れる', () => {
  const win = platformFor('win32').toolInstall('metaflac');
  assert.deepEqual([win.file, win.runnable], ['winget', true]);
  assert.equal(win.command, 'winget install --id Xiph.FLAC -e --accept-source-agreements --accept-package-agreements');
  assert.equal(platformFor('win32').toolInstall('cyanrip').args[2], 'cyanreg.cyanrip');
  const mac = platformFor('darwin').toolInstall('ffprobe');
  assert.deepEqual([mac.command, mac.runnable], ['brew install ffmpeg', true]);
  const apt = platformFor('linux').toolInstall('yt-dlp');
  assert.deepEqual([apt.command, apt.runnable], ['sudo apt install yt-dlp', false]);
});
