import type { ImportEvent } from "./api.ts";

/** 取り込みに失敗した素材 1 つ。 */
export interface ImportFailure {
  path: string;
  error: string;
}

/** 取り込みの進み具合。パネルが出す帯と行の元になる。 */
export interface ImportProgress {
  running: boolean;
  /** 今の素材の番号（0 始まり）と全体の数。 */
  index: number;
  total: number;
  /** 取り込み中の素材のパス。 */
  current: string | null;
  failures: ImportFailure[];
  /** 最後に終わった取り込みの結果。 */
  summary: { added: number; skipped: number; failed: number } | null;
}

export const IDLE_PROGRESS: ImportProgress = {
  running: false,
  index: 0,
  total: 0,
  current: null,
  failures: [],
  summary: null,
};

/** イベントを 1 つ受けて、進み具合を進める。最初の素材が始まったら、前の取り込みの失敗と結果を捨てる。 */
export const reduceProgress = (state: ImportProgress, event: ImportEvent): ImportProgress => {
  switch (event.type) {
    case "started":
      return {
        ...(event.index === 0 ? IDLE_PROGRESS : state),
        running: true,
        index: event.index,
        total: event.total,
        current: event.path,
      };
    case "failed":
      return { ...state, failures: [...state.failures, { path: event.path, error: event.error }] };
    case "finished":
      return {
        ...state,
        running: false,
        current: null,
        summary: { added: event.added, skipped: event.skipped, failed: event.failed },
      };
    default:
      return state;
  }
};
