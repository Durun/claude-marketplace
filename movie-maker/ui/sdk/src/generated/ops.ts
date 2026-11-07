// ops.json から tools/gen-sdk.ts が生成する。直接は書き換えない。
import type { JsonSchema } from "../codegen.ts";

/** 素材。原本のパス、コンテンツハッシュ、色空間、時間軸。 */
export type Asset = {
  "color_space": ColorSpace;
  "content_hash": string;
  "duration": RationalTime;
  /** 固定フレームレートなら fps。VFR は `None`（PTS の索引で引く）。 */
  "fps"?: Rational | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  "path": string;
  /** 音と映像の先頭のずれ。OBS の mkv で起きる。 */
  "timebase_offset"?: RationalTime;
};
/** 合成モード。式は W3C Compositing and Blending Level 1。`Add` は線形の加算（linear dodge）。 */
export type BlendMode = "normal" | "multiply" | "screen" | "overlay" | "darken" | "lighten" | "color_dodge" | "color_burn" | "hard_light" | "soft_light" | "difference" | "exclusion" | "hue" | "saturation" | "color" | "luminosity" | "add";
/** 字幕の置き場所。値はコンポジションの画素。 */
export type CaptionPlacement = {
  /** 字幕の帯の下端の中央（Vec2）。 */
  "position": Param;
  /** 字幕の帯の幅（Number）。これを超える行は折り返す。 */
  "width": Param;
};
/** 色空間の名前。OCIO の名前をそのまま持つ（`bt709`、`srgb`、`lin_rec709` など）。 */
export type ColorSpace = string;
/** コンポジションのミキサーの設定。既定は全部 0 dB でダッキングなし。 */
export type CompMixer = {
  /** 役割ごとのバス。載っていない役割は 0 dB。 */
  "buses"?: Record<string, MixerBus>;
  /** ダッキング。`None` で無効。 */
  "ducking"?: Ducking | null;
  /** マスターの音量（dB）。 */
  "master_db"?: number;
};
/** コンポジション。大きさ、fps、長さ、レイヤーの重なり、マーカー。 */
export type Composition = {
  "duration": RationalTime;
  "fps": Rational;
  "height": number;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** 下から上の順。 */
  "layers": Layer[];
  /** 時刻の順。 */
  "markers"?: Marker[];
  /** ミキサーの設定。`None` は既定（全部 0 dB、ダッキングなし）。 */
  "mixer"?: CompMixer | null;
  "width": number;
};
/** 切り抜き。レイヤーの画像の縁から内側へ削る幅（画素）。 DaVinci Resolve の Inspector の Cropping と Premiere Pro の Crop と同じく上下左右を別々に持ち、どれもキーフレームできる。 無い辺は削らない。 */
export type Crop = {
  "bottom"?: Param | null;
  "left"?: Param | null;
  "right"?: Param | null;
  "top"?: Param | null;
};
/** 進み具合 `p ∈ [0,1]` を曲げる 3 次ベジェ。`(0,0)` と `(1,1)` を端とし、制御点が `(x1,y1)`、`(x2,y2)`。 `x1`、`x2` は `[0,1]` に収める（時間が単調になる）。 */
export type CubicBezier = {
  "x1": number;
  "x2": number;
  "y1": number;
  "y2": number;
};
/** カット。素材の範囲を 1 つ切り出す。 */
export type Cut = {
  /** 外したカットは消さずに残し、配置から除く。 */
  "enabled"?: boolean;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** 素材の時間軸でのイン点。 */
  "in": RationalTime;
  /** 参照・複製の元のカット。ショートで in/out を上書きしたときの対応に使う。 */
  "origin"?: string | null;
  /** ショートで、分けたときの元のカットの値。元が変わったか（参照なら元を割ったか）の比べ先。 */
  "origin_base"?: CutBase | null;
  /** 素材の時間軸での出（含まない）。`in < out`。 */
  "out": RationalTime;
  /** 参照のショートで、元のカットから上書きした所。上書きしていない所は元のカットに従う。 */
  "overrides"?: CutOverrides;
  "source": Node;
  "speed"?: Rational;
  /** 前のカットからこのカットへのトランジション。境目をまたいで掛け、配置は変えない。 */
  "transition_in"?: Transition | null;
};
/** 元のカットの、ショートが従う・比べる値（素材、イン点、アウト点、速さ、有効）。 */
export type CutBase = {
  "enabled": boolean;
  "in": RationalTime;
  "out": RationalTime;
  "source": Node;
  "speed": Rational;
};
/** 参照のショートのカットで上書きした所。真の所は元のカットに従わない。 */
export type CutOverrides = {
  "in": boolean;
  "out": boolean;
};
/** sequence の中の続いたカットの範囲。`first` が `last` より前か同じ。 */
export type CutRange = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "first": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "last": string;
};
/** カット 1 つの再フレーミング。カットの画像（コンポジションの画布に縦横比を保って収めたもの）に掛ける変形。 キーフレームの時刻は素材の時刻（被写体の位置は素材の時刻で決まるので、カットを動かしても切り出しがずれない）。 */
export type CutReframe = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "transform": Transform;
};
/** シーケンスの `index` から `count` 個のカットを `cuts` で置き換える。`cutWords` の逆操作が持つ形。 */
export type CutSplice = {
  "count": number;
  "cuts": Cut[];
  "index": number;
};
/** 納品の設定。字幕とチャプターの出力と、トゥルーピークの上限。 ラウドネスの目標は `RenderTarget::loudness_lufs`（YouTube は -14、放送は -23）。 */
export type Delivery = {
  /** 字幕を書き出しの絵に描き込む。文字起こしから作った字幕を text ノードのレイヤーにするので、プレビューと同じ描画になる。 置き場所はショートの `setCaption`（無ければ下部中央）。字幕のファイルや mov_text（`subtitles`）とは独立。 */
  "burn_in_captions"?: boolean;
  /** コンポジションのマーカーを MP4 のチャプターにし、YouTube の説明欄用の `<名前>.chapters.txt` を置く。 */
  "chapters"?: boolean;
  /** 字幕の置き方。字幕はシーケンスのカットの文字起こしから作る。 */
  "subtitles"?: SubtitleOutput;
  /** トゥルーピークの上限（dBTP）。`None` なら [`DEFAULT_TRUE_PEAK_DBTP`]。ラウドネスの正規化のときだけ効く。 */
  "true_peak_dbtp"?: number | null;
};
/** ダッキング。`trigger` のバスが鳴っている間、`targets` のバスを `reduction_db` だけ下げる。 */
export type Ducking = {
  /** 下がりきるまでの時間（ms）。 */
  "attack_ms": number;
  /** 下げる量（dB）。0 以下。 */
  "reduction_db": number;
  /** 戻りきるまでの時間（ms）。 */
  "release_ms": number;
  /** 下げられるバスの役割（例: `bgm`）。 */
  "targets": string[];
  /** 鳴っていると見なす大きさ（dBFS）。 */
  "threshold_db": number;
  /** 鳴ると他を下げるバスの役割（例: `voice`）。 */
  "trigger": string;
};
/** `Linear` の進み具合に掛けるイージングの型。 */
export type Ease = "none" | "in" | "out" | "in_out";
/** 動かす端。 */
export type Edge = "in" | "out";
/** 効果。ノードの種類を 1 つ、レイヤーの画像を入力にして掛ける。 */
export type Effect = {
  "enabled"?: boolean;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  "node": Node;
};
/** 音のフェード。DaVinci Resolve と Final Cut Pro のクリップのフェードのハンドルと同じく、音量のキーフレームとは別に レイヤーの先頭（`fade_in`）と末尾（`fade_out`）に持ち、音量に掛ける。 */
export type Fade = {
  "curve"?: FadeCurve;
  /** フェードの長さ（コンポジションの時間）。正。 */
  "duration": RationalTime;
};
/** フェードの曲線。進み具合 `p`（無音の側が 0、鳴る側が 1）から振幅の倍率を決める。 */
export type FadeCurve = "linear" | "equal_power" | "db";
/** sequence のレイヤーの、コンポジションごとの見せ方。同じ素材を本編（16:9）とショート（9:16）で別の位置から切り出す。 */
export type Framing = {
  /** 字幕の置き場所。`None` は字幕を描く側の既定。 */
  "caption"?: CaptionPlacement | null;
  /** カットごとの再フレーミング。カットの ID は重ならない。無いカットは変形なし。 */
  "cuts": CutReframe[];
};
/** キーフレームの補間。区間 `[k_i, k_{i+1})` の形は始点 `k_i` の補間で決まる。 数の型（数、2・3 次元の数、色）だけ `Linear` と `Bezier` が効き、ほかはホールド。 */
export type Interpolation = "hold" | "linear" | "bezier";
/** キーフレーム。 */
export type Keyframe = {
  /** `Bezier` の制御点。無ければ [`CubicBezier::EASE_IN_OUT`]。 */
  "bezier"?: CubicBezier | null;
  /** `Linear` の進み具合に掛ける。 */
  "ease"?: Ease;
  "interpolation": Interpolation;
  "time": RationalTime;
  "value": ParamValue;
};
/** レイヤー。ノードを 1 つ持ち、時間と重ね方を足す。 コンポジションの時間軸での範囲は `[start, start + (out - in) / speed)`。 `time_remap` があっても範囲は変わらず、範囲の中の写像だけ変わる。 */
export type Layer = {
  /** 接続先のカット。カットがリップルで動くと一緒に動く。 */
  "attach_to"?: string | null;
  "audio"?: LayerAudio | null;
  "blend"?: BlendMode;
  /** 切り抜き。変形の前のレイヤーの画像に掛ける。 */
  "crop"?: Crop;
  /** 効果の列。上から順に掛ける。 */
  "effects"?: Effect[];
  /** 無効にしたレイヤーは描かず、鳴らさない。 */
  "enabled"?: boolean;
  /** sequence のレイヤーで、このコンポジションでのカットごとの再フレーミングと字幕の置き場所。 */
  "framing"?: Framing;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** 素材の時間軸でのイン点。 */
  "in": RationalTime;
  /** 人が直した所。Claude の操作経路からの操作は拒否する。 */
  "lock"?: boolean;
  "masks"?: Mask[];
  "opacity": Param;
  /** 素材の時間軸での出（含まない）。`in < out`。 */
  "out": RationalTime;
  /** 親のレイヤー（同じコンポジションの中）。変形を親に続けて掛ける。 */
  "parent"?: string | null;
  "source": Node;
  /** 再生の速さ。1 が等速。型が正を保つので 0 にならない。 */
  "speed"?: Rational;
  /** コンポジションの時間軸での始まり。 */
  "start": RationalTime;
  /** 時間の写像の上書き。あれば `in` と `speed` の代わりに使う。 */
  "time_remap"?: TimeRemap | null;
  "track_matte"?: TrackMatte | null;
  /** 位置・拡大・回転・アンカー。レンダラーが合成で掛ける（ノードの種類には渡さない）。 */
  "transform"?: Transform;
};
/** レイヤーの音の設定。 */
export type LayerAudio = {
  /** 先頭のフェード。音量に掛ける。 */
  "fade_in"?: Fade | null;
  /** 末尾のフェード。音量に掛ける。 */
  "fade_out"?: Fade | null;
  "gain_db": Param;
  /** 鳴らさない。プレビューと書き出しの両方に効く。 */
  "mute"?: boolean;
  "pan": Param;
  /** 役割（`voice`、`bgm`、`se`）。ミキサーのバスとダッキングが見る。 */
  "role": string;
  /** ソロ。同じコンポジションにソロのレイヤーが 1 つでもあれば、ソロでないレイヤーは鳴らさない。 */
  "solo"?: boolean;
};
/** マーカー。YouTube のチャプターの書き出しに使う。`duration` が 0 なら点。 */
export type Marker = {
  "duration"?: RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  "name": string;
  "time": RationalTime;
};
/** マスク。形は角丸の矩形（`rect`）か、ベジェの頂点の列（`path`。型だけで、レンダラーは描かない）。 座標はレイヤーの座標で、変形と一緒に動く（After Effects のレイヤーのマスクと同じ）。 */
export type Mask = {
  /** ぼかしの幅（画素）。縁を中心にこの幅で 0 から 1 へ移る。 */
  "feather"?: Param;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  "inverted"?: boolean;
  "mode"?: MaskMode;
  "opacity"?: Param;
  /** 頂点の列。キーフレームで形を動かせる。`rect` があれば使わない。 */
  "path"?: Param;
  /** 角丸の矩形。あれば形はこれで決まる。 */
  "rect"?: MaskRect | null;
};
/** マスクの組み合わせ方。 */
export type MaskMode = "add" | "subtract" | "intersect";
/** 角丸の矩形のマスクの形。DaVinci Resolve（Fusion）の Rectangle Mask と geneva の `mask` の `rect` に合わせ、 位置・大きさ・角の半径をパラメータで持つ（どれもキーフレームできる）。 */
export type MaskRect = {
  /** 矩形の左上（Vec2）。 */
  "position": Param;
  /** 角の半径（Number）。短い辺の半分で頭打ちにする。 */
  "radius"?: Param;
  /** 矩形の幅と高さ（Vec2）。 */
  "size": Param;
};
/** マットに使う成分。 */
export type MatteKind = "alpha" | "luma";
/** バス 1 本の設定。 */
export type MixerBus = {
  /** バスの音量（dB）。 */
  "gain_db": number;
};
/** ノード。ノードの種類の名前とバージョン、パラメータ、入力。 */
export type Node = {
  "inputs"?: Node[];
  "kind": string;
  "params"?: Record<string, Param>;
  "version": number;
};
/** 参照（本編を直すとショートも変わる）か、複製（分岐。元が変わったら知らせる）か。 */
export type OriginRelation = "reference" | "fork";
/** パラメータ。固定の値か、キーフレームの列（時刻の昇順）。 キーフレームがあるときは `value` を使わず、最初のキーフレームより前は最初の値、 最後より後は最後の値になる（`mm-eval`）。 */
export type Param = {
  "keyframes"?: Keyframe[];
  "value": ParamValue;
};
/** パラメータの指し方。`layer` か `cut` のどちらか 1 つ。 */
export type ParamRef = {
  /** カットの ID（カットのノードのパラメータを指すとき）。 */
  "cut"?: string | null;
  /** レイヤーの ID（レイヤーのパラメータを指すとき）。 */
  "layer"?: string | null;
  /** 持ち主の中のパス。 */
  "path": string;
};
/** 評価済みのパラメータの値（時刻 t での値）。 */
export type ParamValue = {
  "type": "number";
  "value": number;
} | {
  "type": "vec2";
  "value": number[];
} | {
  "type": "vec3";
  "value": number[];
} | {
  "type": "color";
  "value": Rgba;
} | {
  "type": "bool";
  "value": boolean;
} | {
  "type": "text";
  "value": string;
} | {
  "type": "choice";
  "value": string;
} | {
  "type": "asset_ref";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "value": string;
} | {
  "type": "json";
  "value": unknown;
};
/** 評価。FCP の Favorite / Reject と同じ 3 値。 */
export type Rating = "unrated" | "favorite" | "rejected";
/** 正の有理数 num/den（fps、再生の速さ） */
export type Rational = {
  "den": number;
  "num": number;
};
/** 有理数の時刻 num/den 秒（負も表せる） */
export type RationalTime = {
  "den": number;
  "num": number;
};
/** レンダーターゲット。成果物の形式。 */
export type RenderTarget = {
  /** エンコーダーの名前（`h264_nvenc`、`prores_ks`、モックは `sample`）。 */
  "codec": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  /** 字幕、チャプター、トゥルーピークの上限。 */
  "delivery"?: Delivery;
  "fps": Rational;
  "height": number;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  "loudness_lufs"?: number | null;
  "output_path": string;
  "width": number;
};
/** 線形、premultiplied の RGBA。各成分は 0..=1 の範囲が基本だが、HDR では 1 を超えてよい。 */
export type Rgba = {
  "a": number;
  "b": number;
  "g": number;
  "r": number;
};
/** 選別。素材の中の範囲に評価とタグを付ける（FCP のキーワード付きの範囲）。 */
export type Selection = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** 素材の時間軸でのイン点。 */
  "in": RationalTime;
  /** 素材の時間軸でのアウト点（含まない）。`in < out`。 */
  "out": RationalTime;
  "rating"?: Rating;
  /** 辞書順で重複しない。 */
  "tags"?: string[];
};
/** マグネティックシーケンス。OTIO の `Track` に対応づけられる形。 */
export type Sequence = {
  /** 先頭から順。 */
  "cuts": Cut[];
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** ショートが本編を参照・複製した元。本編なら `None`。 */
  "origin"?: SequenceOrigin | null;
};
/** ショートと本編の関係。 */
export type SequenceOrigin = {
  "relation": OriginRelation;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** 素材の時刻の範囲 `[in, out)`。 */
export type SourceRange = {
  "in": RationalTime;
  "out": RationalTime;
};
/** 字幕の置き方。 */
export type SubtitleOutput = "none" | "sidecar" | "embedded";
/** タイムリマップのキーフレーム。補間は `Hold` と `Linear` だけ（時刻は有理数のまま変換する）。 */
export type TimeKeyframe = {
  "interpolation": Interpolation;
  /** 素材の時刻。 */
  "source": RationalTime;
  /** レイヤーの始まりからの時刻。 */
  "time": RationalTime;
};
/** タイムリマップ。レイヤーの始まりからの時刻（`time`）を素材の時刻（`source`）に変換する折れ線。 最初より前と最後より後は端の値を保つ。 */
export type TimeRemap = {
  "keyframes": TimeKeyframe[];
};
/** 話題。文字起こしの区間（素材の中の範囲）に見出しを付けたもの。Story が並べ、`insertTopics` がカットにする。 */
export type Topic = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "id": string;
  /** 素材の時間軸でのイン点。 */
  "in": RationalTime;
  /** 素材の時間軸でのアウト点（含まない）。`in < out`。 */
  "out": RationalTime;
  "rating"?: Rating;
  /** 辞書順で重複しない。 */
  "tags"?: string[];
  "title": string;
};
/** トラックマット。同じコンポジションの別のレイヤーのアルファか輝度で切り抜く。 */
export type TrackMatte = {
  "inverted"?: boolean;
  "kind": MatteKind;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
};
/** レイヤーの変形。値はコンポジションの画素と度。 */
export type Transform = {
  /** 回転と拡大の中心（Vec2）。`None` はレイヤーの画像の中心。 */
  "anchor"?: Param | null;
  /** 動かす量（Vec2）。 */
  "position"?: Param;
  /** 度。正が時計回り。 */
  "rotation"?: Param;
  /** 倍率（Number か Vec2）。1 が等倍。 */
  "scale"?: Param;
};
/** カットの間のトランジション。 */
export type Transition = {
  "duration": RationalTime;
  "node": Node;
};

