//! `web.cef` の種類。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use mm_gpu::{GpuContext, WgpuGpu};
use mm_host_api::{HostLauncher, SharedFrame};
use mm_node_api::{Cost, FrameCx, FrameRef, GpuTexture, Isolation, NodeKind, Traits};
use mm_types::{MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, RgbaImage};

use crate::cache::{Cached, FrameCache, FrameKey};
use crate::gpu::{copy_into, Converter};
use crate::hosts::{Delivery, HostPages};
use crate::page::{FrameRequest, Page, DATA_PARAM, HTML_PARAM, STATIC_PARAM};
use crate::{mock, KIND_ID, KIND_VERSION};

/// 描き手。
enum Source {
    Host(HostPages),
    Mock,
}

/// 描いた数と、キャッシュから返した数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub drawn: u64,
    pub hits: u64,
}

/// HTML と CSS のページをレイヤーとして描く種類。ページは `window.mmSetTime(t, data)` を定義し、
/// レイヤーの始まりからの秒 `t` と `data` の関数としてだけ画面を作る（同じ入力なら同じ画素）。
///
/// レンダラーが `WgpuGpu`（D3D12）なら共有テクスチャで受けて GPU のまま変換し、それ以外は CPU の画像で受ける。
/// CEF が無い環境ではモック（[`WebCefKind::mock`]）が矩形を描く。
pub struct WebCefKind {
    source: Mutex<Source>,
    cache: Mutex<FrameCache>,
    converter: Mutex<Option<Converter>>,
    device: Mutex<DeviceGeneration>,
    drawn: AtomicU64,
    hits: AtomicU64,
}

/// 最後に共有テクスチャで描いたデバイスと、その世代。デバイスを作り直すと世代が進み、ホストを起こし直す。
#[derive(Default)]
struct DeviceGeneration {
    ctx: Weak<GpuContext>,
    generation: u64,
}

impl DeviceGeneration {
    /// `ctx` の世代。前と違うデバイスなら世代を進める。
    fn of(&mut self, ctx: &Arc<GpuContext>) -> u64 {
        if !self.ctx.upgrade().is_some_and(|c| Arc::ptr_eq(&c, ctx)) {
            self.ctx = Arc::downgrade(ctx);
            self.generation += 1;
        }
        self.generation
    }
}

impl WebCefKind {
    fn with_source(source: Source) -> Self {
        Self {
            source: Mutex::new(source),
            cache: Mutex::new(FrameCache::default()),
            converter: Mutex::new(None),
            device: Mutex::new(DeviceGeneration::default()),
            drawn: AtomicU64::new(0),
            hits: AtomicU64::new(0),
        }
    }

    /// `executable`（`mm-host-cef`）を `launcher` で起こして描く。
    pub fn with_host(launcher: Arc<dyn HostLauncher>, executable: PathBuf) -> Self {
        Self::with_source(Source::Host(HostPages::new(launcher, executable)))
    }

    /// CEF の代わりに矩形を描く。
    pub fn mock() -> Self {
        Self::with_source(Source::Mock)
    }

    /// ホストで描くか（偽ならモック）。
    pub fn uses_host(&self) -> bool {
        self.source().is_ok_and(|s| matches!(*s, Source::Host(_)))
    }

    /// 描いた数とキャッシュから返した数。
    pub fn stats(&self) -> Stats {
        Stats {
            drawn: self.drawn.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
        }
    }

    fn source(&self) -> Result<MutexGuard<'_, Source>> {
        self.source
            .lock()
            .map_err(|_| MmError::Internal("web.cef のホストのロックが壊れた".into()))
    }

    fn cache(&self) -> Result<MutexGuard<'_, FrameCache>> {
        self.cache
            .lock()
            .map_err(|_| MmError::Internal("web.cef のキャッシュが壊れた".into()))
    }

    fn hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    fn drew(&self) {
        self.drawn.fetch_add(1, Ordering::Relaxed);
    }

    /// 共有テクスチャで受けられるか。ホストが D3D12 のテクスチャを渡すので、D3D12 のデバイスが要る。
    fn can_share(&self, ctx: &GpuContext) -> bool {
        cfg!(windows) && ctx.backend() == wgpu::Backend::Dx12 && self.uses_host()
    }

    /// GPU の経路。共有テクスチャを使えなければ `None`。
    fn render_gpu(
        &self,
        gpu: &mut WgpuGpu,
        req: &FrameRequest<'_>,
        key: &FrameKey,
    ) -> Result<Option<FrameRef>> {
        let ctx = gpu.context().clone();
        if !self.can_share(&ctx) {
            return Ok(None);
        }
        let cached = self.cache()?.get(key);
        let texture = match cached {
            Some(Cached::Gpu { texture, ctx: made })
                if made.upgrade().is_some_and(|c| Arc::ptr_eq(&c, &ctx)) =>
            {
                self.hit();
                texture
            }
            _ => {
                let texture = self.draw_shared(&ctx, req)?;
                let value = Cached::Gpu {
                    texture: texture.clone(),
                    ctx: Arc::downgrade(&ctx),
                };
                self.cache()?.put(key.clone(), value);
                texture
            }
        };
        let id = gpu.create(req.size)?;
        let dst = gpu
            .texture(id)
            .ok_or_else(|| MmError::Internal("確保したテクスチャが無い".into()))?;
        copy_into(&ctx, &texture, dst);
        Ok(Some(FrameRef::Gpu(GpuTexture { id, size: req.size })))
    }

    /// ホストに共有テクスチャで描かせ、中間の形式のテクスチャにする。
    fn draw_shared(&self, ctx: &Arc<GpuContext>, req: &FrameRequest<'_>) -> Result<wgpu::Texture> {
        let device = self
            .device
            .lock()
            .map_err(|_| MmError::Internal("web.cef のデバイスの世代のロックが壊れた".into()))?
            .of(ctx);
        let delivery = Delivery::Shared {
            adapter: ctx.adapter.clone(),
            device,
        };
        let frame = match &mut *self.source()? {
            Source::Host(hosts) => hosts.render(req, &delivery)?,
            Source::Mock => {
                return Err(MmError::Internal("モックは共有テクスチャを出さない".into()))
            }
        };
        self.drew();
        let shared = open_shared(ctx, frame, req)?;
        let mut converter = self
            .converter
            .lock()
            .map_err(|_| MmError::Internal("web.cef の変換のロックが壊れた".into()))?;
        let converter = match converter.take() {
            Some(c) if c.is_for(ctx) => converter.insert(c),
            _ => converter.insert(Converter::new(ctx)),
        };
        Ok(converter.convert(ctx, &shared))
    }

    /// CPU の経路。モックか、ホストの CPU の画像。
    fn render_cpu(&self, req: &FrameRequest<'_>, key: FrameKey) -> Result<FrameRef> {
        if let Some(Cached::Cpu(image)) = self.cache()?.get(&key) {
            self.hit();
            return Ok(FrameRef::Cpu(image));
        }
        let image = match &mut *self.source()? {
            Source::Mock => mock::render(req),
            Source::Host(hosts) => cpu_image(hosts.render(req, &Delivery::Cpu)?)?,
        };
        self.drew();
        if image.size() != req.size {
            return Err(MmError::Internal(format!(
                "web.cef のフレームの大きさ {:?} が頼んだ {:?} と違う",
                image.size(),
                req.size
            )));
        }
        let image = Arc::new(image);
        self.cache()?.put(key, Cached::Cpu(image.clone()));
        Ok(FrameRef::Cpu(image))
    }
}

