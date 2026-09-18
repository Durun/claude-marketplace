import type { EngineInterface, Register, Timer } from 'claude-code'
import { FRAME_MS, initial, step, type Game } from './game.ts'
import { render } from './scene.ts'

const SCREEN = 'screen'

/** 面の大きさ。端末が広くても、1 コマの描画時間が伸びすぎないところで止める。 */
const MAX_COLUMNS = 120
const MAX_ROWS = 20
const MIN_ROWS = 6

/** 得点の行を描き直す間隔。面そのものは毎コマ blit するので、木の組み直しは間引く。 */
const STATUS_EVERY = 10

let game: Game = initial()
let running = false
let ticker: Timer | null = null
let jumpRequested = false
let requestId = ''
let columns = 0
let rows = 0
/** 最後に描いた面。木を組み直すときに使い回し、1 コマを二度描かない。 */
let cells = ''

const stop = () => {
  ticker?.cancel()
  ticker = null
}

/** 1 コマ進めて面を差し替える時計。当たった時点で止める。 */
const start = (engine: EngineInterface) => {
  stop()
  let sinceStatus = 0
  ticker = engine.clock.every(FRAME_MS, async () => {
    const wasOver = game.over
    game = step(game, jumpRequested)
    jumpRequested = false
    // 当たっても時計は止めない。場面はそのままで、カメラだけ回り続ける。
    if (game.over && !wasOver) {
      game = { ...game, best: Math.max(game.best, game.score) }
      await engine.ui.invalidate('ui.render')
    }
    if (requestId !== '' && columns > 0) {
      cells = render(columns, rows, game)
      // 面は据え置きで中身だけ差し替える。木を組み直すより軽い。
      await engine.ui.blit({ requestId, key: SCREEN, cells })
    }
    sinceStatus += 1
    if (sinceStatus >= STATUS_EVERY) {
      sinceStatus = 0
      await engine.ui.invalidate('ui.render')
    }
  })
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'claude-run',
      description: 'Claude が走って柱を跳び越えるゲームを入力欄の上に開く。1 で跳ぶ、2 でもう一度、3 でやめる。',
    })
    return next(e)
  })

  on('command.run', { command: 'claude-run' }, async ($, e, next) => {
    game = initial(game.best)
    jumpRequested = false
    running = true
    cells = ''
    start($)
    await $.ui.invalidate('ui.render')
    return { text: 'Claude Run を始めた。1 か [ 跳ぶ ] で柱を越える。3 か [ やめる ] で閉じる。' }
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    // アンケートが帯を使っている間は譲る。
    if (!running || e.props.hasSurvey) return next(e)
    // Raster を持つのは端末だけ。他の面へは engine の描画を渡す。
    if (e.surface !== 'terminal') return next(e)

    requestId = e.requestId
    const nextColumns = Math.min(e.props.bodyColumns, MAX_COLUMNS)
    const nextRows = Math.min(e.props.maxRows - 2, MAX_ROWS)
    if (nextColumns < 24 || nextRows < MIN_ROWS) return next(e)
    if (nextColumns !== columns || nextRows !== rows || cells === '') {
      columns = nextColumns
      rows = nextRows
      cells = render(columns, rows, game)
    }

    const { Box, Text, Button, Raster } = await $.ui.resolve(e)
    const below = await next(e)

    const status = game.over
      ? `当たった   score ${game.score}   best ${game.best}`
      : `score ${game.score}   best ${game.best}`

    return Box({
      flexDirection: 'column',
      children: [
        below,
        Box({
          flexDirection: 'column',
          paddingTop: 1,
          width: columns,
          children: [
            Raster({ key: SCREEN, columns, rows, cells }),
            Box({
              flexDirection: 'row',
              gap: 2,
              children: [
                Text({ children: status }),
                game.over
                  ? Button({
                      key: 'again',
                      label: 'もう一度',
                      hotkey: '2',
                      plain: true,
                      onPress: () => {
                        game = initial(game.best)
                        jumpRequested = false
                        start($)
                      },
                    })
                  : Button({
                      key: 'jump',
                      label: '跳ぶ',
                      hotkey: '1',
                      plain: true,
                      onPress: () => {
                        jumpRequested = true
                      },
                    }),
                Button({
                  key: 'quit',
                  label: 'やめる',
                  hotkey: '3',
                  plain: true,
                  onPress: () => {
                    running = false
                    stop()
                  },
                }),
              ],
            }),
          ],
        }),
      ],
    })
  })

  // 次のターンが始まったら帯を空ける。会話の邪魔をしない。
  on('turn.start', async ($, e, next) => {
    if (running) {
      running = false
      stop()
      await $.ui.invalidate('ui.render')
    }
    return next(e)
  })
}