/** 素材を足す。 */
export type AddAssetOp = {
  "asset": Asset;
  "index"?: number | null;
  "op": "addAsset";
};
/** 素材を外す。 */
export type RemoveAssetOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "op": "removeAsset";
};
/** レイヤーを足す。 */
export type AddLayerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  "index"?: number | null;
  "layer": Layer;
  "op": "addLayer";
};
/** レイヤーを外す。 */
export type RemoveLayerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "removeLayer";
};
/** レイヤーにマスクを足す。 */
export type AddMaskOp = {
  "index"?: number | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "mask": Mask;
  "op": "addMask";
};
/** レイヤーからマスクを外す。 */
export type RemoveMaskOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "mask": string;
  "op": "removeMask";
};
/** レイヤーの重なりの位置を変える。 */
export type MoveLayerOp = {
  "index": number;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "moveLayer";
};
/** パラメータの固定の値を置く。 */
export type SetParamOp = {
  "keyframes"?: Keyframe[] | null;
  "op": "setParam";
  "target": ParamRef;
  "value": ParamValue;
};
/** パラメータを外す（無くてもよいフィールドだけ）。 */
export type RemoveParamOp = {
  "op": "removeParam";
  "target": ParamRef;
};
/** キーフレームを置く。 */
export type SetKeyframeOp = {
  "keyframe": Keyframe;
  "op": "setKeyframe";
  "target": ParamRef;
};
/** キーフレームを外す。 */
export type RemoveKeyframeOp = {
  "op": "removeKeyframe";
  "target": ParamRef;
  "time": RationalTime;
};
/** キーフレームの補間とイージングを変える。 */
export type SetEaseOp = {
  /** `Bezier` の制御点。`None` は既定（ease-in-out）。 */
  "bezier"?: CubicBezier | null;
  "ease"?: Ease;
  "interpolation": Interpolation;
  "op": "setEase";
  "target": ParamRef;
  "time": RationalTime;
};
/** キーフレームの時刻と中身を置き直す（外すと置くを 1 つにする）。 */
export type MoveKeyframeOp = {
  /** 動かすキーフレームの今の時刻。 */
  "from": RationalTime;
  /** 置き直した後のキーフレーム（時刻、値、区間の形）。 */
  "keyframe": Keyframe;
  "op": "moveKeyframe";
  "target": ParamRef;
};
/** 親のレイヤーを付ける・外す。 */
export type ParentLayerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "parentLayer";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "parent"?: string | null;
};
/** カットへの接続を付ける・外す。 */
export type AttachLayerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut"?: string | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "attachLayer";
};
/** ロックを付ける・外す。 */
export type LockLayerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "lock": boolean;
  "op": "lockLayer";
};
/** レイヤーの組を新しいコンポジションに入れ、入れ子のレイヤーで置き換える。 */
export type PrecomposeOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  "layers": string[];
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "new_composition": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "new_layer": string;
  "op": "precompose";
};
/** 入れ子のレイヤーを中のレイヤーに戻す（precompose の逆）。 */
export type UnprecomposeOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "unprecompose";
};
/** コンポジションを足す。 */
export type AddCompositionOp = {
  "composition": Composition;
  "op": "addComposition";
};
/** コンポジションを外す。 */
export type RemoveCompositionOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  "op": "removeComposition";
};
/** sequence を足す。 */
export type AddSequenceOp = {
  "op": "addSequence";
  "sequence": Sequence;
};
/** sequence を参照か複製で分岐する。 */
export type ForkSequenceOp = {
  /** 新しいカットの ID。コピーする元のカットと同じ数。 */
  "cuts": string[];
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "new_sequence": string;
  "op": "forkSequence";
  /** コピーする続いたカットの範囲（両端を含む）。省くと全部。 */
  "range"?: CutRange | null;
  "relation": OriginRelation;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** sequence を外す。 */
export type RemoveSequenceOp = {
  "op": "removeSequence";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** 参照のショートのカットの in/out を上書きする・上書きを外す。 */
export type OverrideCutOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  /** 上書きするイン点（素材の時間軸）。 */
  "in"?: RationalTime | null;
  "op": "overrideCut";
  /** 上書きするアウト点（素材の時間軸、含まない）。 */
  "out"?: RationalTime | null;
};
/** 参照のショートのカットの上書きを全部外す。 */
export type ResetCutOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "resetCut";
};
/** sequence のレイヤーのカットの再フレーミングを置く・外す。 */
export type SetReframeOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "setReframe";
  "transform"?: Transform | null;
};
/** sequence のレイヤーの字幕の置き場所を置く・外す。 */
export type SetCaptionOp = {
  "caption"?: CaptionPlacement | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "setCaption";
};
/** レイヤーの音の設定（送り先のバス、音量、パン、ミュート、ソロ）を置く・外す。 */
export type SetLayerAudioOp = {
  "audio"?: LayerAudio | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "setLayerAudio";
};
/** コンポジションのミキサーの設定（バスの音量、マスター、ダッキング）を置く・外す。 */
export type SetMixerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  "mixer"?: CompMixer | null;
  "op": "setMixer";
};
/** カットの頭のトランジションを置く・差し替える・外す。 */
export type SetTransitionOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "setTransition";
  "transition"?: Transition | null;
};
/** レンダーターゲットを足す。 */
export type AddRenderTargetOp = {
  "index"?: number | null;
  "op": "addRenderTarget";
  "target": RenderTarget;
};
/** レンダーターゲットを外す。 */
export type RemoveRenderTargetOp = {
  "op": "removeRenderTarget";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "target": string;
};
/** マーカーを足す。 */
export type AddMarkerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "composition": string;
  "marker": Marker;
  "op": "addMarker";
};
/** マーカーを外す。 */
export type RemoveMarkerOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "marker": string;
  "op": "removeMarker";
};
/** カットをシーケンスに差し込む（後ろがずれる）。 */
export type InsertOp = {
  "cut": Cut;
  "index": number;
  "op": "insert";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** カットを別のカットで置き換える（数は変わらない）。 */
export type OverwriteOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "overwrite";
  "replacement": Cut;
};
/** カットを抜く（後ろが詰まる）。 */
export type ExtractOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "extract";
};
/** カットを 2 つに割る。 */
export type SplitOp = {
  "at": RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "new_cut": string;
  "op": "split";
};
/** 続きの 2 つのカットを 1 つに戻す。 */
export type JoinOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "next": string;
  "op": "join";
};
/** カットのイン点かアウト点を動かし、後ろをずらす。 */
export type RippleOp = {
  "by": RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "edge": Edge;
  "op": "ripple";
};
/** 次のカットとの編集点を動かす（全体の長さは変わらない）。 */
export type RollOp = {
  "by": RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "roll";
};
/** カットの内容をずらす（位置と長さは変わらない）。 */
export type SlipOp = {
  "by": RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "slip";
};
/** カットを前後のカットの中で動かす（全体の長さは変わらない）。 */
export type SlideOp = {
  "by": RationalTime;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "slide";
};
/** カットを配置から外す・戻す。 */
export type DisableOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  /** `true` で外し、`false` で戻す。 */
  "disabled"?: boolean;
  "op": "disable";
};
/** カットを抜いて同じ長さの隙間を残す。 */
export type LiftOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "cut": string;
  "op": "lift";
};
/** 続いたカットの列を差し込む。 */
export type InsertCutsOp = {
  "cuts": Cut[];
  "index": number;
  "op": "insertCuts";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** 続いたカットの列を抜く。 */
export type ExtractCutsOp = {
  "count": number;
  "index": number;
  "op": "extractCuts";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
};
/** 話題の列からカットを作って差し込む。 */
export type InsertTopicsOp = {
  "cuts": string[];
  "index"?: number | null;
  "op": "insertTopics";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
  "topics": string[];
};
/** sequence のレイヤーとコンポジションの長さを sequence の長さに合わせる。 */
export type FitLayerOp = {
  "duration"?: RationalTime | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "fitLayer";
  "out_point"?: RationalTime | null;
};
/** レイヤーのイン点かアウト点を動かす。 */
export type TrimLayerOp = {
  "by": RationalTime;
  "edge": Edge;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "layer": string;
  "op": "trimLayer";
};
/** 素材のイン点を付ける・外す。 */
export type MarkInOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "at"?: RationalTime | null;
  "op": "markIn";
};
/** 素材のアウト点を付ける・外す。 */
export type MarkOutOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "at"?: RationalTime | null;
  "op": "markOut";
};
/** 選別を足す。 */
export type AddSelectionOp = {
  "op": "addSelection";
  "selection": Selection;
};
/** 選別を外す。 */
export type RemoveSelectionOp = {
  "op": "removeSelection";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "selection": string;
};
/** 選別か話題の評価を変える。 */
export type RateOp = {
  "op": "rate";
  "rating": Rating;
  /** 選別か話題の ID。 */
  "target": string;
};
/** 選別か話題にタグを付ける・外す。 */
export type TagOp = {
  "op": "tag";
  "remove"?: boolean;
  "tag": string;
  /** 選別か話題の ID。 */
  "target": string;
};
/** 話題を足す。 */
export type AddTopicOp = {
  "op": "addTopic";
  "topic": Topic;
};
/** 話題を外す。 */
export type RemoveTopicOp = {
  "op": "removeTopic";
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "topic": string;
};
/** 文字起こしの単語を消す・戻す。 */
export type DeleteWordsOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "op": "deleteWords";
  "restore"?: boolean;
  "words": string[];
};
/** 文字起こしの単語を消し、その音をシーケンスのカットから抜いて詰める。 */
export type CutWordsOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "op": "cutWords";
  "ranges"?: SourceRange[];
  "restore"?: boolean;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "sequence": string;
  "splices"?: CutSplice[];
  "words"?: string[];
};
/** 文字起こしの単語の文字を直す。 */
export type FixTranscriptOp = {
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "asset": string;
  "op": "fixTranscript";
  "text"?: string | null;
  /** ID（種類ごとにプロジェクトの中で重複しない文字列） */
  "word": string;
};

