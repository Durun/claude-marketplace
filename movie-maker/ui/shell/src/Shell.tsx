import { useEffect, useState } from "preact/hooks";
import { DeliverPanel } from "@mm/deliver";
import { MediaPanel } from "@mm/media";
import { MixerPanel } from "@mm/mixer";
import { ReviewPanel } from "@mm/review";
import {
  invoke,
  onDocChanged,
  type Asset,
  type DeliverApi,
  type Layer,
  type MediaApi,
  type MediaListen,
  type MixerApi,
  type Op,
  type ReviewApi,
  type StoryApi,
} from "@mm/sdk";
import { StoryPanel } from "@mm/story";
import { KeyframePanels } from "./keyframePanels.tsx";
import { TasksPanel } from "@mm/tasks";

/** 選択の変わったことを知らせるイベント。アプリの再生の操作列（`apps/movie-maker/web/transport.js`）が、Timeline の選択の Tauri イベントをこの名前の DOM のイベントにつなぐ。 */
export const LAYER_SELECTED_EVENT = "mm:layer-selected";

export interface LayerSelection {
  layer: Layer | null;
  assets?: Asset[];
}

export interface ShellProps {
  /** 操作を送る。本番は sdk の client（IPC）をつなぐ。拒否されたら理由の文字を返し、Shell が画面に出す。 */
  sendOp: (op: Op) => Promise<string | null> | undefined;
  /** Media パネルがアプリを呼ぶ API。無ければ Media は出さない。 */
  media?: MediaApi;
  /** Story パネルがアプリを呼ぶ API。無ければ Story は出さない。 */
  story?: StoryApi;
  /** Deliver パネルがアプリを呼ぶ API。無ければ Deliver は出さない。 */
  deliver?: DeliverApi;
  /** Mixer パネルがアプリを呼ぶ API。無ければ Mixer は出さない。 */
  mixer?: MixerApi;
  /** Review パネルがアプリを呼ぶ API。無ければ Review は出さない。 */
  review?: ReviewApi;
  /** Tauri のイベントの購読。無ければ本番の listen。テストで差し替える。 */
  listen?: MediaListen;
}

/** 左の列で見せているパネル。XP のムービーメーカーの Tasks と Collections の切り替えに倣う。 */
type LeftTab = "tasks" | "media" | "story" | "mixer" | "deliver";

/** 右の列で見せているパネル。左の列のタブに倣う。 */
type RightTab = "inspector" | "review";

/** Tasks の項目のうち、Media を開いて取り込みを始めるもの。 */
const IMPORT_COMMAND = "media.import";
/** Tasks の項目のうち、Deliver を開くもの。 */
const DELIVER_COMMANDS = ["export.start", "export.choose_target"];

const TABS: { id: LeftTab; label: string }[] = [
  { id: "tasks", label: "Tasks" },
  { id: "media", label: "Media" },
  { id: "story", label: "Story" },
  { id: "mixer", label: "Mixer" },
  { id: "deliver", label: "Deliver" },
];

const RIGHT_TABS: { id: RightTab; label: string }[] = [
  { id: "inspector", label: "Inspector" },
  { id: "review", label: "Review" },
];

const RightTabs = ({ tab, onSelect }: { tab: RightTab; onSelect: (tab: RightTab) => void }) => (
  <div class="mm-shell-tabs mm-shell-right-tabs" role="tablist" aria-label="Right panel">
    {RIGHT_TABS.map(({ id, label }) => (
      <button
        key={id}
        type="button"
        role="tab"
        class="mm-shell-tab"
        aria-selected={tab === id}
        onClick={() => onSelect(id)}
      >
        {label}
      </button>
    ))}
  </div>
);

const LeftTabs = ({ tabs, tab, onSelect }: { tabs: LeftTab[]; tab: LeftTab; onSelect: (tab: LeftTab) => void }) => (
  <div class="mm-shell-tabs" role="tablist" aria-label="Left panel">
    {TABS.filter(({ id }) => tabs.includes(id)).map(({ id, label }) => (
      <button
        key={id}
        type="button"
        role="tab"
        class="mm-shell-tab"
        aria-selected={tab === id}
        onClick={() => onSelect(id)}
      >
        {label}
      </button>
    ))}
  </div>
);

/** Timeline の選択のイベントを受けて、いま選ばれているレイヤーを持つ。 */
const useLayerSelection = (): LayerSelection => {
  const [selection, setSelection] = useState<LayerSelection>({ layer: null });
  useEffect(() => {
    const onSelected = (e: Event) => setSelection((e as CustomEvent<LayerSelection>).detail);
    window.addEventListener(LAYER_SELECTED_EVENT, onSelected);
    return () => window.removeEventListener(LAYER_SELECTED_EVENT, onSelected);
  }, []);
  return selection;
};

