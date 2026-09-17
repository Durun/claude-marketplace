// 開発用。コマを端末へ直接描いて、見た目と 1 コマの所要時間を確かめる。
// 実行: npx tsx preview.ts [コマ数]
import { renderPixels } from './hooks/scene.ts'
import { initial, step, type Game } from './hooks/game.ts'

const COLUMNS = 84
const ROWS = 22
const ESC = String.fromCharCode(27)

const paint = (pixels: Uint32Array) => {
  let out = ''
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLUMNS; col += 1) {
      const top = pixels[row * 2 * COLUMNS + col] ?? 0
      const bottom = pixels[(row * 2 + 1) * COLUMNS + col] ?? 0
      const fg = `38;2;${(top >> 16) & 255};${(top >> 8) & 255};${top & 255}`
      const bg = `48;2;${(bottom >> 16) & 255};${(bottom >> 8) & 255};${bottom & 255}`
      out += `${ESC}[${fg};${bg}m▀`
    }
    out += `${ESC}[0m\n`
  }
  return out
}

const frames = Number(process.argv[2] ?? 120)
let game: Game = initial()
let total = 0
for (let i = 0; i < frames; i += 1) {
  // 柱の手前で必ず跳ぶ自動操作。当たり判定と画角の変化を通しで見る。
  const near = game.obstacles.find((o) => o.x > 1.6 && o.x < 3.4)
  game = step(game, near !== undefined && game.runnerY === 0)
  const at = Date.now()
  const pixels = renderPixels(COLUMNS, ROWS, game, 2)
  total += Date.now() - at
  process.stdout.write(`${ESC}[H${paint(pixels)}score ${game.score}  orbit ${game.orbit.toFixed(2)}  ${game.over ? 'GAME OVER' : ''}${ESC}[K\n`)
}
process.stderr.write(`\n${(total / frames).toFixed(1)} ms/frame\n`)
