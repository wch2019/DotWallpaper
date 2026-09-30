// macOS 视频壁纸引擎。
// - 每块显示器一个 PlaybackSession，仅在 macOS 主线程访问（thread_local 持有全部对象）
// - AVQueuePlayer + AVPlayerLooper + AVPlayerLayer 共同存活，窗口位于桌面图标之下
// - 事务式切换：新会话先候补（隐藏），首帧就绪后才替换旧会话并把海报落为系统壁纸；
//   失败/超时只销毁候补会话，旧会话与旧系统壁纸原样保留
// - 每显示器维护 generation 计数：apply/stop/断开均递增，过期候补的 watcher 自动作废，
//   杜绝快速连续切换时旧 watcher 提交新会话
// - 显示器热插拔 / 休眠唤醒由监视线程处理，操作一律投递回主线程
// - 所有状态变化经 publish() 写入镜像并通过回调通知上层

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_av_foundation::{
    AVAsset, AVAssetImageGenerator, AVPlayerItem, AVPlayerItemStatus, AVPlayerLayer,
    AVPlayerLooper, AVPlayerLooperStatus, AVPlayerStatus, AVQueuePlayer,
};
use objc2_core_media::CMTime;
use objc2_foundation::MainThreadMarker;

use crate::desktop;
use crate::displays;
use crate::media;
use crate::runtime::on_main;

/// 在 monitor 等后台路径上隔离主线程回调中的 Rust panic。
/// AppKit/objc2 的边界一旦遇到无效的显示器状态，不能让 panic 穿过
/// CFRunLoop block 直接终止整个原生宿主；本轮操作失败即可，下一轮继续重试。
fn on_main_safe<T: Send + 'static>(
    label: &'static str,
    f: impl FnOnce(&MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    on_main(move |mtm| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(mtm)))
            .map_err(|_| format!("{label} 主线程回调异常"))
    })?
}
use crate::settings;
use crate::types::{
    ControlAction, DisplayWallpaperState, FitMode, MediaKind, Phase, WallpaperAssignment,
};

/// 状态变化回调类型
pub type StateCallback = extern "C" fn(*const std::ffi::c_char);

/// 全局状态回调
static STATE_CB: OnceLock<Mutex<Option<StateCallback>>> = OnceLock::new();

/// 注册状态变化回调
pub fn set_state_callback(cb: StateCallback) {
    let mut guard = STATE_CB
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(cb);
}

/// 一个显示器的完整播放会话（播放器、循环器、图层共同存活）
struct Session {
    window: Retained<NSWindow>,
    player: Retained<AVQueuePlayer>,
    looper: Retained<AVPlayerLooper>,
    layer: Retained<AVPlayerLayer>,
    assignment: WallpaperAssignment,
    phase: Phase,
    user_paused: bool,
    sleep_paused: bool,
}

/// 候补会话（首帧就绪前的隐藏窗口），携带创建时的 generation 用于过期判定
struct StagedSession {
    generation: u64,
    session: Session,
}

thread_local! {
    /// 当前正在展示的会话
    static SESSIONS: RefCell<HashMap<String, Session>> = RefCell::new(HashMap::new());
    /// 首帧就绪前的候补会话（隐藏窗口）
    static STAGED: RefCell<HashMap<String, StagedSession>> = RefCell::new(HashMap::new());
}

/// 每显示器代数计数：任何"接管意图"（新 apply / stop / 断开销毁）都递增，
/// 候补 watcher 提交前校验自己创建时的代数，过期即自我销毁，防止旧任务覆盖新会话
fn generations() -> &'static Mutex<HashMap<String, u64>> {
    static GENS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    GENS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bump_generation(display_id: &str) -> u64 {
    let mut map = generations().lock().unwrap_or_else(|e| e.into_inner());
    let g = map.entry(display_id.to_string()).or_insert(0);
    *g += 1;
    *g
}

fn current_generation(display_id: &str) -> u64 {
    generations()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(display_id)
        .copied()
        .unwrap_or(0)
}

