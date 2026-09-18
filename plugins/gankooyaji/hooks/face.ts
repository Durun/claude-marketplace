// 1 文字セルに上下 2 ピクセルを詰めるので、ドットは偶数行で持つ。
// K=髪と眉 S=肌 E=白目 N=鼻 M=口髭 R=開いた口 .=透明
const PIXELS = [
  '..............',
  '....KKKKKK....',
  '...KKKKKKKK...',
  '..KKKKKKKKKK..',
  '..KSSSSSSSSK..',
  '..SKK.SS.KKS..',
  '..SSKKSSKKSS..',
  '..SSEKSSKESS..',
  '..SSSSSSSSSS..',
  '..SSSSNNSSSS..',
  '..SMMMMMMMMS..',
  '..SSMMMMMMSS..',
  '..SSSRRRRSSS..',
  '...SSRRRRSS...',
  '....SSSSSS....',
  '..............',
] as const

// 口を閉じたコマ。開いた口の 2 行だけを差し替える。
const MOUTH_SHUT = ['..SSSSSSSSSS..', '...SSKKKKSS...'] as const
const MOUTH_ROW = 12

const INK: Record<string, string | null> = {
  '.': null,
  K: '#2d1f14',
  S: '#e5b184',
  E: '#f5f5f5',
  N: '#c98f61',
  M: '#e8e8e8',
  R: '#7d1a1a',
}

export const FACE_WIDTH = PIXELS[0]?.length ?? 0
export const FACE_HEIGHT = PIXELS.length / 2

export type Cell = { char: string; color?: string; backgroundColor?: string }

/** ドット絵 1 体ぶん。口を閉じた行だけ差し替えて喋らせる。 */
export type Sprite = {
  pixels: readonly string[]
  ink: Record<string, string | null>
  mouthShut: readonly string[]
  mouthRow: number
}

export const OYAJI: Sprite = { pixels: PIXELS, ink: INK, mouthShut: MOUTH_SHUT, mouthRow: MOUTH_ROW }

/** 上下のドットを 1 セルに畳む。片側だけ色があるときは塗る側に寄せた半ブロックを使う。 */
const cell = (top: string | null, bottom: string | null): Cell => {
  if (top && bottom) return { char: '▀', color: top, backgroundColor: bottom }
  if (top) return { char: '▀', color: top }
  if (bottom) return { char: '▄', color: bottom }
  return { char: ' ' }
}

const pixelLine = (sprite: Sprite, line: number, mouthOpen: boolean): string => {
  const { pixels, mouthShut, mouthRow } = sprite
  if (!mouthOpen && line >= mouthRow && line < mouthRow + mouthShut.length) return mouthShut[line - mouthRow] ?? ''
  return pixels[line] ?? ''
}

const dot = (sprite: Sprite, line: number, pixel: number, mouthOpen: boolean): string | null => {
  if (line < 0 || line >= sprite.pixels.length || pixel < 0 || pixel >= (sprite.pixels[0]?.length ?? 0)) return null
  return sprite.ink[pixelLine(sprite, line, mouthOpen)[pixel] ?? '.'] ?? null
}

/**
 * 顔を右へ offsetX 列、下へ offsetY ピクセル寄せて width × height セルの面に切り出す。
 * 縦はセルではなくピクセルで受けるので、1 セルの半分だけ動かせる。
 * offsetX が負の間は左が面の外へはみ出し、壁の裏に隠れている状態になる。
 */
export const faceRows = (
  offsetX: number,
  offsetY: number,
  width: number,
  height: number,
  mouthOpen = true,
  sprite: Sprite = OYAJI,
): Cell[][] =>
  Array.from({ length: height }, (_, row) =>
    Array.from({ length: width }, (_, column) => {
      const pixel = column - offsetX
      return cell(
        dot(sprite, row * 2 - offsetY, pixel, mouthOpen),
        dot(sprite, row * 2 + 1 - offsetY, pixel, mouthOpen),
      )
    }),
  )

/** 同じ見た目のセルをまとめる。1 セル 1 要素だと行あたり 14 要素になり描画が重い。 */
export const runs = (cells: Cell[]): Cell[] => {
  const out: Cell[] = []
  for (const c of cells) {
    const last = out.at(-1)
    if (last && last.color === c.color && last.backgroundColor === c.backgroundColor) last.char += c.char
    else out.push({ ...c })
  }
  return out
}
