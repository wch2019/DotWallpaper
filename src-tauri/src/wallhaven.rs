//! Wallhaven 在线壁纸后端实现。
//!
//! 与 Pixabay 的关键差异：
//!
//! - **不需要 API key**：SFW 公开接口即可用，限流 45 次/分钟（带 key 可到 90）；
//! - **没有视频**，只有静态图片，所以没有"图片 / 视频"两套接口那套分叉；
//! - **id 是 6 位字母数字**（如 `4x16yv`），不是纯数字 —— 落盘命名与扫描正则
//!   必须按这个口径写，照抄 Pixabay 的"全数字"判断会让所有图都扫不到；
//! - **分页固定 24 条/页**（接口没有 `per_page` 参数），翻页只看 `meta.last_page`；
//! - `sorting=random` 时**必须复用第一页返回的 `meta.seed`**，否则每翻一页都是
//!   一批全新的随机图 —— 表现为"翻页看到重复图 / 永远翻不到底"。
//!
//! # 合规口径
//!
//! Wallhaven 的图片是用户上传的，接口免费供个人使用。本模块：
//!
//! - **只请求 SFW**（`purity=100` 写死，**不提供 NSFW 开关**）。这是产品红线：
//!   桌面壁纸工具要上架商店，NSFW 内容不可接受，也不该让用户在设置里"意外打开"。
//!   即使将来加 API key，也不放开 purity。
//! - 只在用户点"设为壁纸 / 下载"时下载**一张**，不做批量抓取、不做永久热链；
//! - 列表用 Wallhaven 自己的 CDN 缩略图（`thumbs.large`，约 30KB/张），
//!   只有设为壁纸时才拉原图。
//!
//! # 缓存
//!
//! 未认证限流 45 次/分钟，用户在筛选之间来回切很容易打满。响应缓存 10 分钟，
//! 同一组参数短时间内重复调用不发请求。`seed` 也参与指纹 —— 换一批随机图
//! 必须换 seed，否则永远命中旧的那一批。

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::http;

/// Wallhaven 搜索接口基址（用户没填自定义地址时走这里）
const API_BASE: &str = "https://wallhaven.cc/api/v1/search";

/// 列表请求超时。
///
/// 共享客户端是 20 秒（见 `http::client`），那是给"拉 JSON / 下小图"定的通用值。
/// 但 Wallhaven 官方没有中国节点，国内经常是"连得上但一直不回"或"直接连不上"，
/// 20 秒足够让用户以为界面卡死。列表请求单独收紧到 8 秒：失败得快，提示得早。
const LIST_TIMEOUT: Duration = Duration::from_secs(8);

/// 响应缓存时长。
///
/// 不是接口硬性要求（Pixabay 才是 24h 合规要求），纯粹是为了扛住未认证的
/// 45 次/分钟限流：用户在分类/排序之间来回切时，同一组参数会被反复请求。
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// 列表条目 —— 按前端渲染需要裁剪，**不原样透传 API 响应**。
///
/// 字段一律 snake_case：本项目 serde 结构体没有 `rename_all`，
/// 线上格式就是 Rust 字段名，改成 camelCase 不会报错、只会让前端静默 `undefined`。
#[derive(Serialize, Clone, Debug)]
pub struct WallhavenWallpaper {
    /// 6 位字母数字 id，下载缓存文件名与「已下载」徽标的判定键
    pub id: String,
    /// 展示名。接口**没有标题**，这里用分辨率（如 `2560×1600`）——
    /// 对壁纸场景来说它比"wallhaven-4x16yv"有用得多。
    pub title: String,
    /// 原始分辨率字符串（接口原样，如 `2560x1600`）
    pub resolution: String,
    /// 分类：general / anime / people
    pub category: String,
    /// 详情页 URL（合规：让用户能回到来源页）
    pub page_url: String,
    /// 设为壁纸时下载用的原图 URL
    pub url: String,
    /// 列表缩略图 URL（`thumbs.large`）
    pub thumb: String,
    /// 原图宽度
    pub width: u32,
    /// 原图高度
    pub height: u32,
    /// 原图字节数（0 = 接口没给）
    pub file_size: u64,
    /// MIME（`image/jpeg` / `image/png`），下载时据此定扩展名
    pub file_type: String,
}