/** 全部の操作。`op` が名前。 */
export type Op =
  | AddAssetOp
  | RemoveAssetOp
  | AddLayerOp
  | RemoveLayerOp
  | AddMaskOp
  | RemoveMaskOp
  | MoveLayerOp
  | SetParamOp
  | RemoveParamOp
  | SetKeyframeOp
  | RemoveKeyframeOp
  | SetEaseOp
  | MoveKeyframeOp
  | ParentLayerOp
  | AttachLayerOp
  | LockLayerOp
  | PrecomposeOp
  | UnprecomposeOp
  | AddCompositionOp
  | RemoveCompositionOp
  | AddSequenceOp
  | ForkSequenceOp
  | RemoveSequenceOp
  | OverrideCutOp
  | ResetCutOp
  | SetReframeOp
  | SetCaptionOp
  | SetLayerAudioOp
  | SetMixerOp
  | SetTransitionOp
  | AddRenderTargetOp
  | RemoveRenderTargetOp
  | AddMarkerOp
  | RemoveMarkerOp
  | InsertOp
  | OverwriteOp
  | ExtractOp
  | SplitOp
  | JoinOp
  | RippleOp
  | RollOp
  | SlipOp
  | SlideOp
  | DisableOp
  | LiftOp
  | InsertCutsOp
  | ExtractCutsOp
  | InsertTopicsOp
  | FitLayerOp
  | TrimLayerOp
  | MarkInOp
  | MarkOutOp
  | AddSelectionOp
  | RemoveSelectionOp
  | RateOp
  | TagOp
  | AddTopicOp
  | RemoveTopicOp
  | DeleteWordsOp
  | CutWordsOp
  | FixTranscriptOp;