/// 任意线程可读的最新状态镜像（主线程逻辑写入）
fn states() -> &'static Mutex<HashMap<String, DisplayWallpaperState>> {
    static STATES: OnceLock<Mutex<HashMap<String, DisplayWallpaperState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 写入状态镜像并通过回调通知上层（任意线程可调）
fn publish(state: &DisplayWallpaperState) {
    if let Ok(mut map) = states().lock() {
        map.insert(state.display_id.clone(), state.clone());
    }
    // 通过 C 回调通知上层
    if let Some(guard) = STATE_CB.get() {
        if let Ok(cb_guard) = guard.lock() {
            if let Some(cb) = *cb_guard {
                let json = serde_json::to_string(state).unwrap_or_default();
                let c_str = std::ffi::CString::new(json).unwrap_or_default();
                cb(c_str.as_ptr());
            }
        }
    }
}

pub fn states_snapshot() -> Vec<DisplayWallpaperState> {
    states()
        .lock()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default()
}

fn state_of(
    display_id: &str,
    phase: Phase,
    assignment: Option<WallpaperAssignment>,
    error: Option<String>,
) -> DisplayWallpaperState {
    DisplayWallpaperState {
        display_id: display_id.to_string(),
        phase,
        assignment,
        error,
    }
}

/// 状态镜像里是否仍把该文件当作某台显示器**当前展示**的内容。
pub fn is_path_displayed(path: &str) -> bool {
    states()
        .lock()
        .map(|m| {
            m.values()
                .filter_map(|s| s.assignment.as_ref())
                .any(|a| same_path(&a.path, path))
        })
        .unwrap_or(true)
}

fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// 视频海报输出路径（应用缓存目录）
pub fn poster_path(video: &str) -> PathBuf {
    crate::thumbs::poster_dir().join(crate::thumbs::hashed_file_name(video, "jpg"))
}

#[allow(deprecated)]
/// 用 AVAssetImageGenerator 抽取首帧并编码为 JPEG（主线程调用）。
pub fn generate_poster(video: &str, out: &Path) -> Result<(), String> {
    let _mtm = MainThreadMarker::new().ok_or("海报生成必须在主线程")?;
    let url = desktop::ns_url_for_path(video);
    let asset = unsafe { AVAsset::assetWithURL(&url) };
    let gen = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    unsafe { gen.setAppliesPreferredTrackTransform(true) };
    let time = unsafe { CMTime::new(0, 600) };
    let mut actual = unsafe { std::mem::zeroed() };
    let image = unsafe { gen.copyCGImageAtTime_actualTime_error(time, &mut actual) }
        .map_err(|e| format!("无法提取视频首帧: {e}"))?;
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    unsafe {
        crate::cfmedia::encode_jpeg(&*image as *const _ as *const _, &out.to_string_lossy(), 0.9)
    }
}

/// 该路径是否正被某块显示器的播放会话（含候补）使用。
pub fn is_path_in_use(path: &str) -> bool {
    let target = path.to_string();
    on_main(move |_mtm| {
        SESSIONS.with(|s| {
            s.borrow()
                .values()
                .any(|x| same_path(&x.assignment.path, &target))
        }) || STAGED.with(|s| {
            s.borrow()
                .values()
                .any(|x| same_path(&x.session.assignment.path, &target))
        })
    })
    .unwrap_or(false)
}

