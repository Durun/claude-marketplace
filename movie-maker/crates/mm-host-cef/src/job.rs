//! ホストと、ホストが起こす CEF のプロセス（GPU、レンダラ）をまとめて止めるための Windows のジョブオブジェクト。
//!
//! ホストだけを `TerminateProcess` で止めると、CEF の子のプロセスがしばらく残って一時フォルダーを掴み続ける。
//! ジョブに入れておけば、止めるときに木ごと止められ、アプリが落ちてジョブのハンドルが閉じたときも一緒に止まる。

use std::process::Child;

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// 1 つのホストの木。
    pub struct Job(HANDLE);

    // SAFETY: ジョブのハンドルはプロセスの中のどのスレッドからでも使える。
    unsafe impl Send for Job {}
    // SAFETY: 同上。ハンドルを書き換える操作は無い。
    unsafe impl Sync for Job {}

    impl Job {
        /// ジョブを作って `child` を入れる。作れなければ `None`（止めるときはホストだけを止める）。
        pub fn attach(child: &Child) -> Option<Self> {
            // SAFETY: 名前の無いジョブを作る。失敗は Err で返る。
            let job = Self(unsafe { CreateJobObjectW(None, None) }.ok()?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of_val(&info) as u32;
            let process = HANDLE(child.as_raw_handle());
            // SAFETY: info はこの呼び出しの間だけ有効な、記述どおりの大きさの構造体。process は生きている子のハンドル。
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    size,
                )
                .ok()?;
                AssignProcessToJobObject(job.0, process).ok()?;
            }
            Some(job)
        }

        /// ジョブの中のプロセスを全部止める。
        pub fn terminate(&self) {
            // SAFETY: 自分で作ったジョブのハンドル。止まっているプロセスしか無ければ何もしない。
            let _ = unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: CreateJobObjectW で作ったハンドル。閉じると KILL_ON_JOB_CLOSE で残りも止まる。
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::process::Child;

    /// ジョブの無い環境の代わり。止めるときはホストだけを止める。
    pub struct Job;

    impl Job {
        pub fn attach(_child: &Child) -> Option<Self> {
            None
        }

        pub fn terminate(&self) {}
    }
}

pub use imp::Job;

/// `child` を、ジョブがあればジョブごと止めて、終わるのを待つ。
pub fn kill_tree(child: &mut Child, job: Option<&Job>) -> std::io::Result<()> {
    if let Some(job) = job {
        job.terminate();
    }
    // 既に終わっていれば kill は失敗するが、wait で後片付けだけする。
    let _ = child.kill();
    child.wait().map(|_| ())
}
