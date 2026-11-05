# RFC 0005: 色空間の名前の決まり

状態: 採用

## 変える理由

- `bt709` の意味が実装ごとに違った（OCIO は Rec.1886、LUT と見本は OETF の逆）。
- デコーダーが `bt709_full` のようにレンジを名前に入れていたが、RGB へ変換した時点でレンジは展開済み。
- 契約テストの「(1,1,1) は白」が、ACEScg のような空間では成り立たない。

## 名前

小文字のスネークケース。レンジは入れない。

| 名前 | 伝達関数 | 原色 |
|---|---|---|
| `lin_rec709` | 線形（作業空間） | Rec.709 |
| `srgb` | IEC 61966-2-1 の区分（静止画の既定） | Rec.709 |
| `bt709` | Rec.1886 の表示の式（`linear = v^2.4`、映像の既定） | Rec.709 |
| `bt709_oetf` | BT.709 のカメラの OETF（シーン参照の素材だけ） | Rec.709 |
| `bt601` | Rec.1886 の表示の式 | Rec.601（SMPTE 170M） |
| `bt2020` | Rec.1886 の表示の式 | Rec.2020（SDR） |

契約の白の確認は、符号化値 (1,1,1) が白になる上の全部（`UNIT_WHITE_SPACES`）に限る。それ以外（`acescg` など）は往復と `program` だけ確かめる。

## 影響するクレート

- `mm-color-api`: 名前の表と `UNIT_WHITE_SPACES`。
- `mm-types`: `ColorSpace::{bt709_oetf, bt601, bt2020}`。
- `mm-testkit`: 見本と契約テスト。
- `mm-color-lut`: `bt709` を Rec.1886 に、`bt709_oetf` を追加。
- `mm-color-ocio`: `bt709_oetf` を `Camera Rec.709`（studio-config）に対応づける。
- `mm-decode-ffmpeg`: `Matrix::color_space` は `_full` を付けない。

## 移行

1. 保存済みの文書の `bt601_full` / `bt709_full` / `bt2020_full` は `_full` を落として読む。
2. 映像の OETF として `bt709` を使っていた素材は、`bt709_oetf` に直す。