/// 统一应用入口（任意线程调用，内部调度到主线程）。
pub fn apply(assignment: WallpaperAssignment) -> Result<DisplayWallpaperState, String> {
    let path = media::validate_for_apply(&assignment.path, assignment.kind)?;
    let assignment = WallpaperAssignment { path, ..assignment };

    {
        let guard = states().lock().map_err(|e| e.to_string())?;
        if let Some(s) = guard.get(&assignment.display_id) {
            let same = s.assignment.as_ref().is_some_and(|a| *a == assignment);
            if same && matches!(s.phase, Phase::Static | Phase::Playing | Phase::Paused) {
                return Ok(s.clone());
            }
        }
    }

    let a = assignment.clone();
    let state = match assignment.kind {
        MediaKind::Image => on_main(move |mtm| apply_static_main(mtm, &a))?,
        MediaKind::Video => {
            let (state, gen) = on_main(move |mtm| start_video_main(mtm, &a))??;
            if state.phase == Phase::Preparing {
                spawn_prepare_watch(assignment.display_id.clone(), gen);
            }
            Ok(state)
        }
    }?;

    publish(&state);
    clear_failure(&assignment.display_id, &assignment.path);
    if state.phase == Phase::Static {
        settings::record_assignment(&state.assignment.clone().unwrap_or(assignment));
    }
    Ok(state)
}

fn apply_static_main(
    mtm: &MainThreadMarker,
    a: &WallpaperAssignment,
) -> Result<DisplayWallpaperState, String> {
    let screen = desktop::screen_for_stable_id(mtm, &a.display_id)
        .ok_or_else(|| format!("未找到显示器: {}", a.display_id))?;
    desktop::set_static_wallpaper(mtm, &screen, &a.path, a.fit_mode)?;
    if let Some(read) = desktop::current_static_wallpaper(mtm, &screen) {
        if !read.is_empty() && !same_path(&read, &a.path) {
            eprintln!(
                "[wallpaper] 系统已接受壁纸设置，等待异步刷新（当前回读: {read}，目标: {}）",
                a.path
            );
        }
    }
    destroy_sessions_for(mtm, &a.display_id);
    Ok(state_of(
        &a.display_id,
        Phase::Static,
        Some(a.clone()),
        None,
    ))
}

fn start_video_main(
    mtm: &MainThreadMarker,
    a: &WallpaperAssignment,
) -> Result<(DisplayWallpaperState, u64), String> {
    let screen = desktop::screen_for_stable_id(mtm, &a.display_id)
        .ok_or_else(|| format!("未找到显示器: {}", a.display_id))?;

    let gen = bump_generation(&a.display_id);
    drop_staged(&a.display_id);

    let session = match build_session(a, &screen) {
        Ok(s) => s,
        Err(e) => {
            let live = live_state(&a.display_id);
            return match live {
                Some(s) if s.phase == Phase::Playing || s.phase == Phase::Paused => {
                    Err(format!("{e}（已保留原壁纸）"))
                }
                _ => Err(e),
            };
        }
    };
    STAGED.with(|s| {
        s.borrow_mut().insert(
            a.display_id.clone(),
            StagedSession {
                generation: gen,
                session,
            },
        )
    });
    Ok((
        state_of(&a.display_id, Phase::Preparing, Some(a.clone()), None),
        gen,
    ))
}

fn build_session(
    a: &WallpaperAssignment,
    screen: &objc2_app_kit::NSScreen,
) -> Result<Session, String> {
    let frame = screen.frame();
    let window: Retained<NSWindow> = unsafe {
        let cls = objc2::class!(NSWindow);
        let allocated: *mut NSWindow = objc2::msg_send![cls, alloc];
        let window: *mut NSWindow = objc2::msg_send![allocated, initWithContentRect: frame,
            styleMask: NSWindowStyleMask::Borderless,
            backing: NSBackingStoreType::Buffered,
            defer: false];
        Retained::from_raw(window).ok_or("桌面播放窗口创建失败")?
    };
    configure_window(&window, frame);

    let url = desktop::ns_url_for_path(&a.path);
    let asset = unsafe { AVAsset::assetWithURL(&url) };
    let item: Retained<AVPlayerItem> =
        unsafe { objc2::msg_send![objc2::class!(AVPlayerItem), playerItemWithAsset: &*asset] };
    // The template is not a playback item. Let AVPlayerLooper own the queue of replicas.
    let items = objc2_foundation::NSArray::<AVPlayerItem>::new();
    let player: Retained<AVQueuePlayer> =
        unsafe { objc2::msg_send![objc2::class!(AVQueuePlayer), queuePlayerWithItems: &*items] };
    let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item) };
    let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&player)) };
    let gravity = unsafe {
        match a.fit_mode {
            FitMode::Fill => objc2_av_foundation::AVLayerVideoGravityResizeAspectFill
                .ok_or("视频填充模式不可用")?,
            FitMode::Fit => {
                objc2_av_foundation::AVLayerVideoGravityResizeAspect.ok_or("视频适应模式不可用")?
            }
        }
    };
    unsafe {
        layer.setVideoGravity(gravity);
        player.setMuted(true);
    }
    if let Some(view) = window.contentView() {
        view.setWantsLayer(true);
        view.setLayer(Some(&layer));
        layer.setFrame(view.bounds());
    }
    unsafe { player.play() };

    Ok(Session {
        window,
        player,
        looper,
        layer,
        assignment: a.clone(),
        phase: Phase::Preparing,
        user_paused: false,
        sleep_paused: false,
    })
}

