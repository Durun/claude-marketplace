//! CEF の共有テクスチャを、ホストが持つテクスチャへコピーする所と、それを別プロセスへ渡す所。
//!
//! CEF の NT ハンドルはコールバックの間だけ有効で、テクスチャは CEF が次のフレームに使い回す。
//! そのためコールバックの中でコピーを終える。別プロセスへ渡すときは、コピー先を D3D12 の共有ヒープに作り、
//! その NT ハンドルを相手のプロセスへ `DuplicateHandle` する（Chromium の GPU プロセスがブラウザへ渡すのと同じ型）。
//! フレームごとに新しいテクスチャを作るので、渡したテクスチャをホストが書き換えることは無い。

use cef::AcceleratedPaintInfo;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_CLOSE_SOURCE, GENERIC_ALL, HANDLE,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::DXGI_SHARED_RESOURCE_READ;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE};

type Dx12 = wgpu::hal::api::Dx12;

/// フレームの形式。CEF の `OnAcceleratedPaint` は BGRA8（premultiplied、sRGB の符号化）。
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// ホストが持つ 1 フレーム。
pub struct FrameTexture {
    pub texture: wgpu::Texture,
    /// 共有ヒープに作ったときの D3D12 の資源。NT ハンドルを作るのに使う。
    pub shared: Option<ID3D12Resource>,
}

fn d3d_err(what: &str, e: windows::core::Error) -> String {
    format!("{what}: {e}")
}

fn descriptor(
    size: wgpu::Extent3d,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("mm-host-cef frame"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: &[],
    }
}

/// D3D12 の資源を wgpu のテクスチャにする。共有の資源は COMMON（wgpu の PRESENT）で出入りする。
///
/// # Safety
/// `resource` は `device` と同じアダプターの、`size` の BGRA8 の 2D テクスチャ。
unsafe fn wrap(
    device: &wgpu::Device,
    resource: ID3D12Resource,
    size: wgpu::Extent3d,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    let desc = descriptor(size, usage);
    // SAFETY: 呼ぶ側の約束どおり、形式と大きさは desc と一致する。
    unsafe {
        let hal = wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            FORMAT,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        );
        device.create_texture_from_hal::<Dx12>(hal, &desc, wgpu::TextureUses::PRESENT)
    }
}

fn raw_device(device: &wgpu::Device) -> Result<ID3D12Device, String> {
    // SAFETY: as_hal の参照はこの関数の間だけ使い、COM の参照を複製して返す。
    unsafe { device.as_hal::<Dx12>() }
        .map(|hal| hal.raw_device().clone())
        .ok_or_else(|| "wgpu が D3D12 ではない".to_string())
}

/// CEF の NT ハンドルを開く。
///
/// # Safety
/// `handle` は `OnAcceleratedPaint` の中の、まだ有効なハンドル。
unsafe fn open_cef(
    device: &wgpu::Device,
    handle: HANDLE,
    size: wgpu::Extent3d,
) -> Result<wgpu::Texture, String> {
    let raw = raw_device(device)?;
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: handle は有効。CEF がハンドルを持ち続けるので、ここでは閉じない。
    unsafe { raw.OpenSharedHandle(handle, &mut resource) }.map_err(|e| {
        d3d_err(
            "CEF の共有テクスチャを開けない（CEF と wgpu の GPU が違うと E_INVALIDARG）",
            e,
        )
    })?;
    let resource = resource.ok_or("共有ハンドルから資源が出てこない")?;
    // SAFETY: 読むだけ。
    let desc = unsafe { resource.GetDesc() };
    if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        || desc.Width != u64::from(size.width)
        || desc.Height != size.height
    {
        return Err(format!(
            "CEF のテクスチャが想定と違う: {:?} {}x{}",
            desc.Format, desc.Width, desc.Height
        ));
    }
    // SAFETY: 形式と大きさは上で確かめた。
    Ok(unsafe { wrap(device, resource, size, wgpu::TextureUsages::COPY_SRC) })
}

/// 共有ヒープに BGRA8 のテクスチャを作る。
fn create_shared(device: &wgpu::Device, size: wgpu::Extent3d) -> Result<FrameTexture, String> {
    let raw = raw_device(device)?;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: u64::from(size.width),
        Height: size.height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_NONE,
    };
    let mut resource: Option<ID3D12Resource> = None;
    // SAFETY: 有効なデバイスに、記述どおりの資源を作る。
    unsafe {
        raw.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    }
    .map_err(|e| d3d_err("共有テクスチャを作れない", e))?;
    let resource = resource.ok_or("共有テクスチャが空")?;
    let usage = wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC;
    // SAFETY: 上の記述で作った資源。
    let texture = unsafe { wrap(device, resource.clone(), size, usage) };
    Ok(FrameTexture {
        texture,
        shared: Some(resource),
    })
}

