//! マクロとテンプレート（`macros/*.json`、`templates/*.json`）。引数を受けて操作の列を作る手順。
//!
//! 手順の中の文字列は `{{名前}}` で引数を引く。文字列の全体が `{{…}}` のときは型どおりの値（時刻は `{num, den}`、
//! 数は数、無い任意の引数は `null`）に置き換わり、一部のときは文字として埋まる。`{{at + dur}}` のように
//! 時刻の引数と秒の数を `+` と `-` でつなげる。`canvas.width` などの theme の値も引ける（[`crate::args`]）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// マクロかテンプレートか。置いたフォルダーで決まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroKind {
    /// `macros/`。既にあるコンポジションに足す。
    #[default]
    Macro,
    /// `templates/`。空のプロジェクトに適用するテンプレート。
    Template,
}

/// マクロ 1 つ。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroDef {
    /// 呼ぶときの名前（ファイル名と同じ）。
    pub name: String,
    /// 画面と記録に出す名前。
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// 引数の名前 → 型と既定。
    #[serde(default)]
    pub params: BTreeMap<String, ParamDef>,
    /// 検査で使う引数の例。検査はこの例で展開して、空のプロジェクトにテンプレートを適用した上に適用してみる。
    #[serde(default)]
    pub example: BTreeMap<String, String>,
    /// 順に展開する手順。
    pub steps: Vec<Step>,
    #[serde(skip)]
    pub kind: MacroKind,
}

/// 画面と一覧に出す、マクロの名前と引数（手順と例を除く）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MacroSummary {
    pub name: String,
    pub kind: MacroKind,
    pub title: String,
    pub description: String,
    pub params: BTreeMap<String, ParamDef>,
}

impl MacroDef {
    /// 一覧に出す形。
    pub fn summary(&self) -> MacroSummary {
        MacroSummary {
            name: self.name.clone(),
            kind: self.kind,
            title: self.title.clone(),
            description: self.description.clone(),
            params: self.params.clone(),
        }
    }
}

/// 引数 1 つの型と既定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamDef {
    #[serde(rename = "type")]
    pub ty: ParamType,
    /// 省いたときの値（引数と同じ文字の形）。
    #[serde(default)]
    pub default: Option<String>,
    /// 省いてよい（既定も無いときは `null` か空になる）。
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub description: String,
}

/// 引数の型。引数は全部文字で受け、この型で読む。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    /// 文。
    Text,
    /// プロジェクトの中の ID（レイヤー、カット、コンポジション）。
    Id,
    /// 秒（`1.5`、`3/2`）。
    Time,
    /// 数。
    Number,
}

/// 手順 1 つ。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// 操作 1 つ（`mm op --schema` の形）。引数を埋めてからそのまま使う。
    Op(serde_json::Value),
    /// 文字のスタイルで文字を置く。プレートと文字のレイヤー、出る・消える動き、カットへの接続を作る。
    Text(TextStep),
    /// パックの `mixer.json` を、コンポジションのミキサーの設定として `setMixer` で当てる。
    Mixer(MixerStep),
    /// パックの `transitions/<name>.wgsl` を、カットの頭のトランジションとして置く。
    Transition(TransitionStep),
    /// パックの `web/<page>.html` を `web.cef` のレイヤーとして置く。
    Web(WebStep),
}

/// パックの WGSL のトランジションをカットの頭に置く手順。文字列は `{{…}}` を埋めてから読む。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionStep {
    /// `transitions/` の中のトランジションの名前（拡張子を除く）。
    pub name: String,
    /// トランジションを置くカット（前のカットとの境に掛かる）。
    pub cut: String,
    /// トランジションの長さ。
    pub dur: String,
    /// `transition.wgsl` の数のパラメータ（`p0`..`p7`、`rewind`）。値は数の文字。
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// HTML の部品を `web.cef` のレイヤーとして置く手順。文字列は `{{…}}` を埋めてから読む。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebStep {
    /// `web/` の中のページの名前（拡張子を除く）。
    pub page: String,
    /// 置くコンポジション。
    pub comp: String,
    /// レイヤーの ID。
    pub layer: String,
    /// コンポジションの時間軸での出る時刻。
    pub at: String,
    /// 出ている長さ。
    pub dur: String,
    /// 接続するカット。空なら接続しない。
    #[serde(default)]
    pub attach: Option<String>,
    /// ページの `mmSetTime(t, data)` に渡す値。展開でテーマ（色、書体、画面）を `theme` に足す。
    #[serde(default)]
    pub data: serde_json::Value,
    /// 出る動き（不透明度、秒）。テーマの `motion.in_max` まで。
    #[serde(default)]
    pub fade_in: f64,
    /// 消える動き（不透明度、秒）。テーマの `motion.out_max` まで。
    #[serde(default)]
    pub fade_out: f64,
    /// ページ自身が時刻で動くか。偽ならパラメータの組ごとに 1 回だけ描く（動きは不透明度で付ける）。
    #[serde(default)]
    pub animated: bool,
}

/// パックのミキサーの設定を当てる手順。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MixerStep {
    /// 当てるコンポジション。`{{…}}` を埋めてから読む。
    pub comp: String,
}

/// 文字のスタイルで文字を置く手順。値は全部 `{{…}}` を埋めてから読む。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStep {
    /// `styles.json` のスタイルの名前。
    pub style: String,
    /// 置くコンポジション。
    pub comp: String,
    /// 文字のレイヤーの ID。プレートは `<ID>.plate`。
    pub layer: String,
    pub text: String,
    /// コンポジションの時間軸での出る時刻。
    pub at: String,
    /// 出ている長さ。
    pub dur: String,
    /// 接続するカット。空なら接続しない。
    #[serde(default)]
    pub attach: Option<String>,
}

impl Step {
    /// 手順の中の文字列の全部（`{{…}}` を探す検査が使う）。
    pub fn strings(&self) -> Vec<&str> {
        match self {
            Step::Op(value) => {
                let mut out = Vec::new();
                collect_strings(value, &mut out);
                out
            }
            Step::Text(t) => {
                let mut out = vec![t.style.as_str(), &t.comp, &t.layer, &t.text, &t.at, &t.dur];
                out.extend(t.attach.as_deref());
                out
            }
            Step::Mixer(m) => vec![m.comp.as_str()],
            Step::Transition(t) => {
                let mut out = vec![t.name.as_str(), &t.cut, &t.dur];
                out.extend(t.params.values().map(String::as_str));
                out
            }
            Step::Web(w) => {
                let mut out = vec![w.page.as_str(), &w.comp, &w.layer, &w.at, &w.dur];
                out.extend(w.attach.as_deref());
                collect_strings(&w.data, &mut out);
                out
            }
        }
    }
}

fn collect_strings<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_strings(v, out)),
        serde_json::Value::Object(map) => map.values().for_each(|v| collect_strings(v, out)),
        _ => {}
    }
}