/** 操作の名前の和。 */
export type OpName = Op["op"];

/** 名前から操作を引く。 */
export type OpOf<N extends OpName> = Extract<Op, { op: N }>;

/** 名前から、`op` を除いた引数の型を引く。 */
export type OpArgs<N extends OpName> = Omit<OpOf<N>, "op">;

/** 全部の操作の名前。 */
export const OP_NAMES: readonly OpName[] = [
  "addAsset",
  "removeAsset",
  "addLayer",
  "removeLayer",
  "addMask",
  "removeMask",
  "moveLayer",
  "setParam",
  "removeParam",
  "setKeyframe",
  "removeKeyframe",
  "setEase",
  "moveKeyframe",
  "parentLayer",
  "attachLayer",
  "lockLayer",
  "precompose",
  "unprecompose",
  "addComposition",
  "removeComposition",
  "addSequence",
  "forkSequence",
  "removeSequence",
  "overrideCut",
  "resetCut",
  "setReframe",
  "setCaption",
  "setLayerAudio",
  "setMixer",
  "setTransition",
  "addRenderTarget",
  "removeRenderTarget",
  "addMarker",
  "removeMarker",
  "insert",
  "overwrite",
  "extract",
  "split",
  "join",
  "ripple",
  "roll",
  "slip",
  "slide",
  "disable",
  "lift",
  "insertCuts",
  "extractCuts",
  "insertTopics",
  "fitLayer",
  "trimLayer",
  "markIn",
  "markOut",
  "addSelection",
  "removeSelection",
  "rate",
  "tag",
  "addTopic",
  "removeTopic",
  "deleteWords",
  "cutWords",
  "fixTranscript"
];