fn create_owned(device: &wgpu::Device, size: wgpu::Extent3d) -> FrameTexture {
    let usage = wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC
        | wgpu::TextureUsages::TEXTURE_BINDING;
    FrameTexture {
        texture: device.create_texture(&descriptor(size, usage)),
        shared: None,
    }
}

/// `OnAcceleratedPaint` のフレームを、ホストが持つテクスチャへコピーして完了を待つ。
/// `share` なら共有ヒープに作る。
///
/// # Safety
/// `OnAcceleratedPaint` の中で呼ぶ。`device` は CEF の GPU プロセスと同じアダプターの D3D12。
pub unsafe fn import(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    info: &AcceleratedPaintInfo,
    share: bool,
) -> Result<FrameTexture, String> {
    let size = wgpu::Extent3d {
        width: info.extra.coded_size.width as u32,
        height: info.extra.coded_size.height as u32,
        depth_or_array_layers: 1,
    };
    let handle = HANDLE(info.shared_texture_handle);
    // SAFETY: 呼ぶ側の約束どおりコールバックの中。
    let source = unsafe { open_cef(device, handle, size)? };
    let target = if share {
        create_shared(device, size)?
    } else {
        create_owned(device, size)
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_texture(source.as_image_copy(), target.texture.as_image_copy(), size);
    // 共有の資源は COMMON に戻して渡す（CEF へ返す方も、別プロセスへ渡す方も）。
    let mut back = vec![&source];
    if share {
        back.push(&target.texture);
    }
    encoder.transition_resources(
        std::iter::empty(),
        back.into_iter().map(|texture| wgpu::TextureTransition {
            texture,
            selector: None,
            state: wgpu::TextureUses::PRESENT,
        }),
    );
    let submission = queue.submit([encoder.finish()]);
    // CEF はコールバックから戻るとテクスチャを使い回すので、コピーの完了まで待つ。
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })
        .map_err(|e| format!("コピーの完了待ちに失敗: {e}"))?;
    Ok(target)
}

/// 相手のプロセスへ渡すハンドルの権限。相手はコピーの元に読むだけなので、読み取りだけを許す。
/// D3D12 の `CreateSharedHandle` は `GENERIC_ALL` しか受けないので、絞るのは複製のとき。
const REMOTE_ACCESS: u32 = DXGI_SHARED_RESOURCE_READ.0;

/// 共有ハンドルを渡す相手のプロセス。
pub struct Client(HANDLE);

impl Client {
    /// `pid` のプロセスを、ハンドルの複製の権限だけで開く。
    pub fn open(pid: u32) -> Result<Self, String> {
        // SAFETY: 失敗は Err で返る。開いたハンドルは Drop で閉じる。
        unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, pid) }
            .map(Self)
            .map_err(|e| d3d_err(&format!("プロセス {pid} を開けない"), e))
    }

    /// `resource` の NT ハンドルを作り、相手のプロセスへ移す。返す値は相手のプロセスで有効。
    pub fn export(&self, device: &wgpu::Device, resource: &ID3D12Resource) -> Result<u64, String> {
        let raw = raw_device(device)?;
        // SAFETY: resource は raw で作った共有ヒープの資源。
        let local =
            unsafe { raw.CreateSharedHandle(resource, None, GENERIC_ALL.0, PCWSTR::null()) }
                .map_err(|e| d3d_err("共有ハンドルを作れない", e))?;
        let mut remote = HANDLE::default();
        // SAFETY: local はこのプロセスのハンドル。DUPLICATE_CLOSE_SOURCE で成否によらず閉じる。
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                local,
                self.0,
                &mut remote,
                REMOTE_ACCESS,
                false,
                DUPLICATE_CLOSE_SOURCE,
            )
        }
        .map_err(|e| d3d_err("ハンドルを相手のプロセスへ複製できない", e))?;
        Ok(remote.0 as usize as u64)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: OpenProcess で開いたハンドル。
        let _ = unsafe { CloseHandle(self.0) };
    }
}
