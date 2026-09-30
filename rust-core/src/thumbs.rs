// 缩略图管线：使用 macOS ImageIO（见 cfmedia.rs），不引入 Rust 编解码库。
// - 缓存目录：app_cache_dir/thumbnails
// - 命名：路径哈希 + 内容指纹（mtime/大小），源文件被替换后旧缓存自然失效
// - 图片：CGImageSourceCreateThumbnailAtIndex -> JPEG q85
// - 视频：主线程 AVAssetImageGenerator 抽取首帧海报（posters/），再 ImageIO 缩放
// - 列表命令仅等待首屏窗口，其余后台渐进生成

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::cfmedia;
use crate::engine;
use crate::runtime;
use crate::types::{MediaItem, MediaKind};

const THUMB_SIZE: u32 = 256;
const THUMB_EXT: &str = "jpg";
const PREFETCH_N: usize = 36;
const PREFETCH_BUDGET_MS: u64 = 1500;
const MAX_WORKERS: usize = 6;

fn fnv1a64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 源文件内容指纹（mtime + 大小）。文件被替换后指纹改变，缓存名随之更换，
/// 无需全量清库即可让缩略图/海报自动失效。取不到元数据时指纹为 0。
fn content_stamp(src: &str) -> u64 {
    let Ok(meta) = std::fs::metadata(src) else {
        return 0;
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    fnv1a64(&format!("{mtime}:{}", meta.len()))
}

/// 同一源文件的全部缓存变体共有的文件名前缀（仅路径哈希）
fn name_prefix(src: &str) -> String {
    format!("{:016x}", fnv1a64(src))
}

pub fn hashed_file_name(src: &str, ext: &str) -> String {
    format!("{}-{:016x}.{ext}", name_prefix(src), content_stamp(src))
}

/// 删除某源文件在 dir 下的所有缓存变体（含内容变化后遗留的旧名字）
fn remove_variants(dir: &Path, src: &str, ext: &str) {
    if !dir.is_dir() {
        return;
    }
    let (prefix, suffix) = (name_prefix(src), format!(".{ext}"));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(&suffix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn app_path(join: fn(PathBuf) -> PathBuf) -> Result<PathBuf, String> {
    let dir = runtime::app_cache_dir()?;
    Ok(join(dir))
}

pub fn cache_dir() -> Result<PathBuf, String> {
    app_path(|d| d.join("thumbnails"))
}

pub fn poster_dir() -> PathBuf {
    app_path(|d| d.join("posters")).unwrap_or_else(|_| std::env::temp_dir())
}

fn cached_thumb(src: &str, cache: &Path) -> Option<String> {
    let p = cache.join(hashed_file_name(src, THUMB_EXT));
    p.is_file().then(|| p.to_string_lossy().to_string())
}

const POSTER_SPACING: Duration = Duration::from_millis(120);

fn poster_slot() -> &'static Mutex<Instant> {
    static SLOT: OnceLock<Mutex<Instant>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Instant::now()))
}

fn wait_poster_slot() {
    loop {
        let mut next = poster_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        if now >= *next {
            *next = now + POSTER_SPACING;
            return;
        }
        let wait = *next - now;
        drop(next);
        std::thread::sleep(wait.min(Duration::from_millis(20)));
    }
}

fn ensure_poster(src: &str) -> Option<String> {
    let poster = engine::poster_path(src);
    if poster.is_file() {
        return Some(poster.to_string_lossy().to_string());
    }
    wait_poster_slot();
    let src = src.to_string();
    let out = poster.clone();
    runtime::on_main(move |_mtm| engine::generate_poster(&src, &out))
        .ok()?
        .ok()?;
    Some(poster.to_string_lossy().to_string())
}

/// 为单个媒体生成（或复用）缩略图。视频走海报两级缩放。
fn ensure_thumb(src: &str, kind: MediaKind, cache: &Path) -> Option<String> {
    if let Some(t) = cached_thumb(src, cache) {
        return Some(t);
    }
    let _ = std::fs::create_dir_all(cache);
    let source_image = match kind {
        MediaKind::Image => {
            if let Some((w, h)) = cfmedia::image_size(src) {
                if w.max(h) <= THUMB_SIZE as usize {
                    return Some(src.to_string());
                }
            }
            src.to_string()
        }
        MediaKind::Video => ensure_poster(src)?,
    };
    let target = cache.join(hashed_file_name(src, THUMB_EXT));
    cfmedia::make_thumbnail_jpeg(&source_image, &target.to_string_lossy(), THUMB_SIZE).ok()?;
    Some(target.to_string_lossy().to_string())
}

