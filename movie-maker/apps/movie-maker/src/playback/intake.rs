//! 再生スレッドに入るものを整える。届いたコマンドのまとめ方と、音の出力に足すチャンクの形。

use std::sync::mpsc::Receiver;

use mm_audio_api::{AudioBlock, DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};

use super::Command;

/// 受けたコマンドに、すでに届いているコマンドを続けて取り出す。連続するシークは最後の 1 つにまとめる。
pub(super) fn collect_batch(first: Command, rx: &Receiver<Command>) -> Vec<Command> {
    let mut batch = vec![first];
    batch.extend(rx.try_iter());
    coalesce_seeks(batch)
}

/// 連続するシークを最後の 1 つにする。スクラブで積まれた古い位置を描いて、プレビューが遅れるのを防ぐ。
pub fn coalesce_seeks(commands: Vec<Command>) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::with_capacity(commands.len());
    for cmd in commands {
        match out.last_mut() {
            Some(last @ Command::Seek(_)) if matches!(cmd, Command::Seek(_)) => *last = cmd,
            _ => out.push(cmd),
        }
    }
    out
}

/// 音の出力が受ける形（レート、チャンネル数、長さ）でなければ、同じ長さの無音にする。
pub(super) fn fit_chunk(block: AudioBlock, frames: usize) -> AudioBlock {
    let fits = block.sample_rate == DEFAULT_SAMPLE_RATE
        && block.channels == DEFAULT_CHANNELS
        && block.frames() == frames;
    if fits {
        block
    } else {
        AudioBlock::silence(DEFAULT_SAMPLE_RATE, DEFAULT_CHANNELS, frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 連続するシークは最後の1つにまとまり_間のコマンドは順序を保つ() {
        let seeks: Vec<Command> = (0..1000).map(|i| Command::Seek(f64::from(i))).collect();
        assert_eq!(coalesce_seeks(seeks), vec![Command::Seek(999.0)]);
        let mixed = vec![
            Command::Seek(1.0),
            Command::Seek(2.0),
            Command::Step(1),
            Command::Seek(3.0),
            Command::Seek(4.0),
            Command::Play,
        ];
        assert_eq!(
            coalesce_seeks(mixed),
            vec![
                Command::Seek(2.0),
                Command::Step(1),
                Command::Seek(4.0),
                Command::Play
            ]
        );
    }

    #[test]
    fn 形の違う音のチャンクは同じ長さの無音にして時計を止めない() {
        let rate = DEFAULT_SAMPLE_RATE;
        let wrong_rate = AudioBlock::silence(44_100, DEFAULT_CHANNELS, 100);
        let fixed = fit_chunk(wrong_rate, 100);
        assert_eq!(
            (fixed.sample_rate, fixed.channels, fixed.frames()),
            (rate, DEFAULT_CHANNELS, 100)
        );
        let short = AudioBlock::silence(rate, DEFAULT_CHANNELS, 40);
        assert_eq!(fit_chunk(short, 100).frames(), 100);
        let ok = AudioBlock::silence(rate, DEFAULT_CHANNELS, 100);
        assert_eq!(fit_chunk(ok.clone(), 100), ok);
    }
}
