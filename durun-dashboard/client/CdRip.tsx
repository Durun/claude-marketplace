// アプリ「CD取り込み」のウィンドウ。取り込みリストを貼って受付を始め、CD を順に入れると、取り込み・情報収集・書き出しが進む。
// サーバーとは SSE でつなぎ、状態（view）を受け取る。切れたら、つなぎ直す。
import { useEffect, useState } from 'preact/hooks';
import type { CdFeatures, CdMeta, CdRipItem, CdRipSettings, CdRipView, CdTrack } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { type CdStep, nextSteps } from './cdripSteps.ts';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { toast } from './toast.tsx';
import { Chip, Empty } from './ui.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。光学ドライブは 1 台ずつ動かすので、ウィンドウは 1 つだけ開く。 */
export const CDRIP_KEY = 'cdrip';
export const CDRIP_TITLE = 'CD取り込み';
export const CDRIP_ICON = '/cdrip.svg';
const SIZE = { w: 900, h: 640 };
/** 接続が切れてから、つなぎ直すまでの時間。 */
const RECONNECT_MS = 2000;

export const openCdRip = (): void => floatUp(CDRIP_KEY, '', { id: CDRIP_KEY }, SIZE);

export const CdRipIcon = () => (
  <DesktopIcon icon={CDRIP_ICON} title={CDRIP_TITLE} hint="取り込みリストを貼って受付を始め、CD を順に入れると、FLAC と mp3 に取り込みます。ダブルクリックで開きます" onLaunch={openCdRip} />
);

/** サーバーに SSE でつなぎ、届いた状態を返す。切れたら RECONNECT_MS 後につなぎ直す。 */
const useCdRipView = (): CdRipView | undefined => {
  const [view, setView] = useState<CdRipView>();
  useEffect(() => {
    let source: EventSource | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let live = true;
    const connect = (): void => {
      source = new EventSource(api.cdripEventsUrl);
      source.addEventListener('view', (e) => setView(JSON.parse((e as MessageEvent<string>).data) as CdRipView));
      source.onerror = () => {
        source?.close();
        if (live) timer = setTimeout(connect, RECONNECT_MS);
      };
    };
    connect();
    return () => {
      live = false;
      clearTimeout(timer);
      source?.close();
    };
  }, []);
  return view;
};

const STATUS: Record<CdRipItem['status'], { glyph: string; label: string }> = {
  waiting: { glyph: '○', label: '待機' },
  ripping: { glyph: '◐', label: '取り込み中' },
  researching: { glyph: '◔', label: '情報収集中' },
  review: { glyph: '●', label: '確認待ち' },
  exporting: { glyph: '⏳', label: '書き出し中' },
  done: { glyph: '☑', label: '書き出し済み' },
  failed: { glyph: '✕', label: '失敗' },
  skipped: { glyph: '—', label: 'スキップ' },
};

const glyphOf = (i: CdRipItem): string => (i.attention && i.status !== 'failed' ? '⚠' : STATUS[i.status].glyph);

const statusText = (i: CdRipItem): string => (i.status === 'ripping' && i.progress ? `取り込み中 ${i.progress.track}/${i.progress.total} 曲` : STATUS[i.status].label);

/** 待機中の行を、貼ったときの形（## グループ名の行つき）に戻す。 */
const listText = (items: CdRipItem[]): string => {
  let group: string | undefined;
  const lines: string[] = [];
  for (const i of items.filter((x) => x.status === 'waiting')) {
    if (i.group !== group) lines.push(`## ${i.group ?? ''}`);
    group = i.group;
    lines.push(i.hint);
  }
  return lines.join('\n');
};

const clock = (at: number): string => new Date(at).toTimeString().slice(0, 8);

