// 通知領域に常駐する、イルカのマイク。押すたびに ON と OFF を切り替え、右クリックで呼びかけの言葉を変える。
import { useRef } from 'preact/hooks';
import { useLightMode } from './lightMode.ts';
import { setMic, setWakeWords, useMic } from './voice.ts';

const MIC_ID = 'mic-flyout';

/** 軽量モードではイルカが居ないので、出さない。 */
export const MicTray = () => {
  const light = useLightMode();
  const { on, preparing, words } = useMic();
  const flyout = useRef<HTMLDivElement>(null);
  if (light) return null;
  const summary = preparing
    ? 'イルカのマイク: 日本語の認識データをダウンロードしています'
    : on
      ? `イルカのマイク: ON。「${words.join('」「')}」と呼びかけると聞きます`
      : 'イルカのマイク: OFF';
  return (
    <>
      <button
        class={`mic-tray${on ? ' is-on' : ''}${preparing ? ' is-preparing' : ''}`}
        aria-pressed={on}
        aria-label={summary}
        title={`${summary}。右クリックで呼びかけを変えます`}
        onClick={() => setMic(!on)}
        onContextMenu={(e) => {
          e.preventDefault();
          flyout.current?.showPopover();
        }}
      >
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <rect class="mic-head" x="7" y="2.5" width="6" height="10" rx="3" />
          <path d="M4.5 9.5a5.5 5.5 0 0 0 11 0M10 15v2.5" />
          {!on && <path class="mic-slash" d="M3.5 3.5l13 13" />}
        </svg>
      </button>
      <div id={MIC_ID} class="mic-flyout" popover ref={flyout}>
        <b class="mic-flyout-head">イルカのマイク</b>
        <label class="mic-words">
          呼びかけ
          <input class="input input-sm" value={words.join('、')} onChange={(e) => setWakeWords(e.currentTarget.value)} />
        </label>
        <p class="muted">「、」で区切って複数書けます。ON の間、呼びかけで始まる発言をイルカへの頼みとして聞きます。音声は Chrome が Mac の中で書き起こします。</p>
      </div>
    </>
  );
};