fn configure_window(window: &NSWindow, frame: objc2_foundation::NSRect) {
    let level = objc2_core_graphics::CGWindowLevelForKey(
        objc2_core_graphics::CGWindowLevelKey::DesktopIconWindowLevelKey,
    );
    window.setLevel((level - 1) as isize);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setOpaque(false);
    window.setHasShadow(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setIgnoresMouseEvents(true);
    window.setAcceptsMouseMovedEvents(false);
    window.setCanHide(false);
    unsafe { window.setReleasedWhenClosed(false) };
    window.setFrame_display(frame, false);
}

fn close_session(session: &Session) {
    unsafe {
        session.player.pause();
        session.window.orderOut(None);
    }
}

fn drop_staged(display_id: &str) {
    STAGED.with(|s| {
        if let Some(st) = s.borrow_mut().remove(display_id) {
            close_session(&st.session);
        }
    });
}

fn destroy_sessions_for(_mtm: &MainThreadMarker, display_id: &str) {
    bump_generation(display_id);
    drop_staged(display_id);
    SESSIONS.with(|s| {
        if let Some(sess) = s.borrow_mut().remove(display_id) {
            close_session(&sess);
        }
    });
}

fn live_state(display_id: &str) -> Option<DisplayWallpaperState> {
    let mirror = states()
        .lock()
        .ok()
        .and_then(|m| m.get(display_id).cloned());
    let live = on_main({
        let id = display_id.to_string();
        move |_mtm| {
            SESSIONS.with(|s| {
                s.borrow()
                    .get(&id)
                    .map(|x| state_of(&id, x.phase, Some(x.assignment.clone()), None))
            })
        }
    })
    .ok()
    .flatten();
    live.or(mirror)
}

fn spawn_prepare_watch(display_id: String, generation: u64) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let id = display_id.clone();
            let outcome = on_main(move |mtm| watch_step(mtm, &id, generation));
            let done = match outcome {
                Ok(WatchOutcome::Waiting) => {
                    if Instant::now() >= deadline {
                        Some(fail_staged(
                            &display_id,
                            generation,
                            "播放器准备超时（文件可能缺失或系统无法解码）",
                        ))
                    } else {
                        None
                    }
                }
                Ok(WatchOutcome::Ready(state)) => Some(state),
                Ok(WatchOutcome::Failed(msg)) => Some(fail_staged(&display_id, generation, &msg)),
                Ok(WatchOutcome::Gone) | Err(_) => return,
            };
            if let Some(state) = done {
                if current_generation(&display_id) == generation {
                    publish(&state);
                    if state.phase == Phase::Playing {
                        if let Some(a) = state.assignment.clone() {
                            settings::record_assignment(&a);
                        }
                    }
                }
                return;
            }
        }
    });
}

enum WatchOutcome {
    Waiting,
    Gone,
    Ready(DisplayWallpaperState),
    Failed(String),
}

