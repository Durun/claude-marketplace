import { useEffect, useRef } from "preact/hooks";
import type { Rating, Topic } from "@mm/sdk";
import type { StoryWord } from "./api.ts";
import type { DragSelect } from "./useDragSelect.ts";
import { buildBlocks, orphanTopics, shownText, type Block, type Span } from "./model.ts";
import { TopicHead } from "./TopicHead.tsx";

export interface TopicActions {
  pickedIds: ReadonlySet<string>;
  onPick: (topic: Topic) => void;
  onRate: (topic: Topic, rating: Rating) => void;
  onTag: (topic: Topic, tag: string, remove: boolean) => void;
  onRemove: (topic: Topic) => void;
}

export interface TranscriptViewProps {
  words: readonly StoryWord[];
  topics: readonly Topic[];
  select: DragSelect;
  /** 検索の結果などで強調する単語。 */
  focusWord: string | null;
  actions: TopicActions;
}

const inSpan = (span: Span | null, index: number): boolean =>
  span !== null && index >= span.from && index <= span.to;

interface WordViewProps {
  word: StoryWord;
  index: number;
  selected: boolean;
  focused: boolean;
  /** Tab で止まる単語か（roving tabindex）。 */
  tabStop: boolean;
}

/** 単語 1 つ。消した単語は打ち消し線、直した単語は下線で示す。 */
const WordView = ({ word, index, selected, focused, tabStop }: WordViewProps) => {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView?.({ block: "center" });
  }, [focused]);
  const title = word.fixed === null ? undefined : `Original: ${word.text}`;
  return (
    <span
      ref={ref}
      class="mm-story-word"
      role="button"
      tabIndex={tabStop ? 0 : -1}
      aria-pressed={selected}
      data-i={index}
      data-deleted={word.deleted}
      data-fixed={word.fixed !== null}
      data-selected={selected}
      data-focus={focused}
      title={title}
    >
      {shownText(word)}
    </span>
  );
};

const TopicHeadFor = ({ topic, actions }: { topic: Topic; actions: TopicActions }) => (
  <TopicHead
    topic={topic}
    picked={actions.pickedIds.has(topic.id)}
    onPick={() => actions.onPick(topic)}
    onRate={(rating) => actions.onRate(topic, rating)}
    onTag={(tag, remove) => actions.onTag(topic, tag, remove)}
    onRemove={() => actions.onRemove(topic)}
  />
);

interface BlockViewProps {
  block: Block;
  props: TranscriptViewProps;
}

const BlockView = ({ block, props }: BlockViewProps) => {
  const { topic } = block;
  const { actions, select, focusWord } = props;
  return (
    <section class="mm-story-block" data-topic={topic !== null}>
      {topic && block.head && <TopicHeadFor topic={topic} actions={actions} />}
      {block.paragraphs.map((words) => (
        <p key={words[0]?.word.id} class="mm-story-paragraph">
          {words.map(({ word, index }) => (
            <WordView
              key={word.id}
              word={word}
              index={index}
              selected={inSpan(select.span, index)}
              focused={word.id === focusWord}
              tabStop={select.cursor === index}
            />
          ))}
        </p>
      ))}
    </section>
  );
};

/** 文字起こしを読み物として出す。話題ごとに区切り、区間（文）ごとに改行する。 */
export const TranscriptView = (props: TranscriptViewProps) => {
  const blocks = buildBlocks(props.words, props.topics);
  const orphans = orphanTopics(blocks, props.topics);
  return (
    <div class="mm-story-text" {...props.select.handlers}>
      {blocks.map((block) => (
        <BlockView key={block.key} block={block} props={props} />
      ))}
      {orphans.length > 0 && (
        <section class="mm-story-block mm-story-orphans" aria-label="Topics without words">
          <p class="mm-story-meta">Topics without words</p>
          {orphans.map((topic) => (
            <TopicHeadFor key={topic.id} topic={topic} actions={props.actions} />
          ))}
        </section>
      )}
    </div>
  );
};
