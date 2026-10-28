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
/// 视频时长边车文件的扩展名（与缩略图同目录、同哈希名）
const DURATION_EXT: &str = "dur";
/// 首屏优先窗口：列表命令返回前等待生成的缺失项数量上限
const PREFETCH_N: usize = 36;
/// 列表命令等待首屏生成的预算（毫秒），超时立即返回，剩余交给后台
const PREFETCH_BUDGET_MS: u64 = 1200;
/// 后台并发线程数上限（CPU 密集解码/编码，封顶避免打满）
const MAX_WORKERS: usize = 8;
/// 前端监听的后台缩略图完成事件名
const THUMB_EVENT: &str = "thumbnail-updated";

/// 后台缩略图完成通知 payload：原图路径 + 缩略图路径（+ 视频时长）
#[derive(Clone, serde::Serialize)]
struct ThumbnailUpdatedPayload {
    path: String,
    thumb: String,
    /// 视频时长（秒）。图片恒为 None；视频读不到时也是 None。
    /// 与缩略图**同一次解码**得到，所以顺手带给前端，省一次额外请求。
    duration: Option<u32>,
}

/// 列表条目：原图路径 + 缩略图路径（可能为空，表示尚未生成/生成失败）
#[derive(serde::Serialize, Clone)]
pub struct WallpaperEntry {
    /// 原图绝对路径（设置壁纸 / 右侧大预览使用）
    pub path: String,
    /// 缩略图绝对路径；无法生成或仍由后台生成中时为 ""（列表显示占位）
    pub thumb: String,
    /// 文件修改时间（Unix 秒）；读取失败为 None。**仅供前端排序**，不参与其他逻辑。
    pub mtime: Option<i64>,
    /// 文件大小（字节）；读取失败为 None。**仅供前端排序**。
    pub size: Option<u64>,
    /// 视频时长（秒）。图片恒为 None；视频尚未采集到（后台探测中）也为 None。
    /// 前端据此在卡片左下角显示时长角标。
    pub duration: Option<u32>,
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

/// 该路径是否为后端可解码的视频（按扩展名判断，与缩略图管线同一口径）
fn is_video_file(src: &str) -> bool {
    let ext = Path::new(src)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    crate::video_thumbs::is_video_ext(&ext)
}

/// 视频时长边车文件路径：与缩略图同目录、同哈希名，扩展名 `.dur`。
///
/// 为什么单独一个小文件，而不是塞进缩略图或做一个 JSON 索引：
/// 一个文件只放一个数字，读写都是 O(1)，**并发写天然安全**（每个 worker 各写各的），
/// 不需要为索引引入锁；索引还要处理"多个 worker 同时改一个 JSON"的合并问题。
fn duration_file(src: &str, cache: &Path) -> PathBuf {
    cache.join(thumb_file_name(src, DURATION_EXT))
}

/// 读边车时长。三种状态要分清：
/// - `Some(Some(s))`：已知，s 秒
/// - `Some(None)`：**探测过但读不到**（文件里存 0），不要再反复重试
/// - `None`：从没探测过（新文件 / 升级前生成的老缓存）
fn read_duration(src: &str, cache: &Path) -> Option<Option<u32>> {
    let text = std::fs::read_to_string(duration_file(src, cache)).ok()?;
    let v: u32 = text.trim().parse().ok()?;
    Some(if v == 0 { None } else { Some(v) })
}

/// 写边车时长（0 = 探测过但读不到）。失败静默 —— 下次加载会再探一次。
fn write_duration(src: &str, cache: &Path, secs: u32) {
    let _ = std::fs::write(duration_file(src, cache), secs.to_string());
}

/// 确保时长已采集：有边车直接读，没有就**只读元数据**探一次并落盘。
///
/// 只给后台 worker 调用 —— 探测要打开视频文件（10~30ms/个），
/// 绝不能放进列表命令的同步路径。
fn ensure_duration(src: &str, cache: &Path) -> Option<u32> {
    match read_duration(src, cache) {
        Some(v) => v,
        None => {
            let secs = crate::video_thumbs::probe_duration(src).unwrap_or(0);
            write_duration(src, cache, secs);
            if secs > 0 {
                Some(secs)
            } else {
                None
            }
        }
    }
}

/// 为一张壁纸生成缩略图缓存（JPEG q85 快速编码），
/// 返回 `(列表可用的 thumb 路径, 视频时长)`。
///
/// - 已有 jpg/webp 缓存 -> 直接返回（**视频顺带确保时长已采集**，见 `ensure_duration`）
/// - 长边 <= THUMB_SIZE 的小图 -> 跳过生成，直接以原图路径作为 thumb（不写缓存）
/// - 其余 -> 解码 -> thumbnail(256) -> JPEG q85 落盘 {fnv}.jpg
/// - 解码/编码失败（损坏、不支持格式）-> None，仅影响该条，不影响整体
fn ensure_thumb_generate(src: &str, cache: &Path) -> Option<(String, Option<u32>)> {
    let is_video = is_video_file(src);
    if let Some(t) = cached_thumb(src, cache) {
        // 缓存命中不等于时长已采集：升级前生成的视频缩略图没有 `.dur` 边车。
        // 这里补一次只读元数据的探测，补完写盘，之后永远走缓存。
        let duration = if is_video {
            ensure_duration(src, cache)
        } else {
            None
        };
        return Some((t, duration));
    }
    // 视频：走 Media Foundation 取首帧。
    // 前端无法做这件事（canvas 被 asset:// 跨源污染，toDataURL 抛 SecurityError），
    // 故必须在后端生成，与图片缩略图共用缓存目录/命名规则。
    if is_video {
        let target = cache.join(thumb_file_name(src, THUMB_EXT));
        // 首帧与时长来自同一次打开，顺手把时长落盘，省掉一次单独的探测
        if let Ok(dur) = crate::video_thumbs::generate_video_thumb(src, &target) {
            write_duration(src, cache, dur);
            let duration = if dur > 0 { Some(dur) } else { None };
            return Some((target.to_string_lossy().replace('/', "\\"), duration));
        }
        return None;
    }
    let reader = ImageReader::open(src).ok()?;
    let dims = reader.into_dimensions().ok()?;
    // 小图无需缩放：直接以原图路径作为 thumb（列表加载原图，避免解码/落盘）
    if dims.0.max(dims.1) <= THUMB_SIZE {
        return Some((src.to_string(), None));
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
    Some((target.to_string_lossy().replace('/', "\\"), None))
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
) -> Arc<Mutex<HashMap<String, Option<(String, Option<u32>)>>>> {
    let queue: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(batch.iter().cloned().collect()));
    let results: Arc<Mutex<HashMap<String, Option<(String, Option<u32>)>>>> =
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
            if let Some((t, d)) = &thumb {
                let payload = ThumbnailUpdatedPayload {
                    path: src.clone(),
                    thumb: t.clone(),
                    duration: *d,
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
fn wait_prefetch(
    batch: &[String],
    results: &Arc<Mutex<HashMap<String, Option<(String, Option<u32>)>>>>,
) {
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

/// 批量构造列表条目（首屏优先 + 后台渐进），并补齐排序元信息。
///
/// `cache` 为 None（缓存目录不可解析）时返回空缩略图条目，不影响列表展示。
/// 返回条目顺序与输入一致；thumb 为空表示缩略图后台生成中或生成失败，
/// 前端以占位显示，成功完成后经事件收到更新。
pub fn make_entries(
    app: &tauri::AppHandle,
    paths: Vec<String>,
    cache: Option<&Path>,
) -> Vec<WallpaperEntry> {
    let mut entries = make_entries_inner(app, paths, cache);
    fill_sort_meta(&mut entries, cache);
    entries
}

/// 补齐排序元信息（修改时间 / 文件大小）与视频时长。
///
/// 单独遍历一遍，而不是在下面 4 个构造点各自读：构造点分散在快/慢路径上，
/// 集中补一次不会漏，也不用把每个 `WallpaperEntry { .. }` 都改一遍。
/// `fs::metadata` 失败就留 None —— 排序键缺失不该让整个列表失败。
///
/// **时长只读 `.dur` 边车，绝不在这里探测**：探测要打开视频文件（10~30ms/个），
/// 放在列表命令里会让每一次刷新都变慢。没边车的由后台 worker 补（见 `make_entries_inner`）。
fn fill_sort_meta(entries: &mut [WallpaperEntry], cache: Option<&Path>) {
    for e in entries.iter_mut() {
        if let Ok(md) = std::fs::metadata(&e.path) {
            e.size = Some(md.len());
            e.mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
        }
        if e.duration.is_none() {
            if let (Some(cache), true) = (cache, is_video_file(&e.path)) {
                if let Some(Some(secs)) = read_duration(&e.path, cache) {
                    e.duration = Some(secs);
                }
            }
        }
    }
}

fn make_entries_inner(
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
                mtime: None,
                size: None,
                duration: None,
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
                // 缩略图有了，但**时长还没采集过**（升级前生成的缓存没有 `.dur` 边车）：
                // 仍要进后台队列跑一次"只读元数据"的探测，否则这批老视频永远没有角标。
                // 采集过一次后边车存在，之后就再也不会进这个分支。
                if is_video_file(p) && read_duration(p, cache).is_none() {
                    missing.push(p.clone());
                }
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
                mtime: None,
                size: None,
                // 由 fill_sort_meta 统一从边车补，这里不重复读
                duration: None,
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
                mtime: None,
                size: None,
                duration: None,
            })
            .collect();
    }

    // 启动后台批次，命令内仅等待首屏窗口部分
    let results = spawn_thumb_batch(app.clone(), cache.to_path_buf(), &batch);
    wait_prefetch(&batch, &results);

    paths
        .into_iter()
        .map(|p| {
            let (thumb, duration) = match results.lock().unwrap().get(&p) {
                Some(Some((t, d))) => (Some(t.clone()), *d),
                // batch 内路径原本全部缺失缓存；此处保留缓存命中值兜底
                Some(None) | None => (thumb_by_path.get(&p).cloned(), None),
            };
            WallpaperEntry {
                path: p.clone(),
                thumb: thumb.unwrap_or_default(),
                mtime: None,
                size: None,
                duration,
            }
        })
        .collect()
}

/// 删除原图时同步删除其缩略图缓存（jpg 新格式 + webp 历史格式 + 时长边车，尽力而为）
pub fn delete_thumb(src: &str, cache: &Path) {
    if !cache.is_dir() {
        return;
    }
    for ext in [THUMB_EXT, LEGACY_THUMB_EXT, DURATION_EXT] {
        let p = cache.join(thumb_file_name(src, ext));
        if p.is_file() {
            let _ = std::fs::remove_file(p);
        }
    }
}
