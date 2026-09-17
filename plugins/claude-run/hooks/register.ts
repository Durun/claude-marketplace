import type { EngineInterface, Register, Timer } from 'claude-code'
import { FRAME_MS, initial, step, type Game } from './game.ts'
import { render } from './scene.ts'

const PANE_ID = 'claude-run'
const SCREEN = 'screen'

/** 面の大きさ。端末が広くても、1 コマの描画時間が伸びすぎないところで止める。 */
const MAX_COLUMNS = 96
const MAX_ROWS = 22

let game: Game = initial()
let ticker: Timer | null = null
let jumpRequested = false
let columns = 0
let rows = 0

const stop = () => {
  ticker?.cancel()
  ticker = null
}

/** 1 コマ進めて面を差し替える時計。当たった時点で止める。 */
const start = (engine: EngineInterface) => {
  ticker = engine.clock.every(FRAME_MS, async () => {
    const wasOver = game.over
    game = step(game, jumpRequested)
    jumpRequested = false
    if (game.over && !wasOver) {
      game = { ...game, best: Math.max(game.best, game.score) }
      stop()
    } else if (columns > 0) {
      // 面は据え置きで中身だけ差し替える。木を組み直すより軽い。
      await engine.ui.blit({ requestId: PANE_ID, key: SCREEN, cells: render(columns, rows, game) })
    }
    // 得点と操作の行は木の側にあるので、そちらも描き直す。
    await engine.ui.invalidate('ui.render')
  })
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'dino',
      description: 'Claude が走る。押すと跳ぶ。柱に当たるまで。',
    })
    return next(e)
  })

  on('command.run', { command: 'dino' }, async ($, e, next) => {
    game = initial(game.best)
    jumpRequested = false
    stop()
    await $.ui.open({ id: PANE_ID, title: 'Claude Run', focus: true, closeOnEscape: true, rows: MAX_ROWS + 2 })
    start($)
    return { text: 'Claude Run を開いた。j か [ 跳ぶ ] で跳ぶ。Esc で閉じる。' }
  })

  on('ui.close', { id: PANE_ID }, ($, e, next) => {
    stop()
    return next(e)
  })

  on('ui.render', { component: 'Pane', requestId: PANE_ID }, async ($, e, next) => {
    // Raster を持つのは端末だけ。他の面へは engine の描画を渡す。
    if (e.surface !== 'terminal') return next(e)
    const { Box, Text, Button, Raster } = await $.ui.resolve(e)
    columns = Math.min(e.props.bodyColumns, MAX_COLUMNS)
    rows = Math.min(e.props.scroll.bodyRows - 2, MAX_ROWS)
    if (columns < 8 || rows < 4) return next(e)

    const status = game.over
      ? `当たった  score ${game.score}  best ${game.best}`
      : `score ${game.score}  best ${game.best}`

    return Box({
      flexDirection: 'column',
      children: [
        Raster({ key: SCREEN, columns, rows, cells: render(columns, rows, game) }),
        Box({
          flexDirection: 'row',
          gap: 2,
          children: [
            Text({ children: status }),
            game.over
              ? Button({
                  key: 'again',
                  label: 'もう一度',
                  hotkey: 'r',
                  onPress: () => {
                    jumpRequested = false
                    game = initial(game.best)
                  },
                })
              : Button({
                  key: 'jump',
                  label: '跳ぶ',
                  hotkey: 'j',
                  onPress: () => {
                    jumpRequested = true
                  },
                }),
          ],
        }),
      ],
    })
  })

  // もう一度を押した後は止まった時計を掛け直す。
  on('ui.press', { element: 'again' }, async ($, e, next) => {
    const result = await next(e)
    if (!ticker) start($)
    return result
  })
}