/// CPU の画像の応答を画像にする。共有テクスチャが来たらハンドルを閉じて誤りにする。
fn cpu_image(frame: SharedFrame) -> Result<RgbaImage> {
    match frame {
        SharedFrame::Cpu { size, data } => RgbaImage::from_raw(size, data),
        SharedFrame::SharedTexture { handle, fence, .. } => {
            close_handles(handle, fence.map(|f| f.handle));
            Err(MmError::Internal(
                "CPU の画像を頼んだのに共有テクスチャが来た".into(),
            ))
        }
    }
}

#[cfg(windows)]
fn close_handles(handle: u64, fence: Option<u64>) {
    std::iter::once(handle)
        .chain(fence)
        .for_each(crate::shared::close);
}

#[cfg(not(windows))]
fn close_handles(_handle: u64, _fence: Option<u64>) {}

#[cfg(windows)]
fn open_shared(
    ctx: &GpuContext,
    frame: SharedFrame,
    req: &FrameRequest<'_>,
) -> Result<wgpu::Texture> {
    match frame {
        SharedFrame::SharedTexture {
            handle,
            size,
            fence: None,
        } if size == req.size => crate::shared::open(ctx, handle, size),
        SharedFrame::SharedTexture {
            handle,
            fence,
            size,
        } => {
            close_handles(handle, fence.map(|f| f.handle));
            Err(MmError::Unsupported(format!(
                "共有テクスチャの大きさ {size:?}（頼んだのは {:?}）かフェンスつきの受け渡しは受けられない",
                req.size
            )))
        }
        SharedFrame::Cpu { .. } => Err(MmError::Internal(
            "共有テクスチャを頼んだのに CPU の画像が来た".into(),
        )),
    }
}

#[cfg(not(windows))]
fn open_shared(_: &GpuContext, _: SharedFrame, _: &FrameRequest<'_>) -> Result<wgpu::Texture> {
    Err(MmError::Unsupported("共有テクスチャは Windows だけ".into()))
}

impl NodeKind for WebCefKind {
    fn id(&self) -> &'static str {
        KIND_ID
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![
            ParamSpec::new(HTML_PARAM, ParamType::Text, ParamValue::Text(String::new())),
            ParamSpec::new(
                DATA_PARAM,
                ParamType::Json,
                ParamValue::Json(serde_json::json!({})),
            ),
            ParamSpec::new(STATIC_PARAM, ParamType::Bool, ParamValue::Bool(false)),
        ])
    }

    fn traits(&self) -> Traits {
        Traits {
            realtime: false,
            cost: Cost::Heavy,
            isolation: Isolation::OutOfProcess,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        if from != KIND_VERSION {
            return Err(MmError::Unsupported(format!(
                "{KIND_ID} のバージョン {from} からの移行が無い"
            )));
        }
        let params: Params = serde_json::from_value(old)?;
        self.params().validate(&params)?;
        Ok(params)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let _span = tracing::info_span!("nodes_cef.render").entered();
        let Some(page) = Page::read(cx.params) else {
            return Ok(FrameRef::Transparent { size: cx.size });
        };
        let req = FrameRequest {
            page: &page,
            time: page.time(cx.source_time),
            size: cx.size,
            scale: cx.scale,
        };
        let key = FrameKey::of(&req);
        if let Some(gpu) = cx.gpu.as_any().downcast_mut::<WgpuGpu>() {
            if let Some(frame) = self.render_gpu(gpu, &req, &key)? {
                return Ok(frame);
            }
        }
        self.render_cpu(&req, key)
    }
}

impl std::fmt::Debug for WebCefKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebCefKind")
            .field("host", &self.uses_host())
            .field("stats", &self.stats())
            .finish()
    }
}