/// 分页响应
#[derive(Serialize, Clone, Debug)]
pub struct WallhavenPage {
    pub items: Vec<WallhavenWallpaper>,
    /// 本次实际返回条数
    pub count: u32,
    /// 命中总数（接口上限内）
    pub total: u32,
    pub page: u32,
    /// 服务端声明的最后一页
    pub last_page: u32,
    /// 是否还有下一页
    pub has_more: bool,
    /// `sorting=random` 时第一页返回的种子，后续页必须原样带回去
    pub seed: Option<String>,
}

/// 一次搜索的全部参数。
///
/// 没有 `key` 字段：SFW 公开接口不需要 key。将来若要支持更高的限流配额，
/// 再在这里加 `apikey` 并把 purity 固定住即可。
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// 搜索关键词（支持 Wallhaven 的 tag 语法，如 `id:4x16yv`、`+city`）
    pub query: String,
    /// 分类位掩码：`111` = 全部，`100` 常规，`010` 动漫，`001` 人物
    pub categories: String,
    /// 排序：date_added / relevance / random / views / favorites / toplist
    pub sorting: String,
    /// 方向：desc / asc
    pub order: String,
    /// 最低分辨率，如 `1920x1080`
    pub atleast: Option<String>,
    /// 比例，如 `16x9`（多值逗号分隔）
    pub ratios: Option<String>,
    /// 颜色，6 位十六进制**不带 `#`**
    pub colors: Option<String>,
    /// 页码，从 1 开始
    pub page: u32,
    /// 随机排序的种子（第一页由接口生成，后续页复用）
    pub seed: Option<String>,
    /// 自定义接口基址（用户自建反代 / 加速地址）。
    ///
    /// 官方没有中国节点，国内访问时通时不通；社区流传的第三方镜像不可信
    /// （无授权、可能篡改内容、随时会死），所以不内置镜像，
    /// 只把选择权交给用户：`None` / 空串 → 官方地址，其余按 `resolve_api_base` 规范化。
    pub api_base: Option<String>,
    /// 自定义图片基址（替换缩略图 / 原图的 CDN 主机）。
    ///
    /// 和 `api_base` 分开是必要的：反代通常只接管了 `/api/`，
    /// 图片仍指向 `th.wallhaven.cc` / `w.wallhaven.cc` —— 于是"列表有图、缩略图全灰"。
    /// 填了就按主机替换、路径原样保留；留空 = 不动（继续走官方 CDN）。
    pub image_base: Option<String>,
}

/// 进程内缓存：`key` 是参数指纹，`value` 是响应 + 写入时刻。
type CacheMap = HashMap<String, (WallhavenPage, Instant)>;

fn cache() -> &'static Mutex<CacheMap> {
    static CACHE: OnceLock<Mutex<CacheMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 归一化分类掩码。
///
/// 只接受恰好 3 位、每位是 `0`/`1` 的字符串；其它一律回退 `111`（全部）。
/// 写错不报错、只是"分类筛选静默失效"是最难查的一类 bug，所以在入口处收口。
fn norm_categories(raw: &str) -> String {
    let s = raw.trim();
    if s.len() == 3 && s.bytes().all(|b| b == b'0' || b == b'1') {
        s.to_string()
    } else {
        "111".to_string()
    }
}

/// 实际下发的排序。
///
/// `relevance` 在没有关键词时接口会报错（"relevance requires a query"），
/// 这里退回 `date_added` —— 前端不必为"空搜索时禁用相关排序"再写一套联动。
fn effective_sorting(q: &Query) -> String {
    let s = q.sorting.trim();
    if s == "relevance" && q.query.trim().is_empty() {
        return "date_added".to_string();
    }
    match s {
        "date_added" | "relevance" | "random" | "views" | "favorites" | "toplist" => s.to_string(),
        _ => "date_added".to_string(),
    }
}