fn inflight_set() -> &'static Mutex<HashSet<String>> {
    static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 缩略图就绪回调类型
pub type ThumbCallback = extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char);

static THUMB_CB: OnceLock<Mutex<Option<ThumbCallback>>> = OnceLock::new();

pub fn set_thumb_callback(cb: ThumbCallback) {
    let mut guard = THUMB_CB
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(cb);
}

/// 构建媒体列表条目：缓存命中即用，缺失项在预算内生成首屏，其余交后台线程。
pub fn make_entries(items: Vec<(String, MediaKind)>) -> Vec<MediaItem> {
    let cache = cache_dir().unwrap_or_default();
    if items.is_empty() {
        return Vec::new();
    }
    let mut thumbs: HashMap<String, String> = HashMap::new();
    let mut missing: Vec<(String, MediaKind)> = Vec::new();
    if !cache.as_os_str().is_empty() {
        for (path, kind) in &items {
            match cached_thumb(path, &cache) {
                Some(t) => {
                    thumbs.insert(path.clone(), t);
                }
                None => missing.push((path.clone(), *kind)),
            }
        }
    } else {
        missing = items.clone();
    }

    let mut batch: Vec<(String, MediaKind)> = Vec::new();
    {
        let mut inflight = inflight_set()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (p, k) in missing {
            if inflight.insert(p.clone()) {
                batch.push((p, k));
            }
        }
    }

    if !batch.is_empty() {
        let results: Arc<Mutex<HashMap<String, Option<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let queue: Arc<Mutex<std::collections::VecDeque<(String, MediaKind)>>> =
            Arc::new(Mutex::new(batch.iter().cloned().collect()));
        let concurrency = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, MAX_WORKERS);
        let cache_for_thread = cache.clone();
        for _ in 0..concurrency {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let cache_t = cache_for_thread.clone();
            std::thread::spawn(move || loop {
                let next = queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .pop_front();
                let Some((src, kind)) = next else { break };
                let thumb = ensure_thumb(&src, kind, &cache_t);
                if let Some(t) = &thumb {
                    if let Some(guard) = THUMB_CB.get() {
                        if let Ok(cb_guard) = guard.lock() {
                            if let Some(cb) = *cb_guard {
                                let path_c =
                                    std::ffi::CString::new(src.clone()).unwrap_or_default();
                                let thumb_c = std::ffi::CString::new(t.clone()).unwrap_or_default();
                                cb(path_c.as_ptr(), thumb_c.as_ptr());
                            }
                        }
                    }
                }
                results
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(src.clone(), thumb);
                inflight_set()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(&src);
            });
        }
        let prefetch: HashSet<String> = batch
            .iter()
            .take(PREFETCH_N)
            .map(|(p, _)| p.clone())
            .collect();
        let deadline = Instant::now() + Duration::from_millis(PREFETCH_BUDGET_MS);
        loop {
            let done = {
                let r = results
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                prefetch.iter().all(|p| r.contains_key(p))
            };
            if done || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        let r = results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (p, t) in r.iter() {
            if let Some(t) = t {
                thumbs.insert(p.clone(), t.clone());
            }
        }
    }

    items
        .into_iter()
        .map(|(path, kind)| {
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            MediaItem {
                thumb: thumbs.get(&path).cloned().unwrap_or_default(),
                mtime: mtime_of(&path),
                path,
                name,
                kind,
            }
        })
        .collect()
}

/// 修改时间（Unix 秒）；读不到记 0，界面按时间排序时把 0 视为未知排末尾
pub(crate) fn mtime_of(path: &str) -> i64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn delete_thumb(src: &str, cache: &Path) {
    remove_variants(cache, src, THUMB_EXT);
    remove_variants(&poster_dir(), src, THUMB_EXT);
}
