import { useEffect, useRef } from "preact/hooks";
import type { Rating, StoryWord, Topic } from "@mm/sdk";
import type { DragSelect } from "./useDragSelect.ts";
import { buildBlocks, shownText, type Block, type Span } from "./model.ts";
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
  /** 検索の結果などで目印を付ける単語。 */
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
}

/** 単語 1 つ。消した単語は打ち消し線、直した単語は下線で印を付ける。 */
const WordView = ({ word, index, selected, focused }: WordViewProps) => {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView?.({ block: "center" });
  }, [focused]);
  const title = word.fixed === null ? undefined : `Original: ${word.text}`;
  return (
    <span
      ref={ref}
      class="mm-story-word"
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

interface BlockViewProps {
  block: Block;
  props: TranscriptViewProps;
}

const BlockView = ({ block, props }: BlockViewProps) => {
  const { topic } = block;
  const { actions, select, focusWord } = props;
  return (
    <section class="mm-story-block" data-topic={topic !== null}>
      {topic && (
        <TopicHead
          topic={topic}
          picked={actions.pickedIds.has(topic.id)}
          onPick={() => actions.onPick(topic)}
          onRate={(rating) => actions.onRate(topic, rating)}
          onTag={(tag, remove) => actions.onTag(topic, tag, remove)}
          onRemove={() => actions.onRemove(topic)}
        />
      )}
      {block.paragraphs.map((words) => (
        <p key={words[0]?.word.id} class="mm-story-paragraph">
          {words.map(({ word, index }) => (
            <WordView
              key={word.id}
              word={word}
              index={index}
              selected={inSpan(select.span, index)}
              focused={word.id === focusWord}
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
  return (
    <div class="mm-story-text" {...props.select.handlers}>
      {blocks.map((block) => (
        <BlockView key={block.key} block={block} props={props} />
      ))}
    </div>
  );
};
