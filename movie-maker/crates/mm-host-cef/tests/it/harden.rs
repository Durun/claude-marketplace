//! ホストの守りと決定性の検査（実際の CEF）。遷移と外への要求の拒否、Web フォントの読み込み待ち、
//! 遅いページで前のフレームを返さないこと、kill の後の片付け、渡すハンドルの権限。
#![cfg(all(feature = "cef", windows))]

use std::net::TcpListener;
use std::time::Duration;

use mm_host_api::{HostProcess, SharedFrame};
use mm_host_cef::PipeHost;
use mm_types::{RationalTime, Size};

use crate::serve::{gpu, host_exe, marker, prepare, read_shared, render, spec, to_image};

/// 外への要求を試すページ。左はファイルの赤い画像、右は `data:` の緑の画像。
/// 時刻 1 で、ネットワークへの要求と別のページへの遷移を試みる。
const LEAK_PAGE: &str = r#"<!doctype html><html><head><style>
html, body { margin: 0; background: transparent; }
img { position: absolute; top: 0; width: 32px; height: 32px; }
</style></head><body>
<img alt="" src="FILE_URL" style="left: 0">
<img alt="" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='4' height='4'%3E%3Crect width='4' height='4' fill='%2300ff00'/%3E%3C/svg%3E" style="left: 32px">
<script>
window.mmSetTime = (t) => {
  if (t !== 1) { return; }
  fetch('http://127.0.0.1:PORT/fetch').catch(() => {});
  new Image().src = 'http://127.0.0.1:PORT/img';
  const s = document.createElement('script'); s.src = 'http://127.0.0.1:PORT/js'; document.head.appendChild(s);
  location.href = 'http://127.0.0.1:PORT/nav';
};
</script></body></html>"#;

const LEAK_SIZE: Size = Size::new(64, 32);

#[test]
fn ページの遷移と外への要求を拒否し_data_の画像は描く() {
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    let listener = TcpListener::bind("127.0.0.1:0").expect("待ち受けられる");
    listener.set_nonblocking(true).expect("非同期にできる");
    let port = listener.local_addr().expect("ポート").port();
    let red = std::env::temp_dir().join(format!("mm-host-cef-red-{}.svg", std::process::id()));
    std::fs::write(
        &red,
        "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4' fill='red'/></svg>",
    )
    .expect("画像を書ける");
    let file_url = format!("file:///{}", red.to_string_lossy().replace('\\', "/"));
    let page = LEAK_PAGE
        .replace("FILE_URL", &file_url)
        .replace("PORT", &port.to_string());
    let mut host = PipeHost::start(spec(&exe, None)).expect("起動");
    prepare(&mut host, &page);
    for n in 0..4 {
        let t = RationalTime::seconds(n);
        render(&mut host, t, LEAK_SIZE, serde_json::json!({}));
        std::thread::sleep(Duration::from_millis(100));
    }
    let frame = render(
        &mut host,
        RationalTime::seconds(5),
        LEAK_SIZE,
        serde_json::json!({}),
    );
    let image = to_image(&frame, &ctx);
    let _ = std::fs::remove_file(&red);
    let left = image.pixel(16, 16);
    // 読み込めなかった画像は Chromium の壊れた画像のアイコン（緑と灰色）になる。
    assert!(left.r < 0.5, "ファイルの赤い画像は読み込まない: {left:?}");
    let green = image.pixel(48, 16);
    assert!(
        green.a > 0.99 && green.g > 0.9,
        "data: の画像は描く: {green:?}"
    );
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("ページがネットワークに接続した: {other:?}"),
    }
}

/// 標準の Base64（パディングつき）。
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            let c = if i <= chunk.len() {
                TABLE[(n >> (18 - 6 * i) & 0x3f) as usize] as char
            } else {
                '='
            };
            out.push(c);
        }
    }
    out
}

/// 埋め込む書体の候補。読み込みに時間がかかるよう、大きい（10 MB 前後の）日本語の書体を使う。
const FONT_FILES: [&str; 2] = [
    "C:/Windows/Fonts/NotoSerifJP-VF.ttf",
    "C:/Windows/Fonts/yumin.ttf",
];

/// 時刻 1 で初めて Web フォント（`data:` で埋め込んだ書体）を使うページ。
fn font_page() -> Option<String> {
    let font = FONT_FILES.iter().find_map(|p| std::fs::read(p).ok())?;
    Some(format!(
        r#"<!doctype html><html><head><style>
@font-face {{ font-family: MMProbe; src: url(data:font/ttf;base64,{}); }}
html, body {{ margin: 0; background: transparent; }}
div {{ font: 40px MMProbe, monospace; color: white; }}
</style></head><body><script>
window.mmSetTime = (t) => {{ document.body.innerHTML = t >= 1 ? '<div>永和九年 WMWM</div>' : ''; }};
</script></body></html>"#,
        base64(&font)
    ))
}

#[test]
fn 初めて使う_web_フォントは読み込みを待ってから描く() {
    let Some(exe) = host_exe() else { return };
    let Some(page) = font_page() else {
        eprintln!("{FONT_FILES:?} が無いのでスキップする");
        return;
    };
    let ctx = gpu();
    let size = Size::new(320, 64);
    let mut host = PipeHost::start(spec(&exe, None)).expect("起動");
    prepare(&mut host, &page);
    let first = to_image(
        &render(
            &mut host,
            RationalTime::seconds(1),
            size,
            serde_json::json!({}),
        ),
        &ctx,
    );
    let again = to_image(
        &render(
            &mut host,
            RationalTime::seconds(1),
            size,
            serde_json::json!({}),
        ),
        &ctx,
    );
    assert!(first.data().iter().any(|v| *v > 0.0), "文字を描いている");
    assert_eq!(first, again, "最初のフレームも読み込んだ書体で描く");
}

