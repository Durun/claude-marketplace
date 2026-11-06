/** 作業のまとまりの中の 1 項目。command は押したときに呼ぶ操作の名前。 */
export interface TaskItem {
  id: string;
  label: string;
  command: string;
  /** 今は押せない項目。薄い色で出し、押しても操作は呼ばない。 */
  disabled?: boolean;
}

/** 作業のまとまり。見出しを押すと開け閉めする。 */
export interface TaskGroup {
  id: string;
  title: string;
  items: TaskItem[];
}

/** 取り込む → 切る → 文字と音を足す → 書き出す、の順（docs/design/08-ui.md）。操作の名前は仮。 */
export const DEFAULT_GROUPS: TaskGroup[] = [
  {
    id: "import",
    title: "Import",
    items: [
      { id: "media", label: "Import video and audio", command: "media.import" },
      { id: "pictures", label: "Import pictures", command: "media.import_pictures" },
      { id: "transcribe", label: "Transcribe speech", command: "media.transcribe" },
    ],
  },
  {
    id: "edit",
    title: "Edit",
    items: [
      { id: "split", label: "Split at playhead", command: "clip.split" },
      { id: "remove-silence", label: "Remove silent parts", command: "clip.remove_silence" },
      { id: "transitions", label: "Add a transition", command: "clip.add_transition" },
    ],
  },
  {
    id: "text-audio",
    title: "Add text and audio",
    items: [
      { id: "title", label: "Add a title", command: "text.add_title" },
      { id: "captions", label: "Add captions", command: "text.add_captions" },
      { id: "music", label: "Add music", command: "audio.add_music" },
    ],
  },
  {
    id: "export",
    title: "Export",
    items: [
      { id: "export-file", label: "Export to a file", command: "export.start" },
      { id: "export-targets", label: "Choose a render target", command: "export.choose_target" },
    ],
  },
];
