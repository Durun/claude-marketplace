import type { Register, Timer } from 'claude-code'
import { type Cell, faceRows, runs } from './face.ts'
import { FACE_AREA_HEIGHT, FACE_AREA_WIDTH, FRAME_MS, WALL_WIDTH, applyDebris, faceOffset, facePixelRow, lastFrame, mouthOpen, speechLength, speechParts, spokenRuns, wallRow } from './entrance.ts'
import { CUTIN_FRAMES, cutinRows } from './cutin.ts'

const PANEL_COLUMNS = WALL_WIDTH + FACE_AREA_WIDTH

/** ツッコミが挙げた語の数。語は **「面」** の形で、鉤括弧ごと強調される。 */
const keywordCount = (line: string) =>
  speechParts(line).filter((part) => part.emphasis && part.text.includes('「')).length

/** 登場が始まるコマ。語が 2 つ以上か、指摘が 2 点以上のときだけ、前にカットインのぶんを取る。 */
const firstEntranceFrame = (intense: boolean) => (intense ? CUTIN_FRAMES : 0)

// 短い応答は曖昧になりようがないので、haiku を呼ばずに見送る。
const MIN_CHARS = 40

const SYSTEM = `あなたは日本の頑固オヤジだ。AI アシスタントの回答を読み、日本語として曖昧な言葉遣いにだけツッコミを入れる。

見る点:
- 主語が無い文
- 「これ」「それ」「該当箇所」など指す先が分からない語
- 具体性を欠いた、翻訳調の、日常では使わない言い回しに転用された名詞と動詞
- 造語、意味の取れないカタカナ語、中身の無い比喩
- 「〜ない」とだけ言って何が有るのかを書いていない文
- 断定を避けた濁し方
- 助詞の抜けと誤り。「ここ確認して」「設定をを変更」のような文
- 「〜される」「〜が行われる」で、誰がやるのかが消えた受け身
- 「基本的に」「原則として」「必要に応じて」など、例外を書かずに断定を避ける前置き
- カッコ付きの注釈を重ねて、1 文に複数の意味を詰め込んだ文
- 「無くはない」「できないことはない」のような二重否定

転用された語の例。これは例であって、これだけを探すのではない:
- 名詞: 面、印、正典、塗り、穴
- 動詞: 効く、落ちる、詰める

読んで一度で意味が取れず、何を指しているのか聞き返したくなる語を選ぶ。
その分野で普通に使う語や、具体物を指している語は見逃す。

1 語だけ気になったときのツッコミの例:
**「主語」**がないぜ主語がぁ〜〜！！
**「面」**って一体何のことダァ〜〜！？
ない、ない、ない、じゃあ一体何が**「ある」**んだってぇ〜の！！
**「ここ確認して」**って何だぁ〜！助詞はどこへ行ったぁ〜〜！？
**「行われる」**って、一体誰がやるってんだぁ〜〜い！！
**「基本的に」**の一言で逃げようったって、そうはいかねぇ〜〜ぞ！！
カッコにカッコを重ねて、**一文に何個詰め込みゃ気が済むんだぁ〜〜！！**
**「無くはない」**ってぇのは、有るのか無いのか**どっちなんだぁ〜〜い！！**

複数の語が気になったときの例。見つけた語を全部並べて叫ぶ:
**「面」**とか**「印」**とか、もうわけわからん**！！！！！**
**「効く」**に**「落ちる」**に**「塗り」**、一体いくつ持ち出せば気が済むん**だぁ〜〜！！**

出力は 1 行だけ。問題にした語を必ず含めて、上の例の口調で叫ぶ。

見る点のうち 2 つ以上に引っかかっているときは、行の先頭に !! と書いてから叫ぶ。
主語も無く語も転用されている、のように種類の違う指摘が重なったときに付ける。

書き方:
- 問題にした語は **「面」** と書く。鉤括弧は ** の内側に入れる。「**面**」のように外へ出さない
- 叫んでいる末尾も **ァ〜〜！？** のように囲む
- 気になった語が 2 つ以上あるときは、その全てを 1 行の中に挙げる。1 つだけ選ばない
- 囲むのは語それぞれで 1 か所ずつと、末尾の叫びで 1 か所
曖昧な語が無ければ OK の 2 文字だけを出力する。`