/// 解析实际请求地址（**纯函数，不联网可测**）。
///
/// 默认官方地址；用户填了自定义基址就按下面的规则规范化：
///
/// | 用户填的 | 实际请求 |
/// |---|---|
/// | 空 / 非 `http(s)://` 开头 | 官方地址（填错不能让整个源不可用） |
/// | `https://host` / `https://host/prefix` | 补 `/api/v1/search` |
/// | `https://host/api/v1` | 补 `/search` |
/// | `https://host/.../search` | 原样使用（反代完整接管了路径） |
///
/// 允许 `http://` 是因为很多人会在本机或内网搭一个明文反代。
fn resolve_api_base(custom: Option<&str>) -> String {
    let Some(raw) = custom.map(str::trim).filter(|s| !s.is_empty()) else {
        return API_BASE.to_string();
    };
    if !(raw.starts_with("https://") || raw.starts_with("http://")) {
        return API_BASE.to_string();
    }
    let base = raw.trim_end_matches('/');
    if base.ends_with("/search") {
        base.to_string()
    } else if base.ends_with("/api/v1") {
        format!("{base}/search")
    } else {
        format!("{base}/api/v1/search")
    }
}

/// 解析图片基址（**纯函数，不联网可测**）。
///
/// 只有"填了且看起来是个 http(s) 地址"才返回 `Some`；其余一律 `None`（不替换）。
/// 不猜、不从 `api_base` 推导：反代常常只接管了 `/api/`，
/// 自动推导会把本来能走的官方 CDN 也一起改坏。
fn resolve_image_base(custom: Option<&str>) -> Option<String> {
    let raw = custom.map(str::trim).filter(|s| !s.is_empty())?;
    if !(raw.starts_with("https://") || raw.starts_with("http://")) {
        return None;
    }
    Some(raw.trim_end_matches('/').to_string())
}

/// 把 CDN 主机换成用户的反代（路径原样保留）。
///
/// 只认这两个主机：`th.wallhaven.cc`（缩略图 `/lg/`、`/orig/`、`/small/`）与
/// `w.wallhaven.cc`（原图 `/full/`）。两条路径前缀不重叠，所以同一个反代域名
/// 可以同时服务它们，用户只需填一个地址。
///
/// 已经是自定义地址的 URL 原样返回 ⇒ **幂等**，重复调用不会二次替换。
fn rewrite_cdn(url: &str, base: &str) -> String {
    const HOSTS: [&str; 4] = [
        "https://th.wallhaven.cc",
        "http://th.wallhaven.cc",
        "https://w.wallhaven.cc",
        "http://w.wallhaven.cc",
    ];
    for host in HOSTS {
        if let Some(rest) = url.strip_prefix(host) {
            return format!("{base}{rest}");
        }
    }
    url.to_string()
}

/// 就地替换一页里的图片地址（缩略图 + 原图）。
///
/// `page_url`（详情页）**不动** —— 那是给用户点开看的，反代一般没代理网页本身。
fn apply_image_base(page: &mut WallhavenPage, custom: Option<&str>) {
    let Some(base) = resolve_image_base(custom) else {
        return;
    };
    for item in page.items.iter_mut() {
        item.thumb = rewrite_cdn(&item.thumb, &base);
        item.url = rewrite_cdn(&item.url, &base);
    }
}

/// 把网络层错误翻译成用户能照着做的中文。
/// reqwest 的原始文案（`error sending request for url (...)`）对普通用户等于没说。
/// 这里把"超时"和"连不上"分开，因为下一步动作不同：超时是"再试一次"，
/// 连不上是"这个地址在当前网络就是不通"。
fn describe_request_error(e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!(
            "连接 Wallhaven 超时（{} 秒）。该站不在国内，可稍后重试；\
             长期连不上可在设置里为 Wallhaven 填自定义地址。",
            LIST_TIMEOUT.as_secs()
        )
    } else if e.is_connect() {
        "无法连接 Wallhaven。请检查网络，或在设置里为 Wallhaven 填写自定义地址。".to_string()
    } else {
        format!("请求 Wallhaven 失败：{e}")
    }
}

