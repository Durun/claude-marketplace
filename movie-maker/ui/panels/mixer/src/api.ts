import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { CompMixer, LayerAudio, MediaInvoke, MediaListen, Op } from "@mm/sdk";

/** Mixer のトラック 1 本。音を持てるレイヤー（撮影素材、入れ子、sequence）だけ。上のレイヤーが先頭。 */
export interface MixerTrack {
  layer: string;
  /** ノードの種類（`footage`、`composition`、`sequence`）。 */
  kind: string;
  /** 表示の名前。素材のファイル名か、入れ子や sequence の ID。 */
  label: string;
  /** レイヤーの音の設定。`null` は既定（役割 `voice`、0 dB、中央）。 */
  audio: LayerAudio | null;
  /** 音量かパンにキーフレームがある。フェーダーで固定の値に置くとキーフレームを消すので、Mixer は触らせない。 */
  animated: boolean;
}

/** Mixer を開いたときの中身（アプリの `mixer_state`）。いま開いているコンポジションのもの。 */
export interface MixerState {
  composition: string;
  /** コンポジションのミキサーの設定。`null` は既定（全部 0 dB、ダッキングなし）。 */
  mixer: CompMixer | null;
  tracks: MixerTrack[];
  /** バスの選択肢。使われている役割と、設定にあるバスと、`voice`・`bgm`・`sfx`。 */
  roles: string[];
}

/** トラック 1 本のメーター。フェーダーとパンとミュートの後のピーク（dBFS）。無音は `null`。 */
export interface TrackMeter {
  layer: string;
  peak_db: number | null;
}

/** マスターのメーター。ラウドネスは ITU-R BS.1770。測れない間（無音、短すぎる）は `null`。 */
export interface MasterMeter {
  /** チャンネルごとのサンプルのピーク（dBFS。L、R の順）。 */
  peak_db: (number | null)[];
  /** トゥルーピーク（dBTP）。統合ラウドネスと同じく、最後に再生の位置を置き直して（シークして）からの最大。 */
  true_peak_db: number | null;
  /** 短期ラウドネス（直近 3 秒、LUFS）。 */
  short_term_lufs: number | null;
  /** 統合ラウドネス（最後にシークしてから、LUFS）。 */
  integrated_lufs: number | null;
}

/** 再生中のメーターの値（イベント `mm:mixer-meters`）。60 Hz の再生位置の通知と同じ間隔で、鳴っている位置の値を送る。 */
export interface MixerMeters {
  /** 値の再生位置（秒）。 */
  t: number;
  playing: boolean;
  tracks: TrackMeter[];
  master: MasterMeter;
}

/** メーターの値を送る Tauri のイベント名。アプリの `mixer_ipc.rs` と同じ。 */
export const MIXER_METERS_EVENT = "mm:mixer-meters";

/** Mixer パネルがアプリに頼むこと。テストでは差し替える。失敗は reject（理由の文字を持つ Error）。 */
export interface MixerApi {
  state: () => Promise<MixerState>;
  /** 操作を 1 つのトランザクションで送る（`apply_ops`）。取り消し 1 回で戻る。 */
  applyOps: (ops: Op[]) => Promise<void>;
  onMeters: (handler: (meters: MixerMeters) => void) => Promise<() => void>;
}

/** Tauri の失敗（文字列）を、理由を持つ Error にする。 */
const asError = (e: unknown): Error => (e instanceof Error ? e : new Error(String(e)));

/** Tauri の IPC で Mixer のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcMixerApi = (call: MediaInvoke = tauriInvoke, listen: MediaListen = tauriListen): MixerApi => {
  const run = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
    try {
      return (await call(command, args)) as T;
    } catch (e) {
      throw asError(e);
    }
  };
  return {
    state: () => run("mixer_state"),
    applyOps: async (ops) => {
      await run("apply_ops", { ops });
    },
    onMeters: (handler) => listen(MIXER_METERS_EVENT, (e) => handler(e.payload as MixerMeters)),
  };
};
