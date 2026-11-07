import type { Op, Rating, Topic } from "@mm/sdk";
import type { StoryApi, StoryHit, StoryWord, WordRange } from "./api.ts";
import { addTopicOp, cutOp, fixOp, hideOp, insertTopicsOp, rateOp, tagOp } from "./ops.ts";
import { seekToCut, seekToSource, type Loaded, type Tray } from "./useStory.ts";

export interface Actions {
  makeTopic: (title: string) => Promise<void>;
  /** 選んだ単語を消し、その音を再生から外す。 */
  cutSelected: () => Promise<void>;
  /** 選んだ単語を表示だけ消す（全部消してあれば戻す）。 */
  hideSelected: () => Promise<void>;
  fixSelected: (text: string) => Promise<void>;
  rate: (topic: Topic, rating: Rating) => Promise<void>;
  tag: (topic: Topic, tag: string, remove: boolean) => Promise<void>;
  removeTopic: (topic: Topic) => Promise<void>;
  arrange: (sequence: string) => Promise<void>;
  /** 単語の始まりの時刻に再生位置を移す。 */
  jump: (word: StoryWord) => Promise<void>;
  openHit: (hit: StoryHit) => Promise<void>;
}

export interface ActionContext {
  api: StoryApi;
  loaded: Loaded;
  tray: Tray;
  /** 選んでいる単語。 */
  selected: readonly StoryWord[];
  /** Delete（`cutWords`）が切る sequence。無ければ切れない。 */
  cutSequence: string | null;
  clearSelection: () => void;
  setFocusWord: (id: string | null) => void;
  setNotice: (message: string | null) => void;
}

/** 選んだ単語の範囲の時刻で組んだ操作を送り、送れたら選択を外す。選んでいなければ何もしない。 */
const sendForRange = async (cx: ActionContext, build: (asset: string, range: WordRange) => Op | null) => {
  const asset = cx.loaded.assetId;
  const first = cx.selected[0];
  const last = cx.selected.at(-1);
  if (!asset || !first || !last) return;
  const op = build(asset, await cx.api.range(asset, first.id, last.id));
  if (op && (await cx.loaded.apply([op]))) cx.clearSelection();
};

/** パネルの操作を、操作の組み立てと送信と再生位置の移動につなぐ。 */
export const createActions = (cx: ActionContext): Actions => {
  const { api, loaded, tray, selected } = cx;
  const asset = loaded.assetId;
  const send = async (op: Parameters<Loaded["apply"]>[0][number] | null): Promise<boolean> =>
    op ? loaded.apply([op]) : false;
  const jumpTo = async (target: string, word: { start: StoryWord["start"] }) => {
    const moved = await seekToSource(api, target, word.start);
    cx.setNotice(moved ? null : "This part is not on the timeline yet.");
  };
  return {
    makeTopic: (title) => sendForRange(cx, (a, range) => addTopicOp(a, title, range)),
    cutSelected: async () => {
      const seq = cx.cutSequence;
      if (seq) await sendForRange(cx, (a, range) => cutOp(a, seq, selected, range));
      else cx.setNotice("No sequence to cut from. Hide only hides the words.");
    },
    hideSelected: async () => {
      if (asset && (await send(hideOp(asset, selected)))) cx.clearSelection();
    },
    fixSelected: async (text) => {
      const word = selected[0];
      if (asset && word && (await send(fixOp(asset, word, text)))) cx.clearSelection();
    },
    rate: async (topic, rating) => void (await send(rateOp(topic, rating))),
    tag: async (topic, tag, remove) => void (await send(tagOp(topic, tag, remove))),
    removeTopic: async (topic) => {
      if (await send({ op: "removeTopic", topic: topic.id })) {
        if (tray.picked.some((t) => t.id === topic.id)) tray.toggle(topic);
      }
    },
    arrange: async (sequence) => {
      if (tray.picked.length === 0) return;
      const op = insertTopicsOp(sequence, tray.picked);
      if (!(await send(op))) return;
      tray.clear();
      const moved = await seekToCut(api, op.cuts[0] ?? "");
      cx.setNotice(moved ? null : "This part is not on the timeline yet.");
    },
    jump: async (word) => {
      if (asset) await jumpTo(asset, word);
    },
    openHit: async (hit) => {
      if (hit.asset !== asset) loaded.selectAsset(hit.asset);
      cx.setFocusWord(hit.word);
      await jumpTo(hit.asset, hit);
    },
  };
};