/** 導入で次にすること。 */
const NextSteps = ({ steps, onInstall }: { steps: CdStep[]; onInstall: (step: CdStep) => void }) =>
  steps.length ? (
    <div class="fl-steps">
      {steps.map((s) => (
        <div key={s.key} class={`fl-step is-${s.tone}`} role={s.tone === 'warn' ? 'alert' : undefined}>
          <span class="fl-step-icon" aria-hidden="true">{s.tone === 'warn' ? '⚠' : 'ℹ'}</span>
          <div class="fl-step-body">
            <strong>{s.title}</strong>
            {s.text && <p>{s.text}</p>}
            {s.command && <pre class="fl-pre">{s.command}</pre>}
          </div>
          {s.action && (
            <button class="btn btn-sm" onClick={() => onInstall(s)}>
              {s.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  ) : null;

const MEDIA = { loaded: 'ディスクあり', none: 'ディスクなし', unknown: '状態不明' } as const;

/** ドライブの状態と、受付の開始と停止。 */
const DriveBar = ({ v, settingsOpen, onSettings }: { v: CdRipView; settingsOpen: boolean; onSettings: () => void }) => (
  <div class="cdrip-bar">
    <span class="cdrip-drive" title={`ドライブ ${v.drive.id || '（未設定）'}、読み取りオフセット ${v.drive.offset}`}>{v.drive.model || '光学ドライブ未設定'} {v.drive.id}</span>
    <Chip tone={v.drive.media === 'loaded' ? 'ok' : 'muted'}>{MEDIA[v.drive.media]}</Chip>
    <button class="btn btn-sm" disabled={v.drive.media !== 'loaded'} title="ディスクを排出する" onClick={() => void attempt('排出', api.cdripEject)}>排出</button>
    <button class={`btn btn-sm ${v.accepting ? '' : 'btn-primary'}`} onClick={() => void attempt(v.accepting ? '受付の停止' : '受付の開始', () => api.cdripAccepting(!v.accepting))}>
      {v.accepting ? '受付停止' : '受付開始'}
    </button>
    <button class="btn btn-sm" aria-pressed={settingsOpen} title="設定" aria-label="設定" onClick={onSettings}>⚙</button>
  </div>
);

/** 取り込みリスト。1 行 1 枚で、## で始まる行がグループ名。 */
const ListInput = ({ v }: { v: CdRipView }) => {
  const [text, setText] = useState('');
  const [edited, setEdited] = useState(false);
  const waiting = listText(v.items);
  // 受付の前は、サーバーの待機中の行を映す。書きかけの間は、サーバーの更新で消さない。
  useEffect(() => {
    if (!edited && !v.accepting) setText(waiting);
  }, [waiting, edited, v.accepting]);
  const submit = async (): Promise<void> => {
    const ok = await attempt(v.accepting ? 'リストへの追記' : 'リストの反映', () => api.cdripList(text));
    if (ok === undefined) return;
    setEdited(false);
    if (v.accepting) setText('');
  };
  return (
    <div class="cdrip-input">
      <textarea
        class="input"
        rows={6}
        value={text}
        placeholder={'1 行に 1 枚。URL でもサークル名でもよい。\n## で始まる行は、次の ## までの行のグループ名（イベント名など）。'}
        onInput={(e) => {
          setText(e.currentTarget.value);
          setEdited(true);
        }}
      />
      <button class="btn btn-sm" disabled={!edited} onClick={() => void submit()}>
        {v.accepting ? '追記' : 'リストを反映'}
      </button>
    </div>
  );
};

const ItemList = ({ v, selected, onSelect }: { v: CdRipView; selected?: string; onSelect: (id: string) => void }) => {
  let group: string | undefined;
  return v.items.length ? (
    <ul class="cdrip-items">
      {v.items.map((i) => {
        const heading = i.group !== group ? i.group : undefined;
        group = i.group;
        return (
          <>
            {heading && <li key={`g-${i.id}`} class="cdrip-group">{heading}</li>}
            <li key={i.id}>
              <button class={`cdrip-item is-${i.status}${selected === i.id ? ' is-selected' : ''}`} onClick={() => onSelect(i.id)}>
                <span class="cdrip-glyph" aria-hidden="true">{glyphOf(i)}</span>
                <span class="cdrip-hint">{i.meta?.album ? `${i.meta.circle} / ${i.meta.album}` : i.hint}</span>
                <span class="cdrip-state">{statusText(i)}</span>
              </button>
            </li>
          </>
        );
      })}
    </ul>
  ) : (
    <Empty>リストを貼って「受付開始」を押し、CD を入れてください。</Empty>
  );
};

const mmss = (sec: number | undefined): string => (sec === undefined ? '' : `${Math.floor(sec / 60)}:${String(Math.round(sec % 60)).padStart(2, '0')}`);

const emptyMeta: CdMeta = { circle: '', album: '', artist: '', dest: 'CLUB', tracks: [] };

/** 曲情報の入力。曲の数が CD と合わない行は赤くする。書き出すときは、保存してから書き出す。 */
const MetaForm = ({ item }: { item: CdRipItem }) => {
  const [draft, setDraft] = useState<CdMeta>(item.meta ?? emptyMeta);
  const editable = item.status === 'review' || item.status === 'researching';
  const discTracks = item.disc?.tracks;
  const set = (patch: Partial<CdMeta>) => setDraft({ ...draft, ...patch });
  const setTrack = (i: number, patch: Partial<CdTrack>) => set({ tracks: draft.tracks.map((t, k) => (k === i ? { ...t, ...patch } : t)) });
  const save = (): Promise<unknown> => attempt('曲情報の保存', () => api.cdripMeta(item.id, draft));
  const field = (label: string, key: 'circle' | 'album' | 'artist' | 'date' | 'genre' | 'catalog') => (
    <label class="cdrip-field">
      {label}
      <input class="input input-sm" disabled={!editable} value={draft[key] ?? ''} onInput={(e) => set({ [key]: e.currentTarget.value })} />
    </label>
  );
  const rows = Math.max(draft.tracks.length, discTracks ?? 0);
  return (
    <>
      <div class="cdrip-meta">
        {field('サークル', 'circle')}
        {field('アルバム', 'album')}
        {field('アーティスト', 'artist')}
        {field('日付', 'date')}
        {field('ジャンル', 'genre')}
        {field('品番', 'catalog')}
        <label class="cdrip-field">
          置き場所
          <select class="input input-sm" disabled={!editable} value={draft.dest} onChange={(e) => set({ dest: e.currentTarget.value as CdMeta['dest'] })}>
            <option value="CLUB">CLUB</option>
            <option value="Music">Music</option>
          </select>
        </label>
        <label class="cdrip-field">
          枚数（複数枚組のとき）
          <span class="cdrip-disc">
            <input class="input input-sm" type="number" min={1} disabled={!editable} value={draft.disc ?? ''} onInput={(e) => set({ disc: e.currentTarget.value ? Number(e.currentTarget.value) : undefined })} /> /
            <input class="input input-sm" type="number" min={1} disabled={!editable} value={draft.totaldiscs ?? ''} onInput={(e) => set({ totaldiscs: e.currentTarget.value ? Number(e.currentTarget.value) : undefined })} />
          </span>
        </label>
      </div>
      <Cover item={item} editable={editable} />
      <table class="cdrip-tracks">
        <thead>
          <tr><th>#</th><th>曲名</th><th>アーティスト</th><th>作曲</th><th>長さ</th></tr>
        </thead>
        <tbody>
          {Array.from({ length: rows }, (_, i) => {
            const t = draft.tracks[i];
            const bad = discTracks !== undefined && (i >= discTracks || !t);
            return (
              <tr key={i} class={bad ? 'is-bad' : undefined}>
                <td>{i + 1}</td>
                <td><input class="input input-sm" disabled={!editable || !t} value={t?.title ?? ''} onInput={(e) => setTrack(i, { title: e.currentTarget.value })} /></td>
                <td><input class="input input-sm" disabled={!editable || !t} value={t?.artist ?? ''} placeholder={draft.artist} onInput={(e) => setTrack(i, { artist: e.currentTarget.value })} /></td>
                <td><input class="input input-sm" disabled={!editable || !t} value={t?.composer ?? ''} onInput={(e) => setTrack(i, { composer: e.currentTarget.value })} /></td>
                <td class="cdrip-duration" title={t?.duration ? `曲情報の長さ ${mmss(t.duration)}` : undefined}>{mmss(item.disc?.durations[i])}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {editable && (
        <div class="fl-actions">
          <button class="btn btn-sm" onClick={() => set({ tracks: [...draft.tracks, { title: '' }] })}>曲を足す</button>
          <button class="btn btn-sm" disabled={!draft.tracks.length} onClick={() => set({ tracks: draft.tracks.slice(0, -1) })}>最後の曲を消す</button>
          <button class="btn btn-sm" onClick={() => void save()}>保存</button>
          {item.status === 'review' && (
            <button class="btn btn-sm btn-primary" disabled={!!discTracks && draft.tracks.length !== discTracks} onClick={() => void save().then((ok) => ok !== undefined && attempt('書き出し', () => api.cdripExport(item.id)))}>
              書き出す
            </button>
          )}
        </div>
      )}
    </>
  );
};

/** ジャケット。URL を貼って差し替える。 */
const Cover = ({ item, editable }: { item: CdRipItem; editable: boolean }) => {
  const [url, setUrl] = useState(item.meta?.coverUrl ?? '');
  return (
    <div class="cdrip-cover">
      {item.meta?.cover ? <img src={api.cdripCoverUrl(item.id, item.updatedAt)} alt="ジャケット" width={96} height={96} /> : <div class="cdrip-nocover">ジャケットなし</div>}
      {editable && (
        <label class="cdrip-field">
          ジャケットの画像の URL
          <span class="cdrip-disc">
            <input class="input input-sm" value={url} onInput={(e) => setUrl(e.currentTarget.value)} />
            <button class="btn btn-sm" disabled={!url || !item.meta} onClick={() => void attempt('ジャケットの取り込み', () => api.cdripCover(item.id, url))}>差し替え</button>
          </span>
        </label>
      )}
    </div>
  );
};

/** AI の情報収集。確信と出典と迷った点を見せ、指示を添えてやり直させる。 */
const Research = ({ item }: { item: CdRipItem }) => {
  const [hint, setHint] = useState('');
  const r = item.research;
  const running = !!r && !r.finishedAt;
  const redo = item.status === 'review' || item.status === 'researching';
  return (
    <div class="cdrip-research">
      {running && <p class="fl-note">AI が曲情報とジャケットを集めています…</p>}
      {r?.finishedAt && (
        <p class="fl-note">
          AI の確信: {r.confidence ?? '不明'}
          {r.sources.length ? ' / 出典: ' : ''}
          {r.sources.map((u, k) => (
            <a key={u} href={u} target="_blank" rel="noreferrer noopener">{k + 1}</a>
          ))}
        </p>
      )}
      {r?.notes && <p class="fl-note">{r.notes}</p>}
      {redo && (
        <div class="cdrip-redo">
          <input class="input input-sm" value={hint} placeholder="AI へのヒント（例: Bandcamp の URL、正しいアルバム名）" onInput={(e) => setHint(e.currentTarget.value)} />
          <button class="btn btn-sm" disabled={running} onClick={() => void attempt('AI のやり直し', () => api.cdripResearch(item.id, hint)).then(() => setHint(''))}>
            AI にやり直させる
          </button>
        </div>
      )}
    </div>
  );
};

const Detail = ({ item }: { item?: CdRipItem }) => {
  if (!item) return <Empty>左の行を選ぶと、詳細を出します。</Empty>;
  const act = (label: string, action: 'skip' | 'retry' | 'abort') => void attempt(label, () => api.cdripItem(item.id, action));
  return (
    <div class="cdrip-detail">
      <h3>{item.meta?.album || item.hint}</h3>
      <p class="fl-note">{item.hint}{item.group ? `（${item.group}）` : ''}</p>
      <p>{glyphOf(item)} {statusText(item)}</p>
      {item.attention && <p class="fl-warn" role="alert">{item.attention}</p>}
      <Research item={item} />
      {item.meta && <MetaForm key={`${item.id}:${JSON.stringify(item.meta)}`} item={item} />}
      {item.exported && (
        <details>
          <summary>書き出したファイル（{item.exported.files.length}）</summary>
          <pre class="fl-pre">{item.exported.files.join('\n')}</pre>
        </details>
      )}
      <div class="fl-actions">
        {item.status === 'ripping' && <button class="btn btn-sm" onClick={() => act('取り込みの中断', 'abort')}>取り込みを中断</button>}
        {['review', 'failed'].includes(item.status) && <button class="btn btn-sm" onClick={() => act('取り込みのやり直し', 'retry')}>取り込みをやり直す</button>}
        {['waiting', 'researching', 'review', 'failed'].includes(item.status) && <button class="btn btn-sm" onClick={() => act('スキップ', 'skip')}>スキップ</button>}
      </div>
    </div>
  );
};

const FLAG_LABELS: [keyof CdFeatures, string][] = [
  ['flacImage', 'FLAC イメージ + cue + log + jpg を CD に置く'],
  ['mp3', '曲ごとの mp3'],
  ['embedCover', 'ジャケットを埋め込む（長辺 1000px に縮小）'],
  ['mp3Gain', '小さい CD は mp3 だけ音量を持ち上げる（ピーク -6 dBFS 未満 → -1 dBFS）'],
  ['ai', 'AI で曲情報とジャケットを集める'],
  ['aiVideo', 'XFD 動画のフレームを AI に見せる（yt-dlp が要る）'],
  ['autoExport', '曲数が合い AI が確信しているときは確認せず書き出す'],
  ['rekordbox', '書き出しを rekordbox 用の XML に記録する'],
];

/** 設定。入力欄は離れたとき、チェックと選択は変えたときに保存する。 */
const Settings = ({ v }: { v: CdRipView }) => {
  const s: CdRipSettings = v.settings;
  const save = (patch: Record<string, unknown>) => void attempt('設定の保存', () => api.cdripSettings(patch));
  const text = (label: string, value: string, key: (x: string) => Record<string, unknown>) => (
    <label class="cdrip-field">
      {label}
      <input class="input input-sm" key={value} defaultValue={value} onBlur={(e) => e.currentTarget.value !== value && save(key(e.currentTarget.value))} />
    </label>
  );
  const number = (label: string, value: number, key: string) => (
    <label class="cdrip-field">
      {label}
      <input class="input input-sm" type="number" key={value} defaultValue={value} onBlur={(e) => e.currentTarget.value !== String(value) && save({ [key]: Number(e.currentTarget.value) })} />
    </label>
  );
  return (
    <div class="cdrip-settings">
      <h3>設定</h3>
      <label class="cdrip-field">
        ドライブ
        <select class="input input-sm" value={s.drive} onChange={(e) => save({ drive: e.currentTarget.value, driveModel: v.drives.find((d) => d.id === e.currentTarget.value)?.model ?? '' })}>
          {!v.drives.some((d) => d.id === s.drive) && <option value={s.drive}>{s.drive || '（未設定）'}</option>}
          {v.drives.map((d) => <option key={d.id} value={d.id}>{d.id} {d.model}</option>)}
        </select>
      </label>
      {number('読み取りオフセット（AccurateRip の値）', s.offset, 'offset')}
      {number('paranoia（0〜3）', s.paranoia, 'paranoia')}
      {text('取り込み先', s.stageDir, (x) => ({ stageDir: x }))}
      {text('書き出し先: CD（FLAC イメージ）', s.dest.cd, (x) => ({ dest: { cd: x } }))}
      {text('書き出し先: CLUB（mp3）', s.dest.club, (x) => ({ dest: { club: x } }))}
      {text('書き出し先: Music（mp3）', s.dest.music, (x) => ({ dest: { music: x } }))}
      <div class="cdrip-flags">
        {FLAG_LABELS.map(([key, label]) => (
          <label key={key}>
            <input type="checkbox" checked={s.features[key] as boolean} onChange={(e) => save({ features: { [key]: e.currentTarget.checked } })} /> {label}
          </label>
        ))}
      </div>
      <label class="cdrip-field">
        mp3 のビットレート
        <select class="input input-sm" value={s.features.mp3Bitrate} onChange={(e) => save({ features: { mp3Bitrate: e.currentTarget.value } })}>
          <option value="320k">320k</option>
          <option value="256k">256k</option>
          <option value="V0">V0</option>
        </select>
      </label>
      <label class="cdrip-field">
        取り込みの検証（cyanrip -Z。0 なら 1 回だけ読む）
        <input class="input input-sm" type="number" min={0} max={10} key={s.features.verify} defaultValue={s.features.verify} onBlur={(e) => e.currentTarget.value !== String(s.features.verify) && save({ features: { verify: Number(e.currentTarget.value) } })} />
      </label>
    </div>
  );
};

/** 書き出し済みの枚数と、rekordbox 用 XML のダウンロード。 */
const Footer = ({ v }: { v: CdRipView }) => {
  const done = v.items.filter((i) => i.status === 'done');
  const fresh = done.filter((i) => !i.exported?.rekordboxAt).length;
  return (
    <div class="cdrip-footer">
      <span>書き出し済み {done.length} 枚（rekordbox 未登録 {fresh} 枚）</span>
      {v.settings.features.rekordbox && (
        <>
          <a class={`btn btn-sm${fresh ? ' btn-primary' : ''}`} href="/api/cdrip/rekordbox.xml?scope=new" download="rekordbox_import.xml" aria-disabled={!fresh}>rekordbox XML をダウンロード</a>
          <a class="btn btn-sm" href="/api/cdrip/rekordbox.xml?scope=all" download="rekordbox_import.xml" title="渡した分も含めて、書き出し済みの全部">全部</a>
        </>
      )}
    </div>
  );
};

const Log = ({ v }: { v: CdRipView }) => (
  <details class="cdrip-log">
    <summary>ログ（{v.log.length}）</summary>
    <ul class="fl-log">
      {[...v.log].reverse().map((e, k) => (
        <li key={`${e.at}-${k}`} class="fl-log-item">
          <time>{clock(e.at)}</time> {e.text}
        </li>
      ))}
    </ul>
  </details>
);

export const CdRipFloat = ({ f }: { f: Float }) => {
  const v = useCdRipView();
  const [selected, setSelected] = useState<string>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const item = v?.items.find((i) => i.id === selected);
  const onInstall = (step: CdStep): void => {
    if (step.action) void attempt(step.action.label, () => api.cdripInstall(step.action?.tool ?? '')).then((r) => r && toast(r.message));
  };
  return (
    <FloatFrame f={f} title={CDRIP_TITLE} icon={CDRIP_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) }]}>
      <div class="widget-body cdrip">
        {!v ? (
          <Empty>ダッシュボードにつないでいます…</Empty>
        ) : (
          <>
            <NextSteps steps={nextSteps(v)} onInstall={onInstall} />
            <DriveBar v={v} settingsOpen={settingsOpen} onSettings={() => setSettingsOpen(!settingsOpen)} />
            <div class="cdrip-main">
              <div class="cdrip-left">
                <ItemList v={v} selected={selected} onSelect={(id) => { setSelected(id); setSettingsOpen(false); }} />
                <ListInput v={v} />
              </div>
              <div class="cdrip-right">{settingsOpen ? <Settings v={v} /> : <Detail item={item} />}</div>
            </div>
            <Footer v={v} />
            <Log v={v} />
          </>
        )}
      </div>
    </FloatFrame>
  );
};