fn watch_step(mtm: &MainThreadMarker, display_id: &str, generation: u64) -> WatchOutcome {
    let staged = STAGED.with(|s| {
        let mut map = s.borrow_mut();
        match map.get(display_id) {
            Some(st) if st.generation != generation => None,
            _ => map.remove(display_id),
        }
    });
    let Some(st) = staged else {
        return WatchOutcome::Gone;
    };
    let lstatus = unsafe { st.session.looper.status() };
    if lstatus == AVPlayerLooperStatus::Failed || lstatus == AVPlayerLooperStatus::Cancelled {
        let msg = unsafe { st.session.looper.error() }
            .map(|e| format!("视频循环准备失败: {}", e.localizedDescription()))
            .unwrap_or_else(|| "视频循环器不可用".to_string());
        close_session(&st.session);
        return WatchOutcome::Failed(msg);
    }
    let pstatus = unsafe { st.session.player.status() };
    if pstatus == AVPlayerStatus::Failed {
        let msg = unsafe { st.session.player.error() }
            .map(|e| format!("播放器准备失败: {}", e.localizedDescription()))
            .unwrap_or_else(|| "播放器准备失败".to_string());
        close_session(&st.session);
        return WatchOutcome::Failed(msg);
    }
    // AVPlayerLooper clones the template; only the queue's current item is decoded.
    let current = unsafe { st.session.player.currentItem() };
    let istatus = current.as_ref().map(|item| unsafe { item.status() });
    if istatus == Some(AVPlayerItemStatus::Failed) {
        let msg = current
            .and_then(|item| unsafe { item.error() })
            .map(|e| format!("播放失败: {}", e.localizedDescription()))
            .unwrap_or_else(|| "播放器准备失败".to_string());
        close_session(&st.session);
        return WatchOutcome::Failed(msg);
    }
    if istatus == Some(AVPlayerItemStatus::ReadyToPlay)
        && pstatus == AVPlayerStatus::ReadyToPlay
        && lstatus == AVPlayerLooperStatus::Ready
    {
        let mut session = st.session;
        session.phase = Phase::Playing;
        unsafe { session.player.setMuted(session.assignment.muted) };
        session.window.orderFront(None::<&AnyObject>);
        let assignment = session.assignment.clone();
        let old = SESSIONS.with(|s| s.borrow_mut().insert(display_id.to_string(), session));
        if let Some(old) = old {
            close_session(&old);
        }
        if let Some(screen) = desktop::screen_for_stable_id(mtm, display_id) {
            let video = assignment.path.clone();
            let poster = poster_path(&video);
            if !poster.is_file() {
                if let Err(e) = generate_poster(&video, &poster) {
                    eprintln!("[poster] {e}");
                }
            }
            if poster.is_file() {
                if let Err(e) = desktop::set_static_wallpaper(
                    mtm,
                    &screen,
                    &poster.to_string_lossy(),
                    FitMode::Fill,
                ) {
                    eprintln!("[poster] 设置海报壁纸失败: {e}");
                }
            }
        }
        return WatchOutcome::Ready(state_of(display_id, Phase::Playing, Some(assignment), None));
    }
    STAGED.with(|s| {
        s.borrow_mut().insert(
            display_id.to_string(),
            StagedSession {
                generation,
                session: st.session,
            },
        )
    });
    WatchOutcome::Waiting
}

