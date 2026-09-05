// 缩略图管线：Rust 端为列表扫描到的壁纸生成 256px 缩略图缓存。
//
// - 缓存目录：Tauri app_cache_dir 下 thumbnails 子目录（普通用户目录，非 C:\Windows）
// - 命名：基于原图绝对路径的 FNV-1a 64 稳定哈希（自实现；禁止 DefaultHasher，
//   其种子每次进程启动随机，无法跨进程复用缓存）
// - 编码：新生成缓存统一 JPEG quality 85（快速编码）；历史 WebP lossless（.webp）
//   缓存仍复用，但不再新产生 WebP——image 0.25 的 WebP 编码器仅支持 lossless(VP8L)，
//   编码极慢，弃用
// - 生成策略：列表命令不阻塞等待全量生成。返回前仅尽力保证首屏窗口
//   （前 PREFETCH_N 张缺失项）在 PREFETCH_BUDGET_MS 预算内生成，其余缺失项交由
//   后台线程池渐进生成；每完成一张通过 "thumbnail-updated" 事件推送 { path, thumb }，
//   前端命中列表条目后单图更新，不做整列表刷新
// - 并发：available_parallelism 封顶 MAX_WORKERS；共享队列按序取图，天然分批节流，
//   避免切换目录瞬间解码打满 CPU 拖垮 UI
// - 小图：长边 <= THUMB_SIZE 的原图跳过生成，直接以原图路径作为 thumb
// - 删除：删除原图时同步清理 jpg/webp 两种历史缓存

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageReader};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

/// 缩略图最长边（px）
const THUMB_SIZE: u32 = 256;
/// 新生成缓存的主格式扩展名（JPEG 快速编码）
const THUMB_EXT: &str = "jpg";
/// 历史 WebP lossless 缓存扩展名（仅复用，不再新生成）
const LEGACY_THUMB_EXT: &str = "webp";
/// 首屏优先窗口：列表命令返回前等待生成的缺失项数量上限
const PREFETCH_N: usize = 36;
/// 列表命令等待首屏生成的预算（毫秒），超时立即返回，剩余交给后台
const PREFETCH_BUDGET_MS: u64 = 1200;
/// 后台并发线程数上限（CPU 密集解码/编码，封顶避免打满）
const MAX_WORKERS: usize = 8;
/// 前端监听的后台缩略图完成事件名
const THUMB_EVENT: &str = "thumbnail-updated";

/// 后台缩略图完成通知 payload：原图路径 + 缩略图路径
#[derive(Clone, serde::Serialize)]
struct ThumbnailUpdatedPayload {
    path: String,
    thumb: String,
}

/// 列表条目：原图路径 + 缩略图路径（可能为空，表示尚未生成/生成失败）
#[derive(serde::Serialize, Clone)]
pub struct WallpaperEntry {
    /// 原图绝对路径（设置壁纸 / 右侧大预览使用）
    pub path: String,
    /// 缩略图绝对路径；无法生成或仍由后台生成中时为 ""（列表显示占位）
    pub thumb: String,
}

/// 缩略图缓存目录：Tauri app_cache_dir 下 thumbnails 子目录
pub fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("获取应用缓存目录失败：{e}"))?;
    Ok(cache.join("thumbnails"))
}

/// FNV-1a 64 位稳定哈希：输入为原文件绝对路径（Windows 风格），
/// 跨进程固定不变，保证不同进程/启动之间缩略图文件名一致、可复用。
fn fnv1a64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn thumb_file_name(src: &str, ext: &str) -> String {
    format!("{:016x}.{}", fnv1a64(src), ext)
}

/// 缓存命中检测：新格式 .jpg 优先，历史 .webp 亦复用；命中返回绝对路径
fn cached_thumb(src: &str, cache: &Path) -> Option<String> {
    for ext in [THUMB_EXT, LEGACY_THUMB_EXT] {
        let p = cache.join(thumb_file_name(src, ext));
        if p.is_file() {
            return Some(p.to_string_lossy().replace('/', "\\"));
        }
    }
    None
}

/// 为一张壁纸生成缩略图缓存（JPEG q85 快速编码），返回列表可用的 thumb 路径。
///
/// - 已有 jpg/webp 缓存 -> 直接返回（后台队列中理论不出现，防御性复用）
/// - 长边 <= THUMB_SIZE 的小图 -> 跳过生成，直接以原图路径作为 thumb（不写缓存）
/// - 其余 -> 解码 -> thumbnail(256) -> JPEG q85 落盘 {fnv}.jpg
/// - 解码/编码失败（损坏、不支持格式）-> None，仅影响该条，不影响整体
fn ensure_thumb_generate(src: &str, cache: &Path) -> Option<String> {
    if let Some(t) = cached_thumb(src, cache) {
        return Some(t);
    }
    let reader = ImageReader::open(src).ok()?;
    let dims = reader.into_dimensions().ok()?;
    // 小图无需缩放：直接以原图路径作为 thumb（列表加载原图，避免解码/落盘）
    if dims.0.max(dims.1) <= THUMB_SIZE {
        return Some(src.to_string());
    }
    let img = ImageReader::open(src).ok()?.decode().ok()?;
    let thumb = img.thumbnail(THUMB_SIZE, THUMB_SIZE);
    let (w, h) = (thumb.width(), thumb.height());
    let rgb = thumb.to_rgb8();
    let mut bytes: Vec<u8> = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 85);
    encoder
        .encode(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)
        .ok()?;
    let target = cache.join(thumb_file_name(src, THUMB_EXT));
    std::fs::write(&target, &bytes).ok()?;
    Some(target.to_string_lossy().replace('/', "\\"))
}