export const register: Register = (on) => {
  let tsukkomi: string | null = null
  let intense = false
  let frame = 0
  let ticker: Timer | null = null

  const stopTicker = () => {
    ticker?.cancel()
    ticker = null
  }

  on('turn.complete', async ($, e, next) => {
    // サブエージェントの回答は画面に出ないので対象にしない。
    if (e.agentId || e.reason !== 'answer' || e.answer.length < MIN_CHARS) return next(e)

    const said = await $.model.complete({
      model: 'haiku',
      system: SYSTEM,
      prompt: e.answer.slice(0, 4000),
      maxTokens: 120,
    })
    const line = said.trim().split('\n')[0]?.trim() ?? ''
    // 指摘が複数の点にまたがったときは haiku が行頭に印を出す。印は落として描く。
    const marked = line.startsWith('!!')
    const body = marked ? line.slice(2).trim() : line
    tsukkomi = body === '' || body.startsWith('OK') ? null : body
    // 挙げた語が 2 つ以上あるかは、鉤括弧で囲まれた強調の数で数えられる。
    intense = marked || keywordCount(body) >= 2
    if (tsukkomi) {
      stopTicker()
      frame = 0
      const ends = firstEntranceFrame(intense) + lastFrame(speechLength(speechParts(tsukkomi)))
      ticker = $.clock.every(FRAME_MS, () => {
        frame += 1
        if (frame >= ends) stopTicker()
        $.ui.invalidate('ui.render')
      })
      $.ui.invalidate('ui.render')
    }
    return next(e)
  })

  on('turn.start', ($, _e, next) => {
    if (tsukkomi) {
      tsukkomi = null
      stopTicker()
      $.ui.invalidate('ui.render')
    }
    return next(_e)
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const line = tsukkomi
    // アンケートが帯を使っている間は譲る。
    if (!line || e.props.hasSurvey) return next(e)
    const { Box, Text } = await $.ui.resolve(e)
    const below = await next(e)

    const parts = speechParts(line)
    const cutin = frame < firstEntranceFrame(intense)
    const at = frame - firstEntranceFrame(intense)
    const paint = (cells: Cell[]) =>
      runs(cells).map((run) => Text({ color: run.color, backgroundColor: run.backgroundColor, children: run.char }))

    // カットインは帯いっぱいに描く。壁もセリフも出さず、目元だけを見せる。
    const body = cutin
      ? Box({
          flexDirection: 'column',
          children: cutinRows(frame, e.props.bodyColumns).map((cells) =>
            Text({ wrap: 'truncate-end', children: paint(cells) }),
          ),
        })
      : Box({
          flexDirection: 'row',
          gap: 1,
          children: [
            Box({
              flexDirection: 'column',
              width: PANEL_COLUMNS,
              flexShrink: 0,
              children: applyDebris(
                faceRows(
                  faceOffset(at, intense),
                  facePixelRow(at, intense),
                  FACE_AREA_WIDTH,
                  FACE_AREA_HEIGHT,
                  mouthOpen(at, speechLength(parts)),
                ),
                at,
                intense,
              ).map((cells, row) =>
                Text({
                  wrap: 'truncate-end',
                  children: [Text({ color: '#5a5a5a', children: wallRow(at, row, intense) }), ...paint(cells)],
                }),
              ),
            }),
            Box({
              flexDirection: 'column',
              justifyContent: 'center',
              flexGrow: 1,
              children: [
                Text({
                  wrap: 'wrap',
                  children: spokenRuns(at, parts).map((run) => Text({ color: run.color, children: run.text })),
                }),
              ],
            }),
          ],
        })

    return Box({
      flexDirection: 'column',
      children: [
        below,
        Box({ flexDirection: 'column', paddingTop: 1, width: e.props.bodyColumns, children: [body] }),
      ],
    })
  })
}