/// 组装请求参数（**纯函数，不联网可测**）。
///
/// 单独抽出来是为了能被单测锁死：`purity` 一旦被漏下发，NSFW 就会混进列表；
/// `seed` 一旦漏下发，随机排序翻页会重复。这两类问题界面上都"看起来正常"。
fn build_params(q: &Query) -> Vec<(&'static str, String)> {
    let mut params: Vec<(&'static str, String)> = vec![
        ("categories", norm_categories(&q.categories)),
        // 【红线】SFW 写死。不要做成参数、不要读用户偏好。
        ("purity", "100".to_string()),
        ("sorting", effective_sorting(q)),
        (
            "order",
            if q.order.trim() == "asc" { "asc" } else { "desc" }.to_string(),
        ),
        ("page", q.page.max(1).to_string()),
    ];

    let query = q.query.trim();
    if !query.is_empty() {
        params.push(("q", query.to_string()));
    }
    if let Some(v) = q.atleast.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("atleast", v.to_string()));
    }
    if let Some(v) = q.ratios.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("ratios", v.to_string()));
    }
    if let Some(v) = q.colors.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        // 接口要求不带 `#`；前端可能从色板带过来，这里统一剥掉
        params.push(("colors", v.trim_start_matches('#').to_string()));
    }
    if let Some(v) = q.seed.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("seed", v.to_string()));
    }
    params
}

/// 参数指纹 —— **顺序固定**，不能用 HashMap 迭代（顺序随机会让同一组参数算出
/// 不同指纹，缓存永远不命中）。
///
/// ⚠️ **新增任何会改变请求结果的字段都必须加进来**。漏了不会报错，
/// 只会让两种不同请求命中同一份缓存 —— 表现为"改了设置结果不变"。
/// `seed` 尤其不能漏：随机排序换一批图靠的就是它。
/// `api_base` 同理：换地址后若还命中旧缓存，用户会以为"填了没用"。
fn fingerprint(q: &Query) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        resolve_api_base(q.api_base.as_deref()),
        q.query.trim(),
        norm_categories(&q.categories),
        effective_sorting(q),
        q.order.trim(),
        q.atleast.as_deref().unwrap_or(""),
        q.ratios.as_deref().unwrap_or(""),
        q.colors.as_deref().unwrap_or(""),
        q.page.max(1),
        q.seed.as_deref().unwrap_or(""),
    )
}

/// 拉取一页搜索结果。
///
/// 命中 10 分钟缓存则直接返回，不发请求 —— 未认证限流 45 次/分钟，
/// 用户在筛选之间来回切很容易打满。
pub fn fetch_page(q: &Query) -> Result<WallhavenPage, String> {
    let fp = fingerprint(q);
    if let Ok(guard) = cache().lock() {
        if let Some((page, at)) = guard.get(&fp) {
            if at.elapsed() < CACHE_TTL {
                let mut hit = page.clone();
                apply_image_base(&mut hit, q.image_base.as_deref());
                return Ok(hit);
            }
        }
    }

    let params = build_params(q);
    let api_base = resolve_api_base(q.api_base.as_deref());
    let resp = http::client()
        .get(&api_base)
        .query(&params)
        .timeout(LIST_TIMEOUT)
        .send()
        .map_err(describe_request_error)?;

    let status = resp.status();
    if !status.is_success() {
        // Wallhaven 的 4xx 响应体是纯文本错误描述，比状态码有用得多
        let body = resp.text().unwrap_or_default();
        let body = body.trim();
        return Err(match status.as_u16() {
            400 => format!(
                "Wallhaven 拒绝了请求：{}",
                if body.is_empty() { "参数不合法" } else { body }
            ),
            403 => "Wallhaven 拒绝了请求（可能触发了风控，请稍后再试）".to_string(),
            429 => "Wallhaven 请求过于频繁（未认证限 45 次/分钟），请稍后再试".to_string(),
            code => format!("Wallhaven 返回 HTTP {code}"),
        });
    }

    let text = resp
        .text()
        .map_err(|e| format!("读取 Wallhaven 响应失败：{e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 Wallhaven 响应失败：{e}"))?;

    let mut page = parse_page(&json, q.page.max(1));

    if let Ok(mut guard) = cache().lock() {
        // 顺手清掉过期项，避免长期运行后缓存无限膨胀
        guard.retain(|_, (_, at)| at.elapsed() < CACHE_TTL);
        // 缓存里存的是**原始 URL**：图片地址只影响展示，不影响请求本身。
        // 放进指纹会让同一份响应按图片地址缓存多份，纯属浪费。
        guard.insert(fp, (page.clone(), Instant::now()));
    }

    apply_image_base(&mut page, q.image_base.as_deref());
    Ok(page)
}

