//! cpal の出力ストリーム。ストリームは専用のスレッドが持ち（プラットフォームによって `Send` でない）、
//! 呼び出し側は [`Ring`] を通して音を足し、時計を読む。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample, StreamConfig};
use mm_audio_api::{AudioBlock, AudioSink};
use mm_types::{MmError, Result};

use crate::convert::{remap_channels, Resampler};
use crate::ring::Ring;

struct Shared {
    ring: Mutex<Ring>,
    running: AtomicBool,
    epoch: Instant,
    /// `AudioSink::write` で足したフレーム数。
    written: AtomicU64,
}

/// プレビューの音の出力。ミキサーの出力（`input_rate`、`input_channels`）を受け、デバイスの形に変えて鳴らす。
pub struct AudioOutput {
    shared: Arc<Shared>,
    convert: Mutex<Resampler>,
    input_rate: u32,
    input_channels: u16,
    device_rate: u32,
    device_channels: u16,
    device_name: String,
    quit: Option<Sender<()>>,
    join: Option<JoinHandle<()>>,
}

fn unsupported(what: &str, e: impl std::fmt::Display) -> MmError {
    MmError::Unsupported(format!("音の出力: {what}: {e}"))
}

impl AudioOutput {
    /// 既定の出力デバイスを開いて鳴らし始める（最初は止めた状態）。
    /// デバイスが無いときや形式が合わないときは `Unsupported`。
    pub fn open(input_rate: u32, input_channels: u16) -> Result<Self> {
        let (ready_tx, ready_rx) = channel();
        let (quit_tx, quit_rx) = channel::<()>();
        let epoch = Instant::now();
        let join = std::thread::Builder::new()
            .name("audio-out".into())
            .spawn(move || {
                let started = start_stream(epoch);
                match started {
                    Ok((stream, info)) => {
                        let _ = ready_tx.send(Ok(info));
                        // ストリームはこのスレッドで生かし、終了の合図まで待つ。
                        let _ = quit_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| unsupported("スレッドを作れない", e))?;
        let info = ready_rx
            .recv()
            .map_err(|e| unsupported("開始の通知が来ない", e))??;
        Ok(Self {
            shared: info.shared,
            convert: Mutex::new(Resampler::new(input_rate, info.sample_rate, input_channels)),
            input_rate,
            input_channels,
            device_rate: info.sample_rate,
            device_channels: info.channels,
            device_name: info.name,
            quit: Some(quit_tx),
            join: Some(join),
        })
    }

    /// デバイスの名前、サンプルレート、チャンネル数。
    pub fn device(&self) -> (&str, u32, u16) {
        (&self.device_name, self.device_rate, self.device_channels)
    }

    /// 鳴らしている間は真。偽の間は溜めた音を消費せず、時計も進まない。
    pub fn set_running(&self, running: bool) {
        self.shared.running.store(running, Ordering::SeqCst);
    }

    /// 溜めた音を捨て、再生位置を `start_secs` に置き直す。
    pub fn reset(&self, start_secs: f64) {
        self.shared.ring.lock().expect("ring").reset(start_secs);
        *self.convert.lock().expect("convert") =
            Resampler::new(self.input_rate, self.device_rate, self.input_channels);
    }

    /// ミキサーの出力を足す。形が違えば `InvalidArgument`。
    pub fn push(&self, block: &AudioBlock) -> Result<()> {
        if block.sample_rate != self.input_rate || block.channels != self.input_channels {
            return Err(MmError::InvalidArgument(format!(
                "音の出力は {} Hz {} ch を受ける: {} Hz {} ch が来た",
                self.input_rate, self.input_channels, block.sample_rate, block.channels
            )));
        }
        let resampled = self
            .convert
            .lock()
            .expect("convert")
            .process(&block.samples);
        let device = remap_channels(&resampled, self.input_channels, self.device_channels);
        self.shared.ring.lock().expect("ring").push(&device);
        self.shared
            .written
            .fetch_add(block.frames() as u64, Ordering::SeqCst);
        Ok(())
    }

    /// 溜まっているフレーム数（デバイスのレートで数える）。
    pub fn buffered_frames(&self) -> usize {
        self.shared.ring.lock().expect("ring").buffered_frames()
    }

    /// 溜まっている音の長さ（秒）。
    pub fn buffered_secs(&self) -> f64 {
        self.buffered_frames() as f64 / f64::from(self.device_rate)
    }

    /// いま耳に届いている再生位置（秒）。再生の主の時計。
    pub fn position_secs(&self) -> f64 {
        let now = self.shared.epoch.elapsed().as_secs_f64();
        self.shared.ring.lock().expect("ring").position(now)
    }

    /// 鳴らすものが無くて無音を出した回数。
    pub fn underruns(&self) -> u64 {
        self.shared.ring.lock().expect("ring").underruns()
    }
}

impl AudioSink for AudioOutput {
    fn sample_rate(&self) -> u32 {
        self.input_rate
    }

    fn channels(&self) -> u16 {
        self.input_channels
    }

    fn write(&mut self, block: &AudioBlock) -> Result<()> {
        self.push(block)
    }

    fn finish(&mut self) -> Result<u64> {
        Ok(self.shared.written.load(Ordering::SeqCst))
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        drop(self.quit.take());
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct Started {
    shared: Arc<Shared>,
    sample_rate: u32,
    channels: u16,
    name: String,
}

fn start_stream(epoch: Instant) -> Result<(cpal::Stream, Started)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| MmError::Unsupported("音の出力: 出力デバイスが無い".into()))?;
    let supported = device
        .default_output_config()
        .map_err(|e| unsupported("既定の形式を読めない", e))?;
    let config: StreamConfig = supported.config();
    let shared = Arc::new(Shared {
        ring: Mutex::new(Ring::new(config.sample_rate, config.channels)),
        running: AtomicBool::new(false),
        epoch,
        written: AtomicU64::new(0),
    });
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, &shared),
        SampleFormat::I16 => build::<i16>(&device, config, &shared),
        SampleFormat::U16 => build::<u16>(&device, config, &shared),
        SampleFormat::I32 => build::<i32>(&device, config, &shared),
        other => Err(MmError::Unsupported(format!(
            "音の出力: サンプル形式 {other} は未対応"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| unsupported("ストリームを始められない", e))?;
    let name = device
        .id()
        .map(|i| i.to_string())
        .unwrap_or_else(|_| "default".into());
    Ok((
        stream,
        Started {
            shared,
            sample_rate: config.sample_rate,
            channels: config.channels,
            name,
        },
    ))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let s = shared.clone();
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &OutputCallbackInfo| {
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(ts.callback).as_secs_f64();
                let plays_at = s.epoch.elapsed().as_secs_f64() + latency;
                scratch.resize(data.len(), 0.0);
                if let Ok(mut ring) = s.ring.lock() {
                    ring.fill(&mut scratch, plays_at, s.running.load(Ordering::SeqCst));
                } else {
                    scratch.fill(0.0);
                }
                for (d, v) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(*v);
                }
            },
            |e| tracing::warn!(error = %e, "音の出力のストリームの警告"),
            None,
        )
        .map_err(|e| unsupported("ストリームを作れない", e))
}
