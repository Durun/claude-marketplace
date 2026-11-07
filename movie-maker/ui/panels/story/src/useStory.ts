import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import { type Op, type RationalTime, type Topic, announceAsset } from "@mm/sdk";
import type { StoryApi, StoryHit, StoryOverview, StoryTranscript } from "./api.ts";
import { toSeconds } from "./model.ts";

const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export interface Loaded {
  overview: StoryOverview | null;
  assetId: string | null;
  transcript: StoryTranscript | null;
  error: string | null;
  setError: (message: string | null) => void;
  selectAsset: (id: string) => void;
  reload: () => Promise<void>;
  /** 操作を順に送る。1 つでも断られたら止めて理由を `error` に出し、`false` を返す。 */
  apply: (ops: Op[]) => Promise<boolean>;
}

/** 素材の一覧と、選んだ素材の文字起こしを読み、操作を送ったあとに読み直す。 */
export const useLoaded = (api: StoryApi, refreshKey: number): Loaded => {
  const [overview, setOverview] = useState<StoryOverview | null>(null);
  const [assetId, setAssetId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<StoryTranscript | null>(null);
  const [error, setError] = useState<string | null>(null);
  const asset = useRef<string | null>(null);
  asset.current = assetId;

  const reload = useCallback(async () => {
    try {
      const next = await api.overview();
      setOverview(next);
      const current = next.assets.find((a) => a.id === asset.current && a.has_transcript);
      const id = current?.id ?? next.assets.find((a) => a.has_transcript)?.id ?? null;
      setAssetId(id);
      setTranscript(id ? await api.transcript(id) : null);
    } catch (e) {
      setError(errorText(e));
    }
  }, [api]);
  useEffect(() => void reload(), [reload, refreshKey]);

  const selectAsset = (id: string) => {
    setAssetId(id);
    announceAsset(id);
    api.transcript(id).then(setTranscript, (e: unknown) => setError(errorText(e)));
  };
  const apply = async (ops: Op[]): Promise<boolean> => {
    setError(null);
    let ok = true;
    for (const op of ops) {
      const result = await api.apply(op);
      if (!result.ok) {
        setError(result.error);
        ok = false;
        break;
      }
    }
    await reload();
    return ok;
  };
  return { overview, assetId, transcript, error, setError, selectAsset, reload, apply };
};

export interface Search {
  hits: StoryHit[] | null;
  run: (query: string) => Promise<void>;
  clear: () => void;
}

/** 全文検索。結果は閉じるまで残す。 */
export const useSearch = (api: StoryApi, onError: (message: string) => void): Search => {
  const [hits, setHits] = useState<StoryHit[] | null>(null);
  const run = async (query: string) => {
    if (query.trim() === "") return setHits(null);
    try {
      setHits(await api.search(query));
    } catch (e) {
      onError(errorText(e));
    }
  };
  return { hits, run, clear: () => setHits(null) };
};

export interface Tray {
  picked: Topic[];
  toggle: (topic: Topic) => void;
  move: (index: number, by: number) => void;
  clear: () => void;
}

/** 「並べる」の対象にした話題を、並べる順に持つ。 */
export const useTray = (): Tray => {
  const [picked, setPicked] = useState<Topic[]>([]);
  const toggle = (topic: Topic) =>
    setPicked((list) =>
      list.some((t) => t.id === topic.id) ? list.filter((t) => t.id !== topic.id) : [...list, topic],
    );
  const move = (index: number, by: number) =>
    setPicked((list) => {
      const to = index + by;
      if (to < 0 || to >= list.length) return list;
      const next = [...list];
      next.splice(to, 0, ...next.splice(index, 1));
      return next;
    });
  return { picked, toggle, move, clear: () => setPicked([]) };
};

/** 足したカットの始まりへ再生位置を移す。カットが画面に出ていなければ `false`。 */
export const seekToCut = async (api: StoryApi, cut: string): Promise<boolean> => {
  const t = await api.cutTime(cut);
  if (t === null) return false;
  await api.seek(t);
  return true;
};

/** 素材の時刻へ再生位置を移す。シーケンスにもレイヤーにも入っていなければ `false`。 */
export const seekToSource = async (
  api: StoryApi,
  asset: string,
  at: RationalTime,
): Promise<boolean> => {
  const t = await api.timelineTime(asset, toSeconds(at));
  if (t === null) return false;
  await api.seek(t);
  return true;
};