/** 操作を送り、拒否された理由を持つ。次に成功した操作で消える。 */
const useSendOp = (sendOp: ShellProps["sendOp"]) => {
  const [opError, setOpError] = useState<string | null>(null);
  const send = async (op: Op) => setOpError((await sendOp(op)) ?? null);
  return { send, opError };
};

/** `mm:doc-changed` が届いた回数。パネルの読み直しの合図にする。 */
const useDocChanges = (listen: MediaListen | undefined): number => {
  const [count, setCount] = useState(0);
  useEffect(() => {
    // 購読が確立する前に外れた場合も、確立した後で外す。
    let off: (() => void) | null = null;
    let stopped = false;
    // Tauri の外（デモや試験）では購読できないので、通知なしで動かす。
    onDocChanged(() => setCount((n) => n + 1), listen).then(
      (fn) => {
        if (stopped) fn();
        else off = fn;
      },
      () => undefined,
    );
    return () => {
      stopped = true;
      off?.();
    };
  }, [listen]);
  return count;
};

/** 左の列のタブの選択と、Tasks の項目が開く先。 */
const useLeftTabs = (apis: Pick<ShellProps, "media" | "story" | "mixer" | "deliver">) => {
  const { media, story, mixer, deliver } = apis;
  const [tab, setTab] = useState<LeftTab>("tasks");
  const [importRequest, setImportRequest] = useState(0);
  const [storyVisits, setStoryVisits] = useState(0);
  const tabs: LeftTab[] = [
    "tasks",
    ...(media ? ["media" as const] : []),
    ...(story ? ["story" as const] : []),
    ...(mixer ? ["mixer" as const] : []),
    ...(deliver ? ["deliver" as const] : []),
  ];
  const selectTab = (next: LeftTab) => {
    if (next === "story") setStoryVisits((n) => n + 1);
    setTab(next);
  };
  const onTaskInvoke = (command: string) => {
    if (media && command === IMPORT_COMMAND) {
      setTab("media");
      setImportRequest((n) => n + 1);
    } else if (deliver && DELIVER_COMMANDS.includes(command)) {
      setTab("deliver");
    } else {
      void invoke(command);
    }
  };
  return { tab, tabs, selectTab, onTaskInvoke, importRequest, storyVisits };
};

interface RightColumnProps {
  review: ReviewApi | undefined;
  opError: string | null;
  onOp: (op: Op) => Promise<void>;
  refreshKey: number;
}

/** 右の列。Inspector（その下に Graph）と、API があれば Review をタブで切り替える。 */
const RightColumn = ({ review, opError, onOp, refreshKey }: RightColumnProps) => {
  const [rightTab, setRightTab] = useState<RightTab>("inspector");
  const selection = useLayerSelection();
  return (
    <aside class="mm-shell-right">
      {opError && (
        <p class="mm-shell-error" role="alert">
          {opError}
        </p>
      )}
      {review && <RightTabs tab={rightTab} onSelect={setRightTab} />}
      <div class="mm-shell-right-body" role="tabpanel" hidden={rightTab !== "inspector"}>
        <KeyframePanels layer={selection.layer} assets={selection.assets} onOp={onOp} />
      </div>
      {review && (
        <div class="mm-shell-right-body" role="tabpanel" hidden={rightTab !== "review"}>
          <ReviewPanel api={review} refreshKey={refreshKey} active={rightTab === "review"} />
        </div>
      )}
    </aside>
  );
};

/** 左に Tasks、Media、Story、Mixer、Deliver、右に Inspector（その下に Graph）と Review（それぞれタブで切り替え）。API を渡したパネルだけタブを出す。 */
export const Shell = ({ sendOp, media, story, mixer, deliver, review, listen }: ShellProps) => {
  const { tab, tabs, selectTab, onTaskInvoke, importRequest, storyVisits } = useLeftTabs({ media, story, mixer, deliver });
  const docChanges = useDocChanges(listen);
  const { send, opError } = useSendOp(sendOp);
  return (
    <>
      <aside class="mm-shell-left">
        {tabs.length > 1 && <LeftTabs tabs={tabs} tab={tab} onSelect={selectTab} />}
        <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "tasks"}>
          <TasksPanel onInvoke={onTaskInvoke} />
        </div>
        {media && (
          <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "media"}>
            <MediaPanel api={media} importRequest={importRequest} refreshKey={docChanges} />
          </div>
        )}
        {story && (
          <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "story"}>
            <StoryPanel api={story} refreshKey={storyVisits + docChanges} />
          </div>
        )}
        {mixer && (
          <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "mixer"}>
            <MixerPanel api={mixer} refreshKey={docChanges} />
          </div>
        )}
        {deliver && (
          <div class="mm-shell-left-body" role="tabpanel" hidden={tab !== "deliver"}>
            <DeliverPanel api={deliver} refreshKey={docChanges} />
          </div>
        )}
      </aside>
      <main class="mm-shell-center" />
      <RightColumn review={review} opError={opError} onOp={send} refreshKey={docChanges} />
    </>
  );
};
