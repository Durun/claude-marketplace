import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { createIpcTransport, type Op, type OpResult, type RationalTime, type Selection, type Topic } from "@mm/sdk";

/** Story の素材 1 つ（アプリの `story_overview`）。時間は秒。 */
export interface StoryAsset {
  id: string;
  name: string;
  duration: number;
  has_transcript: boolean;
}

/** シーケンス 1 つの概要。 */
export interface StorySequence {
  id: string;
  cuts: number;
  duration: number;
}

/** 素材とシーケンスの一覧。 */
export interface StoryOverview {
  assets: StoryAsset[];
  sequences: StorySequence[];
  /** いま開いているコンポジションの sequence のレイヤーが置く sequence。Delete（`cutWords`）が切る先。 */
  open_sequence?: string | null;
}

/** 文字起こしの 1 単語。時刻は素材の時間軸。 */
export interface StoryWord {
  id: string;
  segment: number;
  start: RationalTime;
  end: RationalTime;
  /** 取り込んだままの文字。 */
  text: string;
  /** 直した文字。 */
  fixed: string | null;
  deleted: boolean;
}

/** 素材の文字起こしと、その素材の話題・選別（アプリの `story_transcript`）。 */
export interface StoryTranscript {
  asset: string;
  words: StoryWord[];
  topics: Topic[];
  selections: Selection[];
}

/** 単語の範囲の時刻（アプリの `story_range`）。話題の `in` と `out` にそのまま使える。 */
export interface WordRange {
  start: RationalTime;
  end: RationalTime;
}

/** 検索の 1 件（アプリの `story_search`）。 */
export interface StoryHit {
  asset: string;
  /** 語が始まる単語。 */
  word: string;
  start: RationalTime;
  end: RationalTime;
  context: string;
}

/** Story パネルがアプリに頼むこと。テストでは差し替える。 */
export interface StoryApi {
  overview: () => Promise<StoryOverview>;
  transcript: (asset: string) => Promise<StoryTranscript>;
  range: (asset: string, first: string, last: string) => Promise<WordRange>;
  search: (query: string, limit?: number) => Promise<StoryHit[]>;
  /** 素材の時刻（秒）に当たる再生位置（秒）。sequence に入っていなければ `null`。 */
  timelineTime: (asset: string, at: number) => Promise<number | null>;
  /** カットの始まりの再生位置（秒）。開いているコンポジションの sequence に出ていなければ `null`。 */
  cutTime: (cut: string) => Promise<number | null>;
  /** 操作を送る（`apply_op`）。 */
  apply: (op: Op) => Promise<OpResult>;
  /** 再生位置を動かす（`transport` の `seek`）。 */
  seek: (seconds: number) => Promise<void>;
}

/** Tauri の invoke と同じ形の関数。テストで差し替える。 */
export type StoryInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Tauri の IPC で Story のコマンドを呼ぶ実装。コマンドの名前と引数は ui/sdk/README.md。 */
export const createIpcStoryApi = (call: StoryInvoke = tauriInvoke): StoryApi => {
  const transport = createIpcTransport((command, args) => call(command, args));
  return {
    overview: async () => (await call("story_overview")) as StoryOverview,
    transcript: async (asset) => (await call("story_transcript", { asset })) as StoryTranscript,
    range: async (asset, first, last) =>
      (await call("story_range", { asset, first, last })) as WordRange,
    search: async (query, limit) => (await call("story_search", { query, limit })) as StoryHit[],
    timelineTime: async (asset, at) =>
      (await call("story_timeline_time", { asset, at })) as number | null,
    cutTime: async (cut) => (await call("story_cut_time", { cut })) as number | null,
    apply: (op) => transport.send(op),
    seek: async (seconds) => {
      await call("transport", { action: "seek", value: seconds });
    },
  };
};