fn fail_staged(display_id: &str, generation: u64, msg: &str) -> DisplayWallpaperState {
    let err = msg.to_string();
    let id = display_id.to_string();
    let g = generation;
    let _ = on_main(move |_mtm| {
        let mine = STAGED.with(|s| {
            let map = s.borrow();
            match map.get(&id) {
                Some(x) if x.generation == g => Some(x.session.assignment.path.clone()),
                _ => None,
            }
        });
        if let Some(path) = mine {
            drop_staged(&id);
            latch_failure(&id, &path);
        }
    });
    if let Some(live) = live_state(display_id) {
        if matches!(live.phase, Phase::Playing | Phase::Paused) {
            return state_of(
                display_id,
                live.phase,
                live.assignment,
                Some(format!("视频无法播放，已保留原壁纸：{err}")),
            );
        }
    }
    let prev = states()
        .lock()
        .ok()
        .and_then(|m| m.get(display_id).cloned());
    let assignment = prev
        .as_ref()
        .and_then(|s| s.assignment.clone())
        .or_else(|| settings::get().assignments.get(display_id).cloned());
    state_of(
        display_id,
        Phase::Error,
        assignment,
        Some(format!("无法播放该视频：{err}")),
    )
}

/// 暂停 / 恢复 / 停止（全部调度到主线程操作播放器对象）
pub fn control(display_id: String, action: ControlAction) -> Result<DisplayWallpaperState, String> {
    let state = on_main(move |_mtm| control_main(&display_id, action))??;
    publish(&state);
    match action {
        ControlAction::Pause => settings::set_display_paused(&state.display_id, true),
        ControlAction::Resume => settings::set_display_paused(&state.display_id, false),
        ControlAction::Stop => {
            settings::set_display_paused(&state.display_id, false);
            settings::forget_assignment(&state.display_id);
        }
    }
    Ok(state)
}

fn control_main(display_id: &str, action: ControlAction) -> Result<DisplayWallpaperState, String> {
    if action == ControlAction::Stop {
        let had_staged = STAGED.with(|s| s.borrow().contains_key(display_id));
        if had_staged {
            bump_generation(display_id);
            drop_staged(display_id);
        }
    } else {
        let preparing = STAGED.with(|s| s.borrow().contains_key(display_id));
        if preparing {
            return Err("动态壁纸正在准备中，请稍候再操作".to_string());
        }
    }
    SESSIONS.with(|cell| {
        let mut map = cell.borrow_mut();
        let Some(session) = map.get_mut(display_id) else {
            return match action {
                ControlAction::Stop => Ok(state_of(display_id, Phase::Static, None, None)),
                _ => Err(format!("显示器 {display_id} 当前没有动态壁纸在播放")),
            };
        };
        match action {
            ControlAction::Pause => unsafe { session.player.pause() },
            ControlAction::Resume => unsafe { session.player.play() },
            ControlAction::Stop => {
                let Some(sess) = map.remove(display_id) else {
                    return Err(format!("显示器 {display_id} 当前没有可停止的动态壁纸"));
                };
                bump_generation(display_id);
                close_session(&sess);
                return Ok(state_of(
                    display_id,
                    Phase::Static,
                    Some(sess.assignment),
                    None,
                ));
            }
        }
        session.user_paused = action == ControlAction::Pause;
        if action == ControlAction::Resume {
            session.sleep_paused = false;
        }
        session.phase = match action {
            ControlAction::Pause => Phase::Paused,
            _ => Phase::Playing,
        };
        Ok(state_of(
            display_id,
            session.phase,
            Some(session.assignment.clone()),
            None,
        ))
    })
}

fn bulk_control(action: ControlAction) {
    let _ = on_main(move |mtm| {
        let mut ids: Vec<String> = SESSIONS.with(|s| s.borrow().keys().cloned().collect());
        for k in STAGED.with(|s| s.borrow().keys().cloned().collect::<Vec<_>>()) {
            if !ids.contains(&k) {
                ids.push(k);
            }
        }
        for id in ids {
            if let Ok(state) = control_main(&id, action) {
                publish(&state);
                match action {
                    ControlAction::Pause => settings::set_display_paused(&id, true),
                    ControlAction::Resume => settings::set_display_paused(&id, false),
                    ControlAction::Stop => {
                        settings::set_display_paused(&id, false);
                        settings::forget_assignment(&id);
                    }
                }
            }
        }
        let _ = mtm;
    });
}

pub fn pause_all() {
    bulk_control(ControlAction::Pause);
}

