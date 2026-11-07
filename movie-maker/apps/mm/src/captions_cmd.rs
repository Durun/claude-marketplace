//! `mm subtitles`: sequence の字幕を SRT か WebVTT で出す。`mm render` の字幕に渡す単語を DB から読む。

use std::path::{Path, PathBuf};

use mm_doc::Project;
use mm_export::{CaptionWord, TranscriptWords};
use mm_store::Store;
use mm_types::{Id, MmError, Result};

/// `mm subtitles`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// sequence の字幕（文字起こしから。消した単語を除く）を SRT か WebVTT で出す。
    Subtitles(SubtitlesArgs),
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Subtitles(args) => subtitles(&args),
        }
    }
}

/// `mm subtitles` の引数。
#[derive(clap::Args)]
pub struct SubtitlesArgs {
    /// `project.mmdb` のあるフォルダー（文字起こしは DB にある）。
    pub project: PathBuf,
    /// 字幕にする sequence の ID。時刻は sequence の時間軸。
    #[arg(long)]
    pub sequence: String,
    /// 字幕の形式。
    #[arg(long, value_enum, default_value_t = Format::Srt)]
    pub format: Format,
    /// 出力先。省くと標準出力。
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// 字幕の形式。
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Srt,
    Vtt,
}

/// 素材ごとの字幕にする単語。消した単語は除き、直した文字を使う。
/// 文字起こしは `project.mmdb` にしか無いので、フォルダーでなければ拒否する（黙って字幕なしにしない）。
pub fn transcript_words(path: &Path, project: &Project) -> Result<TranscriptWords> {
    let mut out = TranscriptWords::new();
    if !path.is_dir() {
        return Err(MmError::InvalidArgument(format!(
            "字幕を作るには文字起こしの DB（project.mmdb のあるフォルダー）が要るが、{} はフォルダーでない",
            path.display()
        )));
    }
    let store = Store::open_read_only(path)?;
    for asset in &project.assets {
        let words: Vec<CaptionWord> = store
            .transcript_words(&asset.id)?
            .into_iter()
            .filter_map(|w| {
                let text = w.shown()?.to_owned();
                Some(CaptionWord {
                    start: w.start,
                    end: w.end,
                    text,
                })
            })
            .collect();
        if !words.is_empty() {
            out.insert(asset.id.clone(), words);
        }
    }
    Ok(out)
}

/// 字幕を書く。
pub fn subtitles(args: &SubtitlesArgs) -> Result<()> {
    let project = mm_store::load_project(&args.project)?;
    let words = transcript_words(&args.project, &project)?;
    let seq = project.sequence(&Id::new(args.sequence.as_str()))?;
    let cues = mm_export::sequence_cues(seq, &words)?;
    let text = match args.format {
        Format::Srt => mm_export::srt(&cues),
        Format::Vtt => mm_export::vtt(&cues),
    };
    match &args.output {
        Some(path) => std::fs::write(path, text)?,
        None => print!("{text}"),
    }
    if cues.is_empty() {
        eprintln!("字幕にできる文字起こしが無い（mm ingest で文字起こしを入れる）");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文字起こしの_db_の無いプロジェクトは字幕の単語を読めず拒否する() {
        let json = std::env::temp_dir().join("mm_captions_cmd_no_db.json");
        let err = transcript_words(&json, &Project::new()).unwrap_err();
        assert!(matches!(err, MmError::InvalidArgument(_)), "{err}");
    }
}
