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
| `lin_rec709` | 線形（作業色空間） | Rec.709 |
| `srgb` | IEC 61966-2-1 の区分（静止画の既定） | Rec.709 |
| `bt709` | Rec.1886 の表示の式（`linear = v^2.4`、映像の既定） | Rec.709 |
| `bt709_oetf` | BT.709 のカメラの OETF（シーン参照の素材だけ） | Rec.709 |
| `bt601` | Rec.1886 の表示の式 | Rec.601（SMPTE 170M） |
| `bt2020` | Rec.1886 の表示の式 | Rec.2020（SDR） |

契約の白の確認は、符号化値 (1,1,1) が白になる上の全部（`UNIT_WHITE_SPACES`）に限る。それ以外（`acescg` など）は往復と `program` だけ確かめる。

## bt601 と bt2020 の扱い

デコーダーは SD の素材を `bt601`、UHD の素材を `bt2020` と名乗る。レンダラーが `color_space` を引くので、
名前の表の全部を `mm-color-lut` と `mm-color-ocio` が引けるようにする（契約テストが表の全部を引く）。

- 伝達関数は 3 つとも Rec.1886（`linear = v^2.4`）。YUV → RGB の行列はデコーダーが済ませている。
- `bt601`: 原色は Rec.709 と同じものとして扱い、`bt709` と同じ変換にする。SMPTE 170M と Rec.709 の原色の差は小さく、
  組み込みの OCIO 設定にも Rec.601 の色空間が無い。差を厳密に出す必要が出たら、別の名前を足す。
- `bt2020`: 原色を作業色空間（Rec.709）に変える。広色域の色は Rec.709 の線形で負や 1 超になるので、切らずに持ち回る。
  `mm-color-api` の `REC2020_TO_REC709`（BT.2407 の行列）を `mm-color-lut` と見本が使い、`mm-color-ocio` の
  `Linear Rec.2020` と一致することを試験で確かめる。
- 負の値は伝達関数が符号を保って折り返す（`sign(v) * f(abs(v))`。CPU と WGSL で同じ）。
- OCIO の組み込み設定（studio-config）には `Rec.1886 Rec.2020 - Display` の色空間が無いので、`bt2020` は
  `mm-color-lut` で作業色空間に直してから OCIO に渡す。OCIO の別名は `bt601` → `Rec.1886 Rec.709 - Display`。

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
