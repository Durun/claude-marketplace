// 語を 2 つ以上見つけたときだけ、登場の前に差し込む目元のカットイン。
// 1 文字セルに上下 2 ピクセルを詰めるので、ドットは偶数行で持つ。
// K=眉と瞳 S=肌 E=白目 H=瞳の光 .=透明
import type { Cell } from './face.ts'

const PIXELS = [
  '..KKKKKKKKKKKKKKKKKKKKKKK..',
  '..KKKKKKKKKKKKKKKKKKKKKKK..',
  '..SKKKKKKKKKKKKKKKKKKKKKS..',
  '..SSSSSSSSSSSSSSSSSSSSSSS..',
  '..SSKKKKSSSSSSSSSSSKKKKSS..',
  '..SSSKKKKKSSSSSSSKKKKKSSS..',
  '..SSSSKKKKKKSSSKKKKKKSSSS..',
  '..SSSSSSKKKKSSSKKKKSSSSSS..',
  '..SKKKKKKKKSSSSSKKKKKKKKS..',
  '..SEEHKKKEESSSSSEEKKKHEES..',
  '..SEKKKKKKESSSSSEKKKKKKES..',
  '..SKKKKKKKKSSSSSKKKKKKKKS..',
  '..SSSSSSSSSSSSSSSSSSSSSSS..',
  '..SSSSSSSSSSSSNSSSSSSSSSS..',
  '..SSSSSSSSSSSNNNSSSSSSSSS..',
  '..SSSSSSSSSSSNNNSSSSSSSSS..',
  '..SSSSSSSSSSSSSSSSSSSSSSS..',
  '..SSSSSSSSSSSSSSSSSSSSSSS..',
] as const

const INK: Record<string, string | null> = {
  '.': null,
  K: '#1a1008',
  S: '#e5b184',
  E: '#f5f5f5',
  H: '#ffffff',
  N: '#c98f61',
}

export const CUTIN_WIDTH = PIXELS[0]?.length ?? 0
export const CUTIN_HEIGHT = PIXELS.length / 2

/** カットインのコマ数。1 コマ目は白飛びで、残りは目と集中線が流れる。 */
export const CUTIN_FRAMES = 8

const FLASH = '#ffffff'

// 集中線の色。手前ほど明るい 2 本を交互に流す。
const RAY_COLORS = ['#ffd166', '#8a6a2a'] as const

/**
 * 背景の流れる線を、顔の外にだけ引く。列と行から決まる位相をコマで進めるので、
 * 線が一方向に流れる。向きはオヤジが壁から飛び出す向きに揃える。
 * 絵として持たないので、顔を描き直しても線は付いてくる。
 */
const ray = (row: number, column: number, frame: number): Cell | null => {
  const phase = (column * 7 + row * 3 + frame * 4) % 10
  const index = ((phase % 10) + 10) % 10
  if (index >= RAY_COLORS.length) return null
  return { char: '▀', color: RAY_COLORS[index] }
}

const dot = (line: number, pixel: number): string | null => INK[PIXELS[line]?.[pixel] ?? '.'] ?? null

const cell = (top: string | null, bottom: string | null): Cell => {
  if (top && bottom) return { char: '▀', color: top, backgroundColor: bottom }
  if (top) return { char: '▀', color: top }
  if (bottom) return { char: '▄', color: bottom }
  return { char: ' ' }
}

/** そのコマのカットインを width セル幅いっぱいに描く。顔は中央に置き、左右は流れる線で埋める。 */
export const cutinRows = (frame: number, width: number): Cell[][] => {
  const offset = Math.floor((width - CUTIN_WIDTH) / 2)
  return Array.from({ length: CUTIN_HEIGHT }, (_, row) =>
    Array.from({ length: width }, (_, column) => {
      if (frame === 0) return { char: '█', color: FLASH }
      const pixel = column - offset
      const top = dot(row * 2, pixel)
      const bottom = dot(row * 2 + 1, pixel)
      if (!top && !bottom) return ray(row, column, frame) ?? { char: ' ' }
      return cell(top, bottom)
    }),
  )
}