pub fn resume_all() {
    bulk_control(ControlAction::Resume);
}

pub fn stop_all() {
    bulk_control(ControlAction::Stop);
}

/// 退出时同步销毁全部播放会话（含候补）。
pub fn teardown_all_sync() {
    let _ = on_main(|mtm| {
        let ids: Vec<String> = SESSIONS.with(|s| s.borrow().keys().cloned().collect());
        for id in ids {
            destroy_sessions_for(mtm, &id);
        }
        let staged_ids: Vec<String> = STAGED.with(|s| s.borrow().keys().cloned().collect());
        for id in staged_ids {
            destroy_sessions_for(mtm, &id);
        }
    });
    if let Ok(mut m) = states().lock() {
        m.clear();
    }
}

/// 启动时恢复逐屏配置；缺失媒体标记错误，不阻塞其他显示器。
static STARTUP_RESTORE_QUEUED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn restore_on_startup() {
    // Completion of onboarding can race the 300ms startup delay. Enqueue the
    // restore once, and never probe protected files while setup is open.
    if !settings::get().onboarding_completed
        || STARTUP_RESTORE_QUEUED.swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        let s = settings::get();
        // An existing configuration can be migrated into first-run setup. Never
        // touch protected media before the user has seen the folder picker.
        if !s.onboarding_completed {
            return;
        }
        for assignment in s.assignments.values() {
            let display_id = &assignment.display_id;
            let a = assignment.clone();
            if displays::cg_id_for_stable(display_id).is_none() {
                publish(&state_of(
                    display_id,
                    Phase::Error,
                    Some(a),
                    Some("显示器未连接，重连后将自动恢复".to_string()),
                ));
                continue;
            }
            match apply(a.clone()) {
                Ok(state) => {
                    if s.paused_displays.iter().any(|d| d == display_id)
                        && state.phase == Phase::Preparing
                    {
                        let id = display_id.clone();
                        std::thread::spawn(move || wait_then_pause(&id));
                    }
                }
                Err(e) => {
                    latch_failure(display_id, &assignment.path);
                    publish(&state_of(display_id, Phase::Error, Some(a), Some(e)));
                }
            }
        }
    });
}

fn wait_then_pause(display_id: &str) {
    for _ in 0..16 {
        std::thread::sleep(Duration::from_millis(1000));
        let phase = states()
            .lock()
            .ok()
            .and_then(|m| m.get(display_id).map(|s| s.phase));
        match phase {
            Some(Phase::Playing) => {
                let _ = control(display_id.to_string(), ControlAction::Pause);
                return;
            }
            Some(Phase::Paused) | Some(Phase::Static) | Some(Phase::Error) | None => return,
            Some(Phase::Preparing) => continue,
        }
    }
}

fn display_inflight(display_id: &str) -> bool {
    let id = display_id.to_string();
    on_main_safe("检查显示器播放会话", move |_mtm| {
        SESSIONS.with(|s| s.borrow().contains_key(&id))
            || STAGED.with(|s| s.borrow().contains_key(&id))
    })
    .unwrap_or(false)
}

type DisplaySeen = ((f64, f64, f64, f64), bool);

fn restore_failed() -> &'static Mutex<HashSet<String>> {
    static FAILED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FAILED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn failure_key(display_id: &str, path: &str) -> String {
    format!("{display_id}\u{0}{path}")
}

fn latch_failure(display_id: &str, path: &str) {
    if let Ok(mut set) = restore_failed().lock() {
        set.insert(failure_key(display_id, path));
    }
}

fn clear_failure(display_id: &str, path: &str) {
    if let Ok(mut set) = restore_failed().lock() {
        set.remove(&failure_key(display_id, path));
    }
}

fn is_latched_failure(display_id: &str, path: &str) -> bool {
    restore_failed()
        .lock()
        .is_ok_and(|set| set.contains(&failure_key(display_id, path)))
}