/** パラメータの値の種類（`ParamValue.type`）ごとの、`value` の JSON Schema。 */
export const PARAM_VALUE_SCHEMAS: Record<ParamValue["type"], JsonSchema> = {
  "number": {
    "type": "number",
    "format": "double"
  },
  "vec2": {
    "type": "array",
    "items": {
      "type": "number",
      "format": "double"
    },
    "maxItems": 2,
    "minItems": 2
  },
  "vec3": {
    "type": "array",
    "items": {
      "type": "number",
      "format": "double"
    },
    "maxItems": 3,
    "minItems": 3
  },
  "color": {
    "description": "線形、premultiplied の RGBA。各成分は 0..=1 の範囲が基本だが、HDR では 1 を超えてよい。",
    "type": "object",
    "properties": {
      "a": {
        "type": "number",
        "format": "float"
      },
      "b": {
        "type": "number",
        "format": "float"
      },
      "g": {
        "type": "number",
        "format": "float"
      },
      "r": {
        "type": "number",
        "format": "float"
      }
    },
    "required": [
      "r",
      "g",
      "b",
      "a"
    ]
  },
  "bool": {
    "type": "boolean"
  },
  "text": {
    "type": "string"
  },
  "choice": {
    "type": "string"
  },
  "asset_ref": {
    "description": "ID（種類ごとにプロジェクトの中で重複しない文字列）",
    "type": "string"
  },
  "json": true
};

