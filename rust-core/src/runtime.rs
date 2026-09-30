// 主线程调度：所有 AppKit/AVFoundation 对象只在 macOS 主线程访问。
// 使用主线程 CFRunLoop 在真正的 macOS 主线程执行并等待结果。

use objc2_foundation::MainThreadMarker;

#[cfg(not(test))]
fn schedule_on_main(f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    use block2::RcBlock;
    use objc2_core_foundation::{kCFRunLoopCommonModes, CFRunLoop, CFType};

    let loop_ref = CFRunLoop::main().ok_or("无法获取主线程事件循环")?;
    let mode = unsafe { kCFRunLoopCommonModes }.ok_or("无法获取主线程事件循环模式")?;
    // CFRunLoopPerformBlock targets the actual main run loop. A GCD main-queue
    // callback can run on a helper thread while the app is being launched,
    // which cannot safely touch AppKit and previously aborted the process.
    let task = std::sync::Mutex::new(Some(f));
    let block = RcBlock::new(move || {
        if let Some(f) = task.lock().ok().and_then(|mut task| task.take()) {
            f();
        }
    });
    // SAFETY: the mode is the system's common-mode CFString, and the copied
    // block owns its Send closure until the main run loop invokes it.
    unsafe { loop_ref.perform_block(Some(mode.as_ref() as &CFType), Some(&block)) };
    loop_ref.wake_up();
    Ok(())
}

/// 初始化 runtime（CFRunLoop-based，无需额外初始化）
pub fn init() {}

/// 在主线程执行并等待结果。已在主线程时直接内联执行（避免死锁）。
pub fn on_main<T: Send + 'static>(
    f: impl FnOnce(&MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(&mtm));
    }

    // Rust unit tests do not start NSApplication's main run loop. Keep their
    // pure validation paths synchronous instead of waiting forever on a main
    // run loop that is not being serviced. Production uses the real main run loop.
    #[cfg(test)]
    {
        // SAFETY: test-only callers exercise logic that does not retain or
        // access AppKit objects after returning. Production code never uses
        // this branch.
        Ok(f(unsafe { &MainThreadMarker::new_unchecked() }))
    }

    #[cfg(not(test))]
    {
        let (tx, rx) = std::sync::mpsc::channel();

        schedule_on_main(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                let _ = tx.send(Ok(f(&mtm)));
            } else {
                let _ = tx.send(Err("主线程调度未运行在真正的主线程".to_string()));
            }
        })?;

        rx.recv().map_err(|e| format!("主线程结果丢失: {e}"))?
    }
}

/// 异步在主线程执行（不等待结果）
pub fn on_main_async(f: impl FnOnce(&MainThreadMarker) + Send + 'static) {
    if let Some(mtm) = MainThreadMarker::new() {
        f(&mtm);
        return;
    }

    #[cfg(test)]
    {
        // SAFETY: see the synchronous test-only path in `on_main`.
        f(unsafe { &MainThreadMarker::new_unchecked() });
    }

    #[cfg(not(test))]
    {
        if let Err(error) = schedule_on_main(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                f(&mtm);
            } else {
                eprintln!("[runtime] 主线程调度异常：任务未执行");
            }
        }) {
            eprintln!("[runtime] {error}");
        }
    }
}

/// 获取应用支持目录路径
pub fn app_support_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "无法获取 HOME 目录")?;
    let mut path = std::path::PathBuf::from(home);
    path.push("Library/Application Support/com.dot.wallpaper");
    Ok(path)
}

/// 获取缓存目录路径
pub fn app_cache_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "无法获取 HOME 目录")?;
    let mut path = std::path::PathBuf::from(home);
    path.push("Library/Caches/com.dot.wallpaper");
    Ok(path)
}

/// 获取图片目录路径
pub fn picture_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let pictures = std::path::PathBuf::from(&home).join("Pictures");
    if pictures.is_dir() {
        pictures
    } else {
        std::path::PathBuf::from(&home)
    }
}