/// 显示器热插拔 + 休眠唤醒 + 分辨率变化监视线程。
pub fn spawn_monitor() {
    std::thread::spawn(|| {
        eprintln!("[wallpaper] monitor thread started");
        let mut seen: HashMap<String, DisplaySeen> = HashMap::new();
        let restoring: std::sync::Arc<Mutex<HashSet<String>>> =
            std::sync::Arc::new(Mutex::new(HashSet::new()));
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let active = on_main_safe("读取活动显示器", |_mtm| {
                let mut ids: Vec<String> = SESSIONS.with(|s| s.borrow().keys().cloned().collect());
                for k in STAGED.with(|s| s.borrow().keys().cloned().collect::<Vec<_>>()) {
                    if !ids.contains(&k) {
                        ids.push(k);
                    }
                }
                ids
            })
            .unwrap_or_default();

            for id in &active {
                let Some(cgid) = displays::cg_id_for_stable(id) else {
                    let did = id.clone();
                    let _ = on_main_safe("销毁断开显示器会话", move |mtm| destroy_sessions_for(mtm, &did));
                    let prev = states().lock().ok().and_then(|m| m.get(id).cloned());
                    publish(&state_of(
                        id,
                        Phase::Error,
                        prev.and_then(|s| s.assignment),
                        Some("显示器已断开".to_string()),
                    ));
                    continue;
                };
                let asleep = displays::is_asleep(cgid);
                let bounds = displays::logical_bounds(cgid);
                let prev = seen.get(id).copied();
                let changed = prev.is_some_and(|(b, _)| b != bounds);
                seen.insert(id.clone(), (bounds, asleep));
                let did = id.clone();
                if asleep {
                    let _ = on_main_safe("处理休眠显示器", move |_mtm| {
                        SESSIONS.with(|s| {
                            if let Some(sess) = s.borrow_mut().get_mut(&did) {
                                if !sess.user_paused {
                                    unsafe { sess.player.pause() };
                                    sess.sleep_paused = true;
                                }
                            }
                        });
                    });
                } else {
                    let resume = prev.is_some_and(|(_, a)| a) || changed;
                    if resume {
                        let did2 = id.clone();
                        let _ = on_main_safe("恢复显示器会话", move |mtm| {
                            SESSIONS.with(|s| {
                                if let Some(sess) = s.borrow_mut().get_mut(&did2) {
                                    if let Some(screen) = desktop::screen_for_stable_id(mtm, &did2)
                                    {
                                        let frame = screen.frame();
                                        sess.window.setFrame_display(frame, true);
                                        if let Some(view) = sess.window.contentView() {
                                            sess.layer.setFrame(view.bounds());
                                        }
                                    }
                                    if !sess.user_paused && sess.sleep_paused {
                                        unsafe { sess.player.play() };
                                        sess.sleep_paused = false;
                                    }
                                }
                            });
                        });
                    }
                }
            }

            let s = settings::get();
            if !s.onboarding_completed {
                continue;
            }
            for (id, a) in &s.assignments {
                if a.kind != MediaKind::Video
                    || active.contains(id)
                    || display_inflight(id)
                    || displays::cg_id_for_stable(id).is_none()
                {
                    continue;
                }
                if is_latched_failure(id, &a.path) {
                    continue;
                }
                {
                    let Ok(mut r) = restoring.lock() else {
                        continue;
                    };
                    if !r.insert(id.clone()) {
                        continue;
                    }
                }
                let assignment = a.clone();
                let id = id.clone();
                let restoring = std::sync::Arc::clone(&restoring);
                std::thread::spawn(move || {
                    let shown = assignment.clone();
                    let path = shown.path.clone();
                    match apply(assignment) {
                        Ok(state) => publish(&state),
                        Err(e) => {
                            latch_failure(&id, &path);
                            publish(&state_of(&id, Phase::Static, Some(shown), Some(e)));
                        }
                    }
                    if let Ok(mut r) = restoring.lock() {
                        r.remove(&id);
                    }
                });
            }
        }
    });
}
