import type { Rating, StoryApi, StoryHit, StoryWord, Topic } from "@mm/sdk";
import { addTopicOp, deleteOp, fixOp, insertTopicsOp, rateOp, tagOp } from "./ops.ts";
import { seekToCut, seekToSource, type Loaded, type Tray } from "./useStory.ts";

export interface Actions {
  makeTopic: (title: string) => Promise<void>;
  deleteSelected: () => Promise<void>;
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
  clearSelection: () => void;
  setFocusWord: (id: string | null) => void;
  setNotice: (message: string | null) => void;
}

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
    makeTopic: async (title) => {
      const first = selected[0];
      const last = selected.at(-1);
      if (!asset || !first || !last) return;
      const range = await api.range(asset, first.id, last.id);
      if (await send(addTopicOp(asset, title, range))) cx.clearSelection();
    },
    deleteSelected: async () => {
      if (asset && (await send(deleteOp(asset, selected)))) cx.clearSelection();
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