/// 跨列表请求的全局"正在后台生成"去重表：快速反复切换目录/来源时，
/// 同一路径只允许一个后台任务处理，其余请求等待其完成事件推送即可。
fn inflight_set() -> &'static Mutex<HashSet<String>> {
    static INFLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 启动一批后台缩略图生成 worker：并发受 available_parallelism 封顶约束，
/// 共享队列按序取图（天然分批节流）。每完成一张写 results 并 emit 事件，
/// 完成后从全局 inflight 表移除。
fn spawn_thumb_batch(
    app: tauri::AppHandle,
    cache: PathBuf,
    batch: &[String],
) -> Arc<Mutex<HashMap<String, Option<String>>>> {
    let queue: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(batch.iter().cloned().collect()));
    let results: Arc<Mutex<HashMap<String, Option<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let concurrency = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, MAX_WORKERS);

    for _ in 0..concurrency {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&results);
        let cache = cache.clone();
        let app = app.clone();
        std::thread::spawn(move || loop {
            let next = {
                let mut q = queue.lock().unwrap();
                q.pop_front()
            };
            let Some(src) = next else { break };
            let thumb = ensure_thumb_generate(&src, &cache);
            if let Some(t) = &thumb {
                let payload = ThumbnailUpdatedPayload {
                    path: src.clone(),
                    thumb: t.clone(),
                };
                let _ = app.emit(THUMB_EVENT, payload);
            }
            results.lock().unwrap().insert(src.clone(), thumb);
            inflight_set().lock().unwrap().remove(&src);
        });
    }
    results
}

/// 列表命令返回前等待首屏窗口（batch 前 PREFETCH_N 张）处理完成，
/// 受 PREFETCH_BUDGET_MS 预算约束；超时未完成项继续由后台线程生成并推送事件。
fn wait_prefetch(batch: &[String], results: &Arc<Mutex<HashMap<String, Option<String>>>>) {
    let prefetch: HashSet<&String> = batch.iter().take(PREFETCH_N).collect();
    if prefetch.is_empty() {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(PREFETCH_BUDGET_MS);
    loop {
        let done = {
            let r = results.lock().unwrap();
            prefetch.iter().all(|p| r.contains_key(*p))
        };
        if done || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

/// 批量构造列表条目（首屏优先 + 后台渐进）。
///
/// `cache` 为 None（缓存目录不可解析）时返回空缩略图条目，不影响列表展示。
/// 返回条目顺序与输入一致；thumb 为空表示缩略图后台生成中或生成失败，
/// 前端以占位显示，成功完成后经事件收到更新。
pub fn make_entries(
    app: &tauri::AppHandle,
    paths: Vec<String>,
    cache: Option<&Path>,
) -> Vec<WallpaperEntry> {
    if paths.is_empty() {
        return Vec::new();
    }
    let Some(cache) = cache else {
        return paths
            .into_iter()
            .map(|p| WallpaperEntry {
                path: p,
                thumb: String::new(),
            })
            .collect();
    };

    let _ = std::fs::create_dir_all(cache);

    // 第一轮：快速缓存命中检测（仅 stat，毫秒级，不阻塞）
    let mut thumb_by_path: HashMap<String, String> = HashMap::new();
    let mut missing: Vec<String> = Vec::new();
    for p in &paths {
        match cached_thumb(p, cache) {
            Some(t) => {
                thumb_by_path.insert(p.clone(), t);
            }
            None => missing.push(p.clone()),
        }
    }
    if missing.is_empty() {
        return paths
            .into_iter()
            .map(|p| WallpaperEntry {
                path: p.clone(),
                thumb: thumb_by_path.get(&p).cloned().unwrap_or_default(),
            })
            .collect();
    }

    // 第二层：过滤已被其他后台批次接管的路径，避免重复解码
    let mut batch: Vec<String> = Vec::new();
    {
        let mut inflight = inflight_set().lock().unwrap();
        for p in &missing {
            if !inflight.contains(p) {
                inflight.insert(p.clone());
                batch.push(p.clone());
            }
        }
    }
    // 全部已被旧批次接管：立即返回空 thumb 条目，等待事件推送补图
    if batch.is_empty() {
        return paths
            .into_iter()
            .map(|p| WallpaperEntry {
                path: p.clone(),
                thumb: thumb_by_path.get(&p).cloned().unwrap_or_default(),
            })
            .collect();
    }

    // 启动后台批次，命令内仅等待首屏窗口部分
    let results = spawn_thumb_batch(app.clone(), cache.to_path_buf(), &batch);
    wait_prefetch(&batch, &results);

    paths
        .into_iter()
        .map(|p| {
            let thumb = match results.lock().unwrap().get(&p) {
                Some(Some(t)) => Some(t.clone()),
                // batch 内路径原本全部缺失缓存；此处保留缓存命中值兜底
                Some(None) | None => thumb_by_path.get(&p).cloned(),
            };
            WallpaperEntry {
                path: p.clone(),
                thumb: thumb.unwrap_or_default(),
            }
        })
        .collect()
}

/// 删除原图时同步删除其缩略图缓存（jpg 新格式 + webp 历史格式，尽力而为）
pub fn delete_thumb(src: &str, cache: &Path) {
    if !cache.is_dir() {
        return;
    }
    for ext in [THUMB_EXT, LEGACY_THUMB_EXT] {
        let p = cache.join(thumb_file_name(src, ext));
        if p.is_file() {
            let _ = std::fs::remove_file(p);
        }
    }
}