/** レンダーターゲットの JSON Schema。欄が増えても Deliver は入力欄をここから作る。 */
export const RENDER_TARGET_SCHEMA: JsonSchema = {
  "description": "レンダーターゲット。成果物の形式。",
  "type": "object",
  "properties": {
    "codec": {
      "description": "エンコーダーの名前（`h264_nvenc`、`prores_ks`、モックは `sample`）。",
      "type": "string"
    },
    "composition": {
      "description": "ID（種類ごとにプロジェクトの中で重複しない文字列）",
      "type": "string"
    },
    "delivery": {
      "description": "納品の設定。字幕とチャプターの出力と、トゥルーピークの上限。\nラウドネスの目標は `RenderTarget::loudness_lufs`（YouTube は -14、放送は -23）。",
      "type": "object",
      "properties": {
        "burn_in_captions": {
          "description": "字幕を書き出しの絵に描き込む。文字起こしから作った字幕を text ノードのレイヤーにするので、プレビューと同じ描画になる。\n置き場所はショートの `setCaption`（無ければ下部中央）。字幕のファイルや mov_text（`subtitles`）とは独立。",
          "type": "boolean",
          "default": false
        },
        "chapters": {
          "description": "コンポジションのマーカーを MP4 のチャプターにし、YouTube の説明欄用の `<名前>.chapters.txt` を置く。",
          "type": "boolean",
          "default": false
        },
        "subtitles": {
          "description": "字幕の置き方。",
          "oneOf": [
            {
              "description": "字幕を作らない。",
              "type": "string",
              "const": "none"
            },
            {
              "description": "書き出しのファイルの隣に `<名前>.srt` と `<名前>.vtt` を置く。",
              "type": "string",
              "const": "sidecar"
            },
            {
              "description": "MP4 に字幕のトラック（mov_text）として入れる。",
              "type": "string",
              "const": "embedded"
            }
          ],
          "default": "none"
        },
        "true_peak_dbtp": {
          "description": "トゥルーピークの上限（dBTP）。`None` なら [`DEFAULT_TRUE_PEAK_DBTP`]。ラウドネスの正規化のときだけ効く。",
          "type": [
            "number",
            "null"
          ],
          "format": "float",
          "default": null
        }
      },
      "default": {
        "burn_in_captions": false,
        "chapters": false,
        "subtitles": "none",
        "true_peak_dbtp": null
      }
    },
    "fps": {
      "description": "正の有理数 num/den（fps、再生の速さ）",
      "type": "object",
      "properties": {
        "den": {
          "type": "integer",
          "maximum": 4294967295,
          "minimum": 1
        },
        "num": {
          "type": "integer",
          "maximum": 4294967295,
          "minimum": 1
        }
      },
      "required": [
        "num",
        "den"
      ]
    },
    "height": {
      "type": "integer",
      "format": "uint32",
      "minimum": 0
    },
    "id": {
      "description": "ID（種類ごとにプロジェクトの中で重複しない文字列）",
      "type": "string"
    },
    "loudness_lufs": {
      "type": [
        "number",
        "null"
      ],
      "format": "float",
      "default": null
    },
    "output_path": {
      "type": "string"
    },
    "width": {
      "type": "integer",
      "format": "uint32",
      "minimum": 0
    }
  },
  "required": [
    "id",
    "composition",
    "width",
    "height",
    "fps",
    "codec",
    "output_path"
  ]
};
