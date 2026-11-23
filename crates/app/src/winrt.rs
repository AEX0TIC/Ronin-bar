//! Waiting on Windows' asynchronous calls with a time limit. Some of them (radios, hotspot,
//! Bluetooth devices) can take seconds or, on some PCs, never answer; a plain wait would then
//! freeze everything queued behind it. Here a call that runs too long is cancelled instead.
use std::time::{Duration, Instant};
use windows::core::RuntimeType;
use windows_future::{AsyncStatus, IAsyncOperation};

pub fn wait<T: RuntimeType + 'static>(op: windows::core::Result<IAsyncOperation<T>>, limit: Duration) -> Result<T, String> {
    let op = op.map_err(|e| e.message())?;
    let start = Instant::now();
    loop {
        match op.Status() {
            Ok(AsyncStatus::Completed) => return op.GetResults().map_err(|e| e.message()),
            Ok(AsyncStatus::Error) => {
                let code = op.ErrorCode().map(|c| windows::core::Error::from_hresult(c).message()).unwrap_or_default();
                return Err(if code.is_empty() { "Windows reported an error".into() } else { code })
            }
            Ok(AsyncStatus::Canceled) => return Err("Cancelled".into()),
            Ok(_) => {}
            Err(e) => return Err(e.message()),
        }
        if start.elapsed() >= limit {
            let _ = op.Cancel();
            return Err("Windows did not answer in time".into())
        }
        std::thread::sleep(Duration::from_millis(if start.elapsed() < Duration::from_millis(300) { 10 } else { 40 }));
    }
}

/// Runs `f` on a short-lived thread of its own (with COM ready), so a slow Windows call never
/// holds up anything else.
pub fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || { crate::sys::com_init_mta(); f(); });
}
