import { EFFORT_LEVELS, MODELS, type ImproveSettings, type ImproveState } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { shortDateTime } from './format.ts';
import { href } from './router.ts';
import { Choice } from './ui.tsx';

const save = (patch: Partial<ImproveSettings>): void => {
  void attempt('自動実装の設定の変更', () => api.improve(patch));
};

/** 歯車から開く、改善のセッションのモデルと effort の指定。 */
export const ImproveOptions = ({ improve }: { improve: ImproveState }) => (
  <div class="fb-options">
    <Choice label="モデル" value={improve.model} options={MODELS} onChange={(model) => save({ model })} />
    <Choice label="effort" value={improve.effort} options={EFFORT_LEVELS} onChange={(effort) => save({ effort })} />
    <p class="help fb-help">モデルと effort は、次に起動するセッションから使います。既定は Claude Code の設定に従います。</p>
  </div>
);

const statusText = (i: ImproveState): string | undefined => {
  const rest = i.pending ? `（残り ${i.pending} 件）` : '';
  if (i.running && !i.enabled) return `対応中のウェーブを終えたら止まります: ${i.current ?? ''}`;
  if (i.running) return `対応中: ${i.current ?? ''}${rest}`;
  if (i.limitResetsAt) return `使用制限で止まっています。${shortDateTime(i.limitResetsAt, Date.now())} に制限が明けたら続けます: ${i.current ?? ''}${rest}`;
  if (i.enabled && i.error) return '待機しています。新しい要望が届くか「続ける」を押すと、次のウェーブを始めます。';
  if (i.enabled) return i.pending ? 'セッションを起動しています…' : '要望を送ると、自動で実装を始めます。';
  return i.pending ? `未対応の要望が ${i.pending} 件あります。` : undefined;
};

/** 自動実装の切り替えと、いまの進み具合。 */
export const ImproveStatus = ({ improve }: { improve: ImproveState }) => {
  const status = statusText(improve);
  return (
    <div class="fb-improve">
      <div class="actions">
        <label class="check">
          <input type="checkbox" role="switch" checked={improve.enabled} onChange={(e) => save({ enabled: e.currentTarget.checked })} />
          自動で実装する
        </label>
        <span class="spacer" />
        {improve.enabled && improve.error && !improve.running && <button class="btn btn-sm" onClick={() => save({ enabled: true })}>続ける</button>}
        {(improve.running || improve.limitResetsAt) && improve.sessionId && <a class="fb-link" href={href.session(improve.sessionId)}>進み具合を見る</a>}
      </div>
      {status && <p class="help fb-help">{status}</p>}
      {improve.error && <p class="help fb-help error-text">{improve.error}</p>}
    </div>
  );
};