/// 解析一页响应（**纯函数，不联网可测**）。
fn parse_page(json: &serde_json::Value, page: u32) -> WallhavenPage {
    let meta = json.get("meta");
    let total = meta
        .and_then(|m| m.get("total"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let last_page = meta
        .and_then(|m| m.get("last_page"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let current_page = meta
        .and_then(|m| m.get("current_page"))
        .and_then(|v| v.as_u64())
        .map(|v| v.min(u32::MAX as u64) as u32)
        .unwrap_or(page);
    let seed = meta
        .and_then(|m| m.get("seed"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut items: Vec<WallhavenWallpaper> = Vec::new();
    if let Some(list) = json.get("data").and_then(|v| v.as_array()) {
        for d in list {
            let Some(id) = d.get("id").and_then(|v| v.as_str()).map(str::trim) else {
                continue;
            };
            let url = d.get("path").and_then(|v| v.as_str()).unwrap_or("").trim();
            if id.is_empty() || url.is_empty() {
                // 没有原图地址的条目用不了，丢弃而不是留个点不动的空卡片
                continue;
            }

            let width = d
                .get("dimension_x")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let height = d
                .get("dimension_y")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let resolution = d
                .get("resolution")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{width}×{height}"));

            let thumbs = d.get("thumbs");
            let thumb = ["large", "original", "small"]
                .iter()
                .find_map(|k| {
                    thumbs
                        .and_then(|t| t.get(*k))
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or("")
                .to_string();

            items.push(WallhavenWallpaper {
                id: id.to_string(),
                title: resolution.replace('x', "×"),
                resolution,
                category: d
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                page_url: d
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: url.to_string(),
                thumb,
                width,
                height,
                file_size: d.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
                file_type: d
                    .get("file_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }

    let count = items.len() as u32;
    // 空页一定没有下一页 —— 否则会出现"翻到底还在转圈"的死循环。
    // 另外用 last_page 而不是 total：接口的 total 可能被上限截断。
    let has_more = count > 0 && current_page < last_page;

    WallhavenPage {
        items,
        count,
        total,
        page: current_page,
        last_page,
        has_more,
        seed,
    }
}

// ---------- 下载与本地扫描 ----------

/// Wallhaven 下载目录：`<壁纸目录>\Wallhaven`。
///
/// 与必应 / Pixabay 同样做成**派生**而非独立配置 —— 独立目录会在用户改壁纸目录
/// 之后与「本地」列表脱钩（文件落在壁纸目录之外，本地选项卡看不到）。
pub fn download_dir(local_dir: Option<String>) -> PathBuf {
    crate::resolve_save_dir(local_dir).join("Wallhaven")
}

/// id 只允许 ASCII 字母数字（Wallhaven 的 id 形如 `4x16yv`）。
///
/// **必须校验**：id 会直接拼进文件名，放任它进路径就等于把 `../` 这种输入
/// 变成写文件到目录之外。
fn safe_id(id: &str) -> Option<&str> {
    let s = id.trim();
    if !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Some(s)
    } else {
        None
    }
}

/// 按 MIME / URL 推断扩展名。
///
/// 接口给 `file_type`（`image/jpeg`），但老数据/异常响应可能为空，
/// 所以再用 URL 后缀兜底。写死 `.jpg` 会让 png 被系统壁纸接口按错误格式解码。
fn ext_of(file_type: &str, url: &str) -> &'static str {
    let mime = file_type.trim().to_ascii_lowercase();
    if mime.contains("png") {
        return "png";
    }
    if mime.contains("webp") {
        return "webp";
    }
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    if path.ends_with(".png") {
        "png"
    } else if path.ends_with(".webp") {
        "webp"
    } else {
        "jpg"
    }
}

/// 下载原图到 `dir` 并返回本地绝对路径。
///
/// 同名文件已存在时直接复用，不重复走网络（同一张图反复点"设为壁纸"很常见）。
/// 先写 `.part` 再改名，避免半截文件被后续流程当成完整图片使用。
pub fn download_wallpaper(
    url: &str,
    id: &str,
    file_type: &str,
    dir: &Path,
) -> Result<String, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("壁纸地址无效".into());
    }
    let id = safe_id(id).ok_or_else(|| "Wallhaven 图片 id 无效".to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("创建下载目录失败 {}: {e}", dir.display()))?;

    let ext = ext_of(file_type, url);
    let dest: PathBuf = dir.join(format!("Wallhaven_{id}.{ext}"));

    if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(normalize(&dest));
    }

    let resp = http::get(url, "Wallhaven 图片")?;
    if !resp.status().is_success() {
        return Err(format!("下载 Wallhaven 图片失败：HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("读取 Wallhaven 图片数据失败：{e}"))?;
    // 正常原图数百 KB ~ 十几 MB，小于 4KB 视为异常响应（错误页/占位图）
    if bytes.len() < 4096 {
        return Err("下载的图片数据异常（文件过小）".into());
    }

    let tmp = dir.join(format!("Wallhaven_{id}.{ext}.part"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入壁纸文件失败：{e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("保存壁纸文件失败：{e}")
    })?;

    Ok(normalize(&dest))
}

/// 从 `Wallhaven_{id}.{jpg|jpeg|png|webp}` 解析图片 id；不匹配返回 None。
///
/// 只认自家命名 —— 用户自己的 `IMG_20260917.jpg` 之类绝不能误判成 Wallhaven 图。
/// **id 是字母数字**，不能沿用 Pixabay 的"全数字"判断。
fn parse_cached_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_prefix("wallhaven_")?;
    let stem = stem
        .strip_suffix(".jpg")
        .or_else(|| stem.strip_suffix(".jpeg"))
        .or_else(|| stem.strip_suffix(".png"))
        .or_else(|| stem.strip_suffix(".webp"))?;
    if !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Some(stem.to_string())
    } else {
        None
    }
}

/// 在**本地壁纸目录**里递归找已下载的 Wallhaven 图，返回 `(id, 本地绝对路径)`，按 id 倒序。
///
/// 与必应 / Pixabay 同理：扫的是本地壁纸目录而非下载目录，这样「已下载」徽标与
/// 「本地」选项卡能否看到该文件共用同一个事实来源，两者永远不会互相矛盾。
pub fn scan_downloaded(dir: &Path) -> Vec<(String, String)> {
    let Ok(files) = crate::wallpaper::walk_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for path in files {
        let Some(name) = Path::new(&path).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = parse_cached_name(name) else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push((id, normalize(Path::new(&path))));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// 路径统一为 Windows 反斜杠风格（与项目其它返回保持一致）
fn normalize(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn q_default() -> Query {
        Query {
            query: "city".into(),
            categories: "111".into(),
            sorting: "relevance".into(),
            order: "desc".into(),
            page: 1,
            ..Default::default()
        }
    }

    #[test]
    fn purity_is_always_sfw() {
        let params = build_params(&q_default());
        let purity = params
            .iter()
            .find(|(k, _)| *k == "purity")
            .map(|(_, v)| v.as_str());
        assert_eq!(purity, Some("100"));
    }

    #[test]
    fn relevance_without_query_falls_back_to_date_added() {
        let mut q = q_default();
        q.query = "   ".into();
        let params = build_params(&q);
        let sorting = params
            .iter()
            .find(|(k, _)| *k == "sorting")
            .map(|(_, v)| v.as_str());
        assert_eq!(sorting, Some("date_added"));
    }

    #[test]
    fn invalid_categories_fall_back_to_all() {
        let mut q = q_default();
        q.categories = "10".into();
        let params = build_params(&q);
        let cats = params
            .iter()
            .find(|(k, _)| *k == "categories")
            .map(|(_, v)| v.as_str());
        assert_eq!(cats, Some("111"));
    }

    #[test]
    fn seed_is_sent_for_pagination() {
        let mut q = q_default();
        q.sorting = "random".into();
        q.seed = Some("abc123".into());
        let params = build_params(&q);
        let seed = params
            .iter()
            .find(|(k, _)| *k == "seed")
            .map(|(_, v)| v.as_str());
        assert_eq!(seed, Some("abc123"));
    }

    #[test]
    fn colors_strip_hash() {
        let mut q = q_default();
        q.colors = Some("#3b82f6".into());
        let params = build_params(&q);
        let c = params
            .iter()
            .find(|(k, _)| *k == "colors")
            .map(|(_, v)| v.as_str());
        assert_eq!(c, Some("3b82f6"));
    }

    #[test]
    fn parses_page_and_has_more() {
        let json = json!({
            "data": [
                {
                    "id": "4x16yv",
                    "url": "https://wallhaven.cc/w/4x16yv",
                    "path": "https://w.wallhaven.cc/full/4x/wallhaven-4x16yv.jpg",
                    "dimension_x": 2560,
                    "dimension_y": 1600,
                    "resolution": "2560x1600",
                    "category": "general",
                    "file_size": 1179008,
                    "file_type": "image/jpeg",
                    "thumbs": {
                        "large": "https://th.wallhaven.cc/lg/4x/4x16yv.jpg",
                        "original": "https://th.wallhaven.cc/orig/4x/4x16yv.jpg",
                        "small": "https://th.wallhaven.cc/small/4x/4x16yv.jpg"
                    }
                }
            ],
            "meta": {
                "current_page": 1,
                "last_page": 3,
                "per_page": 24,
                "total": 60,
                "query": "city",
                "seed": null
            }
        });
        let page = parse_page(&json, 1);
        assert_eq!(page.count, 1);
        assert_eq!(page.total, 60);
        assert_eq!(page.last_page, 3);
        assert!(page.has_more);
        assert_eq!(page.items[0].id, "4x16yv");
        assert_eq!(page.items[0].title, "2560×1600");
        assert_eq!(page.items[0].thumb, "https://th.wallhaven.cc/lg/4x/4x16yv.jpg");
        assert_eq!(page.items[0].file_type, "image/jpeg");
    }

    #[test]
    fn last_page_has_no_more() {
        let json = json!({
            "data": [{ "id": "abc123", "path": "https://x/y.jpg" }],
            "meta": { "current_page": 3, "last_page": 3, "total": 60, "seed": "s1" }
        });
        let page = parse_page(&json, 3);
        assert!(!page.has_more);
        assert_eq!(page.seed.as_deref(), Some("s1"));
    }

    #[test]
    fn empty_page_has_no_more() {
        let json = json!({ "data": [], "meta": { "current_page": 1, "last_page": 5, "total": 100 } });
        let page = parse_page(&json, 1);
        assert_eq!(page.count, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn cached_name_accepts_alphanumeric_id() {
        assert_eq!(
            parse_cached_name("Wallhaven_4x16yv.jpg").as_deref(),
            Some("4x16yv")
        );
        assert_eq!(
            parse_cached_name("wallhaven_abc123.png").as_deref(),
            Some("abc123")
        );
        assert_eq!(parse_cached_name("Wallhaven_.jpg"), None);
        assert_eq!(parse_cached_name("IMG_20260917.jpg"), None);
        assert_eq!(parse_cached_name("Wallhaven_4x16yv.jpg.part"), None);
    }

    #[test]
    fn ext_prefers_mime_then_url() {
        assert_eq!(ext_of("image/png", "https://x/y.jpg"), "png");
        assert_eq!(ext_of("", "https://x/y.webp"), "webp");
        assert_eq!(ext_of("", "https://x/y.jpg"), "jpg");
    }

    #[test]
    fn safe_id_rejects_path_tricks() {
        assert!(safe_id("4x16yv").is_some());
        assert!(safe_id("../evil").is_none());
        assert!(safe_id("a/b").is_none());
        assert!(safe_id("").is_none());
    }

    #[test]
    fn api_base_defaults_to_official() {
        assert_eq!(resolve_api_base(None), API_BASE);
        assert_eq!(resolve_api_base(Some("   ")), API_BASE);
        // 填错（少了协议头）不能让整个源不可用，静默回退官方
        assert_eq!(resolve_api_base(Some("wallhaven.cc")), API_BASE);
        assert_eq!(resolve_api_base(Some("ftp://x/y")), API_BASE);
    }

    #[test]
    fn api_base_appends_path() {
        assert_eq!(
            resolve_api_base(Some("https://proxy.example.com")),
            "https://proxy.example.com/api/v1/search"
        );
        // 尾斜杠要吃掉，否则拼出 `//api/v1/search`
        assert_eq!(
            resolve_api_base(Some("https://proxy.example.com/")),
            "https://proxy.example.com/api/v1/search"
        );
        assert_eq!(
            resolve_api_base(Some("https://proxy.example.com/wh")),
            "https://proxy.example.com/wh/api/v1/search"
        );
        assert_eq!(
            resolve_api_base(Some("https://proxy.example.com/api/v1")),
            "https://proxy.example.com/api/v1/search"
        );
        // 反代完整接管路径时原样使用，不要再拼一遍
        assert_eq!(
            resolve_api_base(Some("https://proxy.example.com/api/v1/search")),
            "https://proxy.example.com/api/v1/search"
        );
        // 本机明文反代要允许
        assert_eq!(
            resolve_api_base(Some("http://127.0.0.1:8787")),
            "http://127.0.0.1:8787/api/v1/search"
        );
    }

    #[test]
    fn api_base_is_part_of_cache_key() {
        let official = q_default();
        let mut custom = q_default();
        custom.api_base = Some("https://proxy.example.com".into());
        assert_ne!(fingerprint(&official), fingerprint(&custom));
    }

    fn page_with_cdn_urls() -> WallhavenPage {
        let json = json!({
            "data": [{
                "id": "4x16yv",
                "url": "https://wallhaven.cc/w/4x16yv",
                "path": "https://w.wallhaven.cc/full/4x/wallhaven-4x16yv.jpg",
                "resolution": "2560x1600",
                "thumbs": {
                    "large": "https://th.wallhaven.cc/lg/4x/4x16yv.jpg",
                    "original": "https://th.wallhaven.cc/orig/4x/4x16yv.jpg",
                    "small": "https://th.wallhaven.cc/small/4x/4x16yv.jpg"
                }
            }],
            "meta": { "current_page": 1, "last_page": 1, "total": 1 }
        });
        parse_page(&json, 1)
    }

    #[test]
    fn image_base_rewrites_both_cdn_hosts() {
        let mut page = page_with_cdn_urls();
        apply_image_base(&mut page, Some("https://img.example.com"));
        // 缩略图走 th.，原图走 w.，两个都要换；路径必须原样保留
        assert_eq!(
            page.items[0].thumb,
            "https://img.example.com/lg/4x/4x16yv.jpg"
        );
        assert_eq!(
            page.items[0].url,
            "https://img.example.com/full/4x/wallhaven-4x16yv.jpg"
        );
        // 详情页不动：反代一般没代理网页本身
        assert_eq!(page.items[0].page_url, "https://wallhaven.cc/w/4x16yv");
    }

    #[test]
    fn image_base_is_noop_when_empty_or_invalid() {
        let mut page = page_with_cdn_urls();
        let before = page.items[0].thumb.clone();
        apply_image_base(&mut page, None);
        assert_eq!(page.items[0].thumb, before);
        apply_image_base(&mut page, Some("   "));
        assert_eq!(page.items[0].thumb, before);
        apply_image_base(&mut page, Some("img.example.com"));
        assert_eq!(page.items[0].thumb, before);
    }

    #[test]
    fn image_base_rewrite_is_idempotent() {
        let mut page = page_with_cdn_urls();
        apply_image_base(&mut page, Some("https://img.example.com/"));
        let once = page.items[0].thumb.clone();
        // 尾斜杠被吃掉，不会拼出 `//lg/`
        assert_eq!(once, "https://img.example.com/lg/4x/4x16yv.jpg");
        apply_image_base(&mut page, Some("https://img.example.com/"));
        assert_eq!(page.items[0].thumb, once);
    }

    #[test]
    fn image_base_leaves_foreign_hosts_alone() {
        let mut page = page_with_cdn_urls();
        page.items[0].thumb = "https://other.cdn/x.jpg".into();
        apply_image_base(&mut page, Some("https://img.example.com"));
        assert_eq!(page.items[0].thumb, "https://other.cdn/x.jpg");
    }
}