/// 描くたびに、BeginFrame の後の requestAnimationFrame で 150 ms かかり、`seq` を左上の画素に書くページ。
const SLOW_PAGE: &str = r#"<!doctype html><html><head><style>
html, body { margin: 0; width: 100%; height: 100%; background: rgb(0, 0, 0); }
</style></head><body><script>
window.mmSetTime = (t, p) => {
  requestAnimationFrame(() => {
    const end = performance.now() + 150;
    while (performance.now() < end) {}
    document.body.style.background = `rgb(${p.seq & 255}, ${p.seq >> 8}, 0)`;
  });
};
</script></body></html>"#;

#[test]
fn 描画の遅いページでも前のフレームを返さずに待つ() {
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    let size = Size::new(64, 36);
    let mut host = PipeHost::start(spec(&exe, Some(std::process::id()))).expect("起動");
    prepare(&mut host, SLOW_PAGE);
    for seq in 1..=4u64 {
        let t = RationalTime::new(i128::from(seq), 30).unwrap();
        let frame = render(&mut host, t, size, serde_json::json!({ "seq": seq }));
        let SharedFrame::SharedTexture { handle, .. } = frame else {
            panic!("共有テクスチャのはず");
        };
        assert_eq!(marker(&read_shared(&ctx, handle, size), size.width), seq);
    }
}

#[test]
fn kill_したホストの一時フォルダーを消す() {
    let Some(exe) = host_exe() else { return };
    let mut host = PipeHost::start(spec(&exe, None)).expect("起動");
    prepare(&mut host, "<!doctype html><p>x</p>");
    render(
        &mut host,
        RationalTime::ZERO,
        Size::new(32, 16),
        serde_json::json!({}),
    );
    let dir = host.work_dir().expect("動いている").to_path_buf();
    assert!(
        std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some()),
        "キャッシュとページを置いている"
    );
    host.kill().expect("kill");
    assert!(!dir.exists(), "{} が残っている", dir.display());
    assert!(!crate::locate::work_dirs().contains(&dir));
}

/// ハンドルに許された権限（`NtQueryObject` の `GrantedAccess`）。
fn granted_access(handle: u64) -> u32 {
    use windows::Wdk::Foundation::{NtQueryObject, ObjectBasicInformation};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::WindowsProgramming::PUBLIC_OBJECT_BASIC_INFORMATION;
    let mut info = PUBLIC_OBJECT_BASIC_INFORMATION::default();
    // SAFETY: info は記述どおりの大きさの構造体。handle はホストがこのプロセスへ複製したもの。
    let status = unsafe {
        NtQueryObject(
            Some(HANDLE(handle as usize as *mut _)),
            ObjectBasicInformation,
            Some(std::ptr::from_mut(&mut info).cast()),
            std::mem::size_of_val(&info) as u32,
            None,
        )
    };
    assert!(status.is_ok(), "NtQueryObject: {status:?}");
    info.GrantedAccess
}

#[test]
fn 渡す共有ハンドルは読み取りの権限だけ() {
    // READ_CONTROL と SYNCHRONIZE。GENERIC_READ を共有の資源の権限に直した値はこの中に収まる。
    const READ_ONLY: u32 = 0x0002_0000 | 0x0010_0000;
    let Some(exe) = host_exe() else { return };
    let ctx = gpu();
    let size = Size::new(32, 16);
    let mut host = PipeHost::start(spec(&exe, Some(std::process::id()))).expect("起動");
    prepare(&mut host, "<!doctype html><p>x</p>");
    let frame = render(&mut host, RationalTime::ZERO, size, serde_json::json!({}));
    let SharedFrame::SharedTexture { handle, .. } = frame else {
        panic!("共有テクスチャのはず");
    };
    let access = granted_access(handle);
    read_shared(&ctx, handle, size);
    assert_eq!(
        access & !READ_ONLY,
        0,
        "読み取りの標準の権限だけ: {access:#x}"
    );
}

/// `bundle.sh` で組み立てた、sandbox つきで動くホストを指す環境変数。無ければその検査をスキップする。
const SANDBOX_HOST_ENV: &str = "MM_CEF_SANDBOX_HOST";

#[test]
fn sandbox_つきのホストも同じ画素で描く() {
    let Some(exe) = host_exe() else { return };
    let Some(sandboxed) = std::env::var_os(SANDBOX_HOST_ENV).map(std::path::PathBuf::from) else {
        eprintln!("{SANDBOX_HOST_ENV} が無いので sandbox の検査をスキップする");
        return;
    };
    let reply = mm_host_cef::probe_reply(&sandboxed);
    let expected = format!("{}{}", mm_host_cef::PROBE_REPLY, mm_host_cef::PROBE_SANDBOX);
    assert_eq!(
        reply.as_deref(),
        Some(expected.as_str()),
        "bootstrapc から起こすと sandbox つき"
    );
    let ctx = gpu();
    let size = Size::new(320, 180);
    let page = include_str!("pages/golden.html");
    let frames: Vec<_> = [exe, sandboxed]
        .iter()
        .map(|exe| {
            let mut host = PipeHost::start(spec(exe, Some(std::process::id()))).expect("起動");
            prepare(&mut host, page);
            let t = RationalTime::seconds(1);
            to_image(&render(&mut host, t, size, serde_json::json!({})), &ctx)
        })
        .collect();
    assert_eq!(frames[0], frames[1], "sandbox の有無で画素が変わらない");
}
