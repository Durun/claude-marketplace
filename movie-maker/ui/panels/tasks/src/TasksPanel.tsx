import { useState } from "preact/hooks";
import { DEFAULT_GROUPS, type TaskGroup } from "./groups.ts";

/** 項目を押したときの口。後で ui/sdk の invoke につなぐのはここ 1 か所。 */
export type OnInvoke = (command: string) => void;

export interface TasksPanelProps {
  groups?: TaskGroup[];
  onInvoke: OnInvoke;
  /** 最初から閉じておくまとまりの id。 */
  initiallyClosed?: readonly string[];
}

interface GroupViewProps {
  group: TaskGroup;
  open: boolean;
  onToggle: () => void;
  onInvoke: OnInvoke;
}

/** 見出しと、開いているときの項目の並び。 */
const GroupView = ({ group, open, onToggle, onInvoke }: GroupViewProps) => {
  const bodyId = `mm-tasks-${group.id}`;
  return (
    <section class="mm-tasks-group" data-open={open}>
      <h2 class="mm-tasks-title">
        <button
          type="button"
          class="mm-tasks-toggle"
          aria-expanded={open}
          aria-controls={bodyId}
          onClick={onToggle}
        >
          <span class="mm-tasks-chevron" aria-hidden="true" />
          {group.title}
        </button>
      </h2>
      {open && (
        <ul class="mm-tasks-body" id={bodyId}>
          {group.items.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                class="mm-tasks-link"
                disabled={item.disabled}
                onClick={() => onInvoke(item.command)}
              >
                <span class="mm-tasks-bullet" aria-hidden="true" />
                {item.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};

/** XP のムービーメーカー風の作業ウィンドウ。まとまりごとに開け閉めできる。 */
export const TasksPanel = ({
  groups = DEFAULT_GROUPS,
  onInvoke,
  initiallyClosed = [],
}: TasksPanelProps) => {
  const [closed, setClosed] = useState<ReadonlySet<string>>(new Set(initiallyClosed));

  const toggle = (id: string) => {
    const next = new Set(closed);
    if (!next.delete(id)) next.add(id);
    setClosed(next);
  };

  return (
    <nav class="mm-tasks" aria-label="Movie Tasks">
      {groups.map((group) => (
        <GroupView
          key={group.id}
          group={group}
          open={!closed.has(group.id)}
          onToggle={() => toggle(group.id)}
          onInvoke={onInvoke}
        />
      ))}
    </nav>
  );
};
