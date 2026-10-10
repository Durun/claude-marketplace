import { useMemo } from 'preact/hooks';
import type { Session, Subagent, Usage } from '../shared/types.ts';
import { dateTimeSeconds, fmtInt, fmtTokens, totalTokens } from './format.ts';
import { Timeline, type TimelinePoint } from './Timeline.tsx';
import { Chip, Empty, Meter, Panel, contextColor } from './ui.tsx';

// 値の文字が棒の右に収まる幅。最大の棒でも値がはみ出さないようにする。
const BAR_VALUE_SPACE_PX = 72;

type BarRow = { key: string; label: string; value: number };

const BarList = ({ rows, wide }: { rows: BarRow[]; wide?: boolean }) => {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <ul class={`barlist ${wide ? 'barlist-wide' : ''}`}>
      {rows.map((r) => (
        <li key={r.key} class="barrow">
          <span class="bar-label" title={r.label}>{r.label}</span>
          <span class="bar-track">
            <span class="bar-fill" style={`width: calc((100% - ${BAR_VALUE_SPACE_PX}px) * ${r.value / max})`} />
            <span class="bar-value" title={`${fmtInt(r.value)} トークン`}>{fmtTokens(r.value)}</span>
          </span>
        </li>
      ))}
    </ul>
  );
};

const ContextCard = ({ u }: { u: Usage }) => (
  <Panel title="コンテキスト">
    <div class="tiles">
      <div class="tile">
        <span class="tile-label">使用率</span>
        <span class="tile-value">{Math.round(u.contextPercent)}%</span>
        {!u.exact && <Chip title="会話ログとモデルの既定値から推定した値です">推定</Chip>}
      </div>
      <div class="tile">
        <span class="tile-label">トークン数</span>
        <span class="tile-value">{fmtInt(u.contextTokens)}</span>
        <span class="tile-sub">上限 {fmtInt(u.contextWindow)}</span>
      </div>
      {u.model && (
        <div class="tile">
          <span class="tile-label">モデル</span>
          <span class="tile-value tile-text">{u.model}</span>
        </div>
      )}
    </div>
    <Meter percent={u.contextPercent} color={contextColor(u.contextPercent)} label="コンテキスト使用率" />
  </Panel>
);

const TimelineTable = ({ points, limit }: { points: TimelinePoint[]; limit: number }) => (
  <details class="table-view">
    <summary>表で見る（{points.length} 件）</summary>
    <div class="scroll-y">
      <table class="data">
        <thead>
          <tr><th>時刻</th><th class="num">トークン数</th><th class="num">使用率</th></tr>
        </thead>
        <tbody>
          {points.map(([at, tokens]) => (
            <tr key={at}>
              <td>{dateTimeSeconds(at)}</td>
              <td class="num">{fmtInt(tokens)}</td>
              <td class="num">{((tokens / limit) * 100).toFixed(1)}%</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  </details>
);

const TimelinePanel = ({ u }: { u: Usage }) => {
  const points = useMemo(() => [...u.timeline].sort((a, b) => a[0] - b[0]), [u.timeline]);
  return (
    <Panel title="コンテキストの推移（トークン数）">
      {points.length === 0 ? <Empty>推移のデータはまだありません。</Empty> : (
        <>
          <Timeline points={points} limit={u.contextWindow} />
          <TimelineTable points={points} limit={u.contextWindow} />
        </>
      )}
    </Panel>
  );
};

const TotalsPanel = ({ u }: { u: Usage }) => {
  const t = u.totals;
  return (
    <Panel title="トークンの合計">
      <p class="total-line">
        <b>{fmtInt(totalTokens(t))}</b> トークン
        {u.costUsd !== undefined && <span class="muted"> · 概算 ${u.costUsd.toFixed(2)}</span>}
      </p>
      <BarList
        rows={[
          { key: 'input', label: '入力', value: t.input },
          { key: 'output', label: '出力', value: t.output },
          { key: 'cacheRead', label: 'キャッシュ読取', value: t.cacheRead },
          { key: 'cacheCreation', label: 'キャッシュ作成', value: t.cacheCreation },
        ]}
      />
      <p class="help">メインスレッドとサブエージェントの合計です。</p>
    </Panel>
  );
};

const SubagentPanel = ({ subagents }: { subagents: Subagent[] }) => {
  const rows = [...subagents]
    .sort((a, b) => b.outputTokens - a.outputTokens)
    .map((a) => ({ key: a.id, label: `${a.description} (${a.type})`, value: a.outputTokens }));
  return (
    <Panel title="サブエージェントの出力トークン" count={subagents.length}>
      {rows.length === 0 ? <Empty>サブエージェントはいません。</Empty> : <BarList rows={rows} wide />}
    </Panel>
  );
};

export const UsageView = ({ s }: { s: Session }) => (
  <div class="usage">
    <ContextCard u={s.usage} />
    <TimelinePanel u={s.usage} />
    <div class="usage-cols">
      <TotalsPanel u={s.usage} />
      <SubagentPanel subagents={s.subagents} />
    </div>
  </div>
);
