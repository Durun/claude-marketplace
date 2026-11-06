//! コマンドラインの引数。

use std::path::PathBuf;

/// プロジェクトを指定しなかったときに開くフィクスチャのプロジェクト。作業フォルダーは `movie-maker/` を想定する。
const DEFAULT_PROJECT: &str = "fixtures/projects/stage2/project.json";

/// 自動の再生試験。決まった秒数だけ再生して、数字とスクリーンショットを残して終わる。
#[derive(Debug, Clone, PartialEq)]
pub struct PlayTest {
    /// 再生する秒数（実時間）。
    pub secs: f64,
    /// 結果の JSON の書き出し先。
    pub report: PathBuf,
    /// 再生の途中のウィンドウのスクリーンショット。
    pub screenshot: Option<PathBuf>,
}

/// 起動の引数。
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub project: PathBuf,
    pub composition: Option<String>,
    pub no_audio: bool,
    pub play_test: Option<PlayTest>,
}

const USAGE: &str = "使い方: movie-maker [PROJECT] [--composition ID] [--no-audio]\n  \
     [--play-test SECS --report OUT.json [--screenshot OUT.png]]";

fn value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a String, String> {
    it.next()
        .ok_or_else(|| format!("{flag} には値が要る\n{USAGE}"))
}

/// 引数を読む（先頭のプログラム名は含めない）。
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args {
        project: PathBuf::from(DEFAULT_PROJECT),
        composition: None,
        no_audio: false,
        play_test: None,
    };
    let (mut secs, mut report, mut shot) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--composition" => out.composition = Some(value(&mut it, a)?.clone()),
            "--no-audio" => out.no_audio = true,
            "--play-test" => {
                let v = value(&mut it, a)?;
                secs = Some(
                    v.parse::<f64>()
                        .map_err(|e| format!("--play-test {v}: {e}"))?,
                );
            }
            "--report" => report = Some(PathBuf::from(value(&mut it, a)?)),
            "--screenshot" => shot = Some(PathBuf::from(value(&mut it, a)?)),
            flag if flag.starts_with("--") => return Err(format!("知らない引数 {flag}\n{USAGE}")),
            path => out.project = PathBuf::from(path),
        }
    }
    out.play_test = match (secs, report) {
        (Some(secs), Some(report)) => Some(PlayTest {
            secs,
            report,
            screenshot: shot,
        }),
        (None, None) if shot.is_none() => None,
        _ => return Err(format!("--play-test と --report は組で指定する\n{USAGE}")),
    };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn 引数なしはフィクスチャのプロジェクトを開く() {
        let a = parse(&[]).unwrap();
        assert_eq!(a.project, PathBuf::from(DEFAULT_PROJECT));
        assert!(!a.no_audio && a.play_test.is_none());
    }

    #[test]
    fn 再生試験は秒数と報告の書き出し先が組() {
        let a = parse(&args(&[
            "p.json",
            "--play-test",
            "10",
            "--report",
            "r.json",
            "--screenshot",
            "s.png",
        ]))
        .unwrap();
        let t = a.play_test.unwrap();
        assert_eq!((a.project, t.secs), (PathBuf::from("p.json"), 10.0));
        assert_eq!(t.screenshot, Some(PathBuf::from("s.png")));
        assert!(parse(&args(&["--play-test", "10"])).is_err());
    }

    #[test]
    fn 知らない引数と値の無い引数は拒否する() {
        assert!(parse(&args(&["--nope"])).is_err());
        assert!(parse(&args(&["--composition"])).is_err());
    }
}
