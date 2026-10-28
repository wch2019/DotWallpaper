//! Pixabay 在线壁纸后端实现。
//!
//! 与「必应」的关键差异：必应是**日期流**（每天一张，按时间往前翻），
//! Pixabay 是**搜索式图库**（关键词 / 分类 / 排序 + 分页）。
//!
//! # 授权口径（Pixabay Content License + API Terms）
//!
//! - 图片可免费商用、可修改、无需署名，但**不得以独立文件形式再分发**
//!   ⇒ 本模块只做"用户点一张、下载一张"，**不提供批量下载入口**；
//! - API 响应**必须缓存 24 小时**（见 `CACHE_TTL`，缓存在调用方实现）；
//! - 返回的图片 URL 只能"临时展示搜索结果"，**禁止永久热链**
//!   ⇒ 列表用远程缩略图，只有"设为壁纸"时才下载到本地；
//! - 搜索结果展示时必须标明来源是 Pixabay（前端负责，不在本模块）。
//!
//! # 分辨率降级（本模块最关键的一处）
//!
//! 免费 key 只返回 `largeImageURL`（1280px）；`fullHDURL`（1920）与 `imageURL`（原图）
//! 需要单独申请 full API access。
//!
//! 因此取图**必须按字段存在与否依次降级**。写死 `imageURL` 的后果是：
//! 免费 key 下该字段是 `undefined`，`as_str()` 拿到 `None`，最终拼出空串，
//! 下载阶段才会失败，而报错信息与"没有权限"毫无关系 —— 排查时会被彻底带偏。
//!
//! 降级顺序：`imageURL`（原图）→ `fullHDURL`（1920）→ `largeImageURL`（1280）。
//! 三档全空说明这张图不可用，直接丢弃。
//!
//! # 视频（`/api/videos/`）
//!
//! 与图片是**两套完全不同的响应结构**，不能复用同一条解析：
//!
//! - 图片的 URL 是平铺字段（`imageURL` / `fullHDURL` / `largeImageURL`），
//!   视频是嵌套的 `videos.{large,medium,small,tiny}.{url,width,height,size,thumbnail}`；
//! - 图片的降级是"**权限**降级"（免费 key 拿不到高清），
//!   视频的降级是"**素材**降级"（老素材没有 4K 版，`large.url` 是空串、`size` 为 0），
//!   与 key 权限无关 —— 所以视频没有"degraded"这个整页标记，
//!   而是逐条回报**实际拿到的档位**（`quality`），由前端如实展示。
//! - 授权比图片宽松：文档明确 "Videos may be embedded directly in your applications"，
//!   即允许直接嵌入播放。但本项目仍**先预览、设壁纸时才下载** ——
//!   既省流量，也让桌面播放用本地文件（`<video>` 播远程大文件在 WebView2 上更容易卡）。

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::http;

/// Pixabay 图片搜索接口基址
const API_BASE: &str = "https://pixabay.com/api/";

/// Pixabay 视频搜索接口基址
const VIDEO_API_BASE: &str = "https://pixabay.com/api/videos/";

/// 响应缓存时长。**Pixabay 明文要求缓存 24 小时**，不是性能优化而是合规要求。
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// 单次搜索 Pixabay 服务端最多返回 500 条（超出部分翻页也拿不到）
const MAX_TOTAL_HITS: u32 = 500;

/// 列表条目 —— 按前端渲染需要裁剪，**不原样透传 API 响应**。
///
/// 字段一律 snake_case：本项目 serde 结构体没有 `rename_all`，
/// 线上格式就是 Rust 字段名，改成 camelCase 不会报错、只会让前端静默 `undefined`。
#[derive(Serialize, Clone, Debug)]
pub struct PixabayWallpaper {
    /// Pixabay 图片 id，下载缓存文件名与「已下载」徽标的判定键
    pub id: u64,
    /// 标题。Pixabay **没有 title 字段**，取 tags 的第一段
    pub title: String,
    /// 完整 tags（前端可用于 `title` 属性悬浮提示）
    pub tags: String,
    /// 作者名 —— 合规必需：卡片上要显示
    pub user: String,
    /// 作者主页 URL —— 合规必需：可点击回到 Pixabay
    pub user_url: String,
    /// 图片详情页 URL —— 合规必需
    pub page_url: String,
    /// 设为壁纸时下载用的**最大可用**原图 URL（已按三档降级）
    pub url: String,
    /// 列表缩略图 URL（`webformatURL` 的 340 变体，约 20KB/张）
    pub thumb: String,
    /// 原图宽度（Pixabay 给的是原图尺寸，与降级后拿到的 URL 无关）
    pub width: u32,
    /// 原图高度
    pub height: u32,
}

/// 分页响应
#[derive(Serialize, Clone, Debug)]
pub struct PixabayPage {
    pub items: Vec<PixabayWallpaper>,
    /// 本次实际返回条数（可能少于 `per_page`）
    pub count: u32,
    /// 服务端声明的命中总数（上限 [`MAX_TOTAL_HITS`]）
    pub total_hits: u32,
    pub page: u32,
    /// 是否还有下一页
    pub has_more: bool,
    /// 是否处于降级状态：整页没有任何 `imageURL` / `fullHDURL`
    /// ⇒ 免费 key，只能拿到 1280px。前端据此显示提示。
    pub degraded: bool,
}

/// 一次搜索的全部参数。
///
/// `key` 由**前端传入**（用户自填方案）：后端不持有、不落盘、不写日志。
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Pixabay API key（用户自填）
    pub key: String,
    /// 搜索关键词，空串 = 不筛关键词（配合 category 浏览）
    pub query: String,
    /// 分类：backgrounds / nature / travel / ...，空 = 全部
    pub category: Option<String>,
    /// 方向：horizontal / vertical / all，空 = 不筛
    pub orientation: Option<String>,
    /// 最小宽度（按**原图**宽度算，与降级后的 URL 尺寸无关），0 = 不筛
    pub min_width: Option<u32>,
    /// 最小高度，0 = 不筛。网页端"尺寸"筛选用的是精确分辨率，接口只给
    /// 最小宽/高两个下限 ⇒ 这边做成"至少 1920×1080 / 2560×1440 / 3840×2160"档位。
    pub min_height: Option<u32>,
    /// 颜色筛选（逗号分隔的多值，接口原生支持）。
    ///
    /// 取值见 Pixabay 文档：grayscale / transparent / red / orange / yellow /
    /// green / turquoise / blue / lilac / pink / white / gray / black / brown。
    /// **仅图片接口支持** —— 视频接口会静默忽略这个参数（不报错、也不生效），
    /// 所以视频查询里刻意没有这个字段。
    pub colors: Option<String>,
    /// 排序：popular / latest
    pub order: Option<String>,
    /// 页码，从 1 开始
    pub page: u32,
    /// 每页条数（Pixabay 上限 200）
    pub per_page: u32,
    /// 搜索词语言，默认 zh
    pub lang: Option<String>,
    /// 图片类型：`photo` / `illustration` / `vector` / `all`。
    ///
    /// **不能只给 photo**：Pixabay 上"有壁纸感"的素材大量是插画/3D 渲染，
    /// photo 类目多是素材库实拍（灯塔、蝴蝶、食物特写）。实测
    /// `city night + illustration` 出的是赛博朋克霓虹街道、城市天际线剪影。
    /// 空 = photo（保持旧行为，避免老前端调用时语义突变）。
    pub image_type: Option<String>,
    /// 只要官方精选（Editor's Choice）。
    ///
    /// `popular` 是按**累计浏览量**排的，老素材天然霸榜 —— 实测
    /// `city night` 前 100 条里 89 条是 2013-2017 年的图。精选能筛掉大量平庸素材
    /// （同条件下老图 48/50 → 34/50）。这是目前唯一能改善"图很老很土"的参数，
    /// 因为排序本身只有 popular / latest 两种，而 latest 对多词组合会掉相关性。
    pub editors_choice: bool,
}

/// 进程内缓存：`key` 是参数的指纹，`value` 是响应 + 写入时刻。
type CacheMap = HashMap<String, (PixabayPage, Instant)>;

fn cache() -> &'static Mutex<CacheMap> {
    static CACHE: OnceLock<Mutex<CacheMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 参数指纹 —— **顺序固定**，不能用 HashMap 迭代（顺序随机会让同一组参数算出不同指纹，
/// 缓存永远不命中）。
///
/// ⚠️ **新增任何会改变请求结果的字段都必须加进来**。漏了不会报错，
/// 只会让两种不同请求命中同一份缓存 —— 表现为"改了设置结果不变"，
/// 排查时几乎不可能想到是缓存。`lang` 就曾漏在这里。
fn fingerprint(q: &Query) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        q.key,
        q.query.trim(),
        q.category.as_deref().unwrap_or(""),
        q.orientation.as_deref().unwrap_or(""),
        q.min_width.unwrap_or(0),
        q.min_height.unwrap_or(0),
        q.colors.as_deref().unwrap_or(""),
        q.order.as_deref().unwrap_or(""),
        q.page,
        q.per_page,
        q.lang.as_deref().unwrap_or(""),
        q.image_type.as_deref().unwrap_or(""),
        q.editors_choice
    )
}

/// 拉取一页搜索结果。
///
/// 命中 24h 缓存则直接返回，不发请求 —— 既是合规要求，也顺手挡掉
/// "用户在分类之间来回切导致疯狂请求"（限流 100 次/分钟，很容易打满）。
/// 组装图片搜索的 query 参数（**纯函数，不联网可测**）。
///
/// 单独抽出来只有一个理由：能被单测锁死。`colors` / `min_height` / `orientation`
/// 这类筛选参数一旦漏下发，表现是"用户改了筛选、结果一点没变" —— 界面上
/// 一切正常、也不报错，靠手测几乎不可能发现（`lang` 就曾这样漏在指纹里）。
fn build_image_params(q: &Query) -> Vec<(&'static str, String)> {
    // 空搜索（既没关键词也没分类）= 用户在"随便逛逛"。
    //
    // 直接发无 q 请求拿到的是**全站最热门杂图**（蝴蝶、花、池塘、海），
    // 既不像壁纸也不像一个图库首页。这里只加一个 `editors_choice`。
    //
    // ⚠️ **不要再叠加 `category=backgrounds`** —— 实测反而更差：
    // 仅精选 = 老图 4/24、新图 20/24；再加 backgrounds = 老图 16/24、新图 8/24。
    // 原因是背景类目的精选池更小，很快翻到老素材。
    let browsing = q.query.trim().is_empty()
        && q.category.as_deref().map(str::trim).unwrap_or("").is_empty();

    let image_type = q
        .image_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("photo");

    let mut params: Vec<(&'static str, String)> = vec![
        ("key", q.key.trim().to_string()),
        ("image_type", image_type.to_string()),
        // 上架 Store，写死开启，不给用户关
        ("safesearch", "true".into()),
        ("lang", q.lang.clone().unwrap_or_else(|| "zh".into())),
        ("page", q.page.max(1).to_string()),
        ("per_page", q.per_page.clamp(3, 200).to_string()),
    ];

    // 精选：用户显式开启，或"闲逛"时默认开启。
    // 只在 true 时下发 —— 传 `false` 与省略等价，少一个参数少一次缓存指纹分叉。
    if q.editors_choice || browsing {
        params.push(("editors_choice", "true".into()));
    }

    let query = q.query.trim();
    if !query.is_empty() {
        params.push(("q", query.to_string()));
    }
    if let Some(c) = q.category.as_deref().filter(|s| !s.is_empty()) {
        params.push(("category", c.to_string()));
    }
    if let Some(o) = q.orientation.as_deref().filter(|s| !s.is_empty() && *s != "all") {
        params.push(("orientation", o.to_string()));
    }
    if let Some(m) = q.min_width.filter(|m| *m > 0) {
        params.push(("min_width", m.to_string()));
    }
    if let Some(m) = q.min_height.filter(|m| *m > 0) {
        params.push(("min_height", m.to_string()));
    }
    if let Some(c) = q.colors.as_deref().filter(|s| !s.is_empty()) {
        params.push(("colors", c.to_string()));
    }
    if let Some(o) = q.order.as_deref().filter(|s| !s.is_empty()) {
        params.push(("order", o.to_string()));
    }
    params
}

pub fn fetch_page(q: &Query) -> Result<PixabayPage, String> {
    let key = q.key.trim();
    if key.is_empty() {
        return Err("未填写 Pixabay API key".into());
    }

    let fp = fingerprint(q);
    if let Ok(guard) = cache().lock() {
        if let Some((page, at)) = guard.get(&fp) {
            if at.elapsed() < CACHE_TTL {
                return Ok(page.clone());
            }
        }
    }

    let params = build_image_params(q);

    let resp = http::client()
        .get(API_BASE)
        .query(&params)
        .send()
        .map_err(|e| format!("请求 Pixabay 失败：{e}"))?;

    let status = resp.status();
    if !status.is_success() {
        // Pixabay 的 4xx 响应体是纯文本错误描述，比状态码有用得多
        let body = resp.text().unwrap_or_default();
        let body = body.trim();
        return Err(match status.as_u16() {
            400 => format!(
                "Pixabay 拒绝了请求：{}",
                if body.is_empty() {
                    "API key 无效或参数不合法"
                } else {
                    body
                }
            ),
            429 => "Pixabay 请求过于频繁（限 100 次/分钟），请稍后再试".to_string(),
            code => format!("Pixabay 返回 HTTP {code}"),
        });
    }

    let text = resp
        .text()
        .map_err(|e| format!("读取 Pixabay 响应失败：{e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 Pixabay 响应失败：{e}"))?;

    let page = parse_page(&json, q.page.max(1), q.per_page.clamp(3, 200));

    if let Ok(mut guard) = cache().lock() {
        // 顺手清掉过期项，避免长期运行后缓存无限膨胀
        guard.retain(|_, (_, at)| at.elapsed() < CACHE_TTL);
        guard.insert(fp, (page.clone(), Instant::now()));
    }

    Ok(page)
}

/// 解析一页响应。
///
/// 拆成纯函数是为了**不联网就能测**降级逻辑 —— 这是本模块最容易出错的地方，
/// 而真实 key 分免费/付费两种状态，手测很难覆盖。
fn parse_page(json: &serde_json::Value, page: u32, per_page: u32) -> PixabayPage {
    let total_hits = json
        .get("totalHits")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(MAX_TOTAL_HITS as u64) as u32;

    let mut items: Vec<PixabayWallpaper> = Vec::new();
    // 任意一条带 imageURL / fullHDURL ⇒ 这个 key 有 full access
    let mut has_full_access = false;

    if let Some(hits) = json.get("hits").and_then(|v| v.as_array()) {
        for h in hits {
            let Some(id) = h.get("id").and_then(|v| v.as_u64()) else {
                continue;
            };

            let original = h.get("imageURL").and_then(|v| v.as_str()).unwrap_or("");
            let full_hd = h.get("fullHDURL").and_then(|v| v.as_str()).unwrap_or("");
            let large = h.get("largeImageURL").and_then(|v| v.as_str()).unwrap_or("");

            // 三档降级：原图 → 1920 → 1280
            let url = if !original.is_empty() {
                has_full_access = true;
                original
            } else if !full_hd.is_empty() {
                has_full_access = true;
                full_hd
            } else {
                large
            };
            if url.is_empty() {
                // 连 1280 都没有：这张用不了，丢弃而不是留个点不动的空卡片
                continue;
            }

            let tags = h.get("tags").and_then(|v| v.as_str()).unwrap_or("").trim();
            let user = h.get("user").and_then(|v| v.as_str()).unwrap_or("").trim();
            let user_id = h.get("user_id").and_then(|v| v.as_u64()).unwrap_or(0);

            items.push(PixabayWallpaper {
                id,
                title: first_tag(tags),
                tags: tags.to_string(),
                user: user.to_string(),
                user_url: user_profile_url(user, user_id),
                page_url: h
                    .get("pageURL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                url: url.to_string(),
                thumb: thumb_url(
                    h.get("webformatURL").and_then(|v| v.as_str()).unwrap_or(""),
                ),
                width: h.get("imageWidth").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                height: h.get("imageHeight").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            });
        }
    }

    let count = items.len() as u32;
    // 空页一定没有下一页 —— 否则会出现"翻到底还在转圈"的死循环
    let has_more = count > 0 && (page as u64) * (per_page as u64) < total_hits as u64;

    PixabayPage {
        items,
        count,
        total_hits,
        page,
        has_more,
        // 空页不能判成"降级"：整页没有条目时 has_full_access 恒为 false，
        // 会把"没搜到结果"误报成"你的 key 只能拿 1280px"。
        degraded: count > 0 && !has_full_access,
    }
}

/// 取 tags 的第一段当标题。
///
/// Pixabay 的 tags 形如 `"blossom, bloom, flower"`，整串当标题太长，
/// 卡片上放不下；完整 tags 仍保留在 `tags` 字段里供悬浮提示。
fn first_tag(tags: &str) -> String {
    tags.split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 作者主页 URL。`user` 或 `user_id` 缺任一个都返回空串（前端据此不渲染链接）。
///
/// Pixabay 的主页格式是 `https://pixabay.com/users/{USERNAME}-{ID}/`，
/// **两段都必需** —— 缺 `user_id` 时拼出 `/users/foo-0/` 是必然 404 的死链，
/// 而 `user_id` 并未出现在图片接口的文档字段表里（只在示例 JSON 中出现），
/// 属于"可能缺失"的字段，所以这里必须挡掉，不能想当然。
fn user_profile_url(user: &str, user_id: u64) -> String {
    if user.is_empty() || user_id == 0 {
        return String::new();
    }
    format!("https://pixabay.com/users/{user}-{user_id}/")
}

/// 把 `webformatURL` 的 640 变体改成 340 变体（列表缩略图，约 20KB/张）。
///
/// Pixabay 文档明确：`webformatURL` 里替换 `_640` 即可拿到其它尺寸
/// （`_180` / `_340` / `_960`）。替换不到（格式变了）就原样返回 ——
/// 宁可图大一点，也不能返回一个 404 的地址。
fn thumb_url(webformat: &str) -> String {
    if webformat.is_empty() {
        return String::new();
    }
    let replaced = webformat.replace("_640", "_340");
    if replaced == webformat {
        webformat.to_string()
    } else {
        replaced
    }
}

// ---------- 视频 ----------

/// 视频清晰度偏好（用户可在设置页切换）。
///
/// 只给两档：`Medium`（1080p，3~10MB/条，默认）与 `Large`（多为 4K，可能几十 MB）。
/// 不给 small/tiny —— 那是降级兜底，不是用户会主动选的目标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoQuality {
    Large,
    Medium,
}

impl VideoQuality {
    /// 解析前端传来的偏好值。**未知值一律回退 Medium** ——
    /// 宁可给个 1080p，也不要因为拼错一个字符串就整页空掉。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "large" | "4k" => Self::Large,
            _ => Self::Medium,
        }
    }

    /// 降级链。`Large` 也从 medium 起兜底：老素材的 `large.url` 是空串。
    fn chain(self) -> &'static [&'static str] {
        match self {
            Self::Large => &["large", "medium", "small", "tiny"],
            Self::Medium => &["medium", "small", "tiny"],
        }
    }
}

/// 视频列表条目 —— 同样按前端需要裁剪，不原样透传。
#[derive(Serialize, Clone, Debug)]
pub struct PixabayVideo {
    /// Pixabay 视频 id。**与图片 id 是两个命名空间**，可能撞号，
    /// 所以落盘文件名用 `PixabayVideo_` 前缀而不是 `Pixabay_`。
    pub id: u64,
    pub title: String,
    pub tags: String,
    pub user: String,
    pub user_url: String,
    pub page_url: String,
    /// 选中档位的视频 URL（设壁纸时下载用；预览直接播它）
    pub url: String,
    /// 海报图 URL（列表缩略图，取最小档的 thumbnail）
    pub thumb: String,
    /// **选中档位**的宽高（不是素材原始尺寸 —— 用户看到的画面就是这个档位）
    pub width: u32,
    pub height: u32,
    /// 时长（秒）
    pub duration: u32,
    /// 实际拿到的档位：large / medium / small / tiny。
    /// 与用户偏好不一致就说明"4K 不可用已降级"，前端如实展示。
    pub quality: String,
    /// 选中档位的文件大小（字节），0 = 接口没给。前端据此提示"约 xx MB"
    pub size: u64,
}

/// 视频分页响应
#[derive(Serialize, Clone, Debug)]
pub struct PixabayVideoPage {
    pub items: Vec<PixabayVideo>,
    pub count: u32,
    pub total_hits: u32,
    pub page: u32,
    pub has_more: bool,
}

/// 视频搜索参数。
///
/// 刻意与图片的 [`Query`] **分开**：两者参数集不同（视频没有 `orientation`，
/// 但多了 `video_type`），硬合并会造出一堆"对图片无意义"的字段，
/// 后续再加参数时不知道该往哪边放。
#[derive(Debug, Clone, Default)]
pub struct VideoQuery {
    pub key: String,
    pub query: String,
    pub category: Option<String>,
    /// film / animation / all，空 = 不筛
    pub video_type: Option<String>,
    pub order: Option<String>,
    pub page: u32,
    pub per_page: u32,
    pub lang: Option<String>,
    /// 最小宽度（按原视频宽度算），0 = 不筛。
    ///
    /// 视频接口**支持** `min_width` / `min_height`（与图片一致），
    /// 但**不支持** `colors` 与 `orientation` —— 后两个传过去会被静默忽略。
    pub min_width: Option<u32>,
    /// 最小高度，0 = 不筛
    pub min_height: Option<u32>,
    /// 方向：horizontal / vertical / all，空 = 横向（保持旧行为）。
    ///
    /// ⚠️ **接口没有 `orientation` 参数**（实测传 `vertical` 返回的仍是 96% 横屏，
    /// 与 `horizontal` 完全同分布）⇒ 只能按返回的宽高在本地过滤。
    /// 这也是为什么视频的"方向"和图片的"方向"实现方式完全不同：
    /// 图片是服务端筛，视频是解析时筛。
    pub orientation: Option<String>,
    /// 清晰度偏好（large / medium），见 [`VideoQuality::parse`]
    pub quality: String,
}

type VideoCacheMap = HashMap<String, (PixabayVideoPage, Instant)>;

fn video_cache() -> &'static Mutex<VideoCacheMap> {
    static CACHE: OnceLock<Mutex<VideoCacheMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 视频参数指纹 —— 同样**顺序固定**。
/// 末尾带上 `quality`：不同档位解析出的 `url` 不同，共用一个缓存会把 4K 请求喂成 1080p。
fn video_fingerprint(q: &VideoQuery) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        q.key,
        q.query.trim(),
        q.category.as_deref().unwrap_or(""),
        q.video_type.as_deref().unwrap_or(""),
        q.order.as_deref().unwrap_or(""),
        q.orientation.as_deref().unwrap_or(""),
        q.min_width.unwrap_or(0),
        q.min_height.unwrap_or(0),
        q.page,
        q.per_page,
        q.quality,
        q.lang.as_deref().unwrap_or("")
    )
}

/// 组装视频搜索的 query 参数（纯函数，不联网可测）。
///
/// ⚠️ 这里**永远不出现** `colors` / `orientation` / `image_type`：
/// 视频接口没有这三个参数，传过去不报错也不生效（实测确认）。
/// 单测 `video_params_never_include_image_only_filters` 锁死这一点。
fn build_video_params(q: &VideoQuery) -> Vec<(&'static str, String)> {
    let mut params: Vec<(&'static str, String)> = vec![
        ("key", q.key.trim().to_string()),
        ("safesearch", "true".into()),
        ("lang", q.lang.clone().unwrap_or_else(|| "zh".into())),
        ("page", q.page.max(1).to_string()),
        ("per_page", q.per_page.clamp(3, 200).to_string()),
    ];

    let query = q.query.trim();
    if !query.is_empty() {
        params.push(("q", query.to_string()));
    }
    if let Some(c) = q.category.as_deref().filter(|s| !s.is_empty()) {
        params.push(("category", c.to_string()));
    }
    if let Some(t) = q
        .video_type
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "all")
    {
        params.push(("video_type", t.to_string()));
    }
    if let Some(m) = q.min_width.filter(|m| *m > 0) {
        params.push(("min_width", m.to_string()));
    }
    if let Some(m) = q.min_height.filter(|m| *m > 0) {
        params.push(("min_height", m.to_string()));
    }
    if let Some(o) = q.order.as_deref().filter(|s| !s.is_empty()) {
        params.push(("order", o.to_string()));
    }
    params
}

/// 拉取一页视频搜索结果（24h 缓存，与图片同一套合规要求）。
pub fn fetch_videos(q: &VideoQuery) -> Result<PixabayVideoPage, String> {
    let key = q.key.trim();
    if key.is_empty() {
        return Err("未填写 Pixabay API key".into());
    }

    let fp = video_fingerprint(q);
    if let Ok(guard) = video_cache().lock() {
        if let Some((page, at)) = guard.get(&fp) {
            if at.elapsed() < CACHE_TTL {
                return Ok(page.clone());
            }
        }
    }

    let want = VideoQuality::parse(&q.quality);
    let orientation = VideoOrientation::parse(q.orientation.as_deref());
    let params = build_video_params(q);

    let resp = http::client()
        .get(VIDEO_API_BASE)
        .query(&params)
        .send()
        .map_err(|e| format!("请求 Pixabay 视频失败：{e}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        let body = body.trim();
        return Err(match status.as_u16() {
            400 => format!(
                "Pixabay 拒绝了视频请求：{}",
                if body.is_empty() {
                    "API key 无效或参数不合法"
                } else {
                    body
                }
            ),
            429 => "Pixabay 请求过于频繁（限 100 次/分钟），请稍后再试".to_string(),
            code => format!("Pixabay 返回 HTTP {code}"),
        });
    }

    let text = resp
        .text()
        .map_err(|e| format!("读取 Pixabay 视频响应失败：{e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 Pixabay 视频响应失败：{e}"))?;

    let page = parse_video_page(
        &json,
        q.page.max(1),
        q.per_page.clamp(3, 200),
        want,
        orientation,
    );

    if let Ok(mut guard) = video_cache().lock() {
        guard.retain(|_, (_, at)| at.elapsed() < CACHE_TTL);
        guard.insert(fp, (page.clone(), Instant::now()));
    }

    Ok(page)
}

/// 取某个档位节点；`url` 为空串一律当作**没有这个档位**。
///
/// 这是视频解析最容易踩的地方：`large` 节点**通常存在**，只是 `url` 为空、
/// `size` 为 0。只判 `get("large").is_some()` 会拿到一个空 URL，
/// 下载阶段才失败且报错完全误导。
fn rendition<'a>(videos: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let node = videos.get(key)?;
    let url = node.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return None;
    }
    Some(node)
}

/// 按偏好档位挑一个可用流，返回 `(url, 档位名, 宽, 高, 字节, 海报图)`。
fn pick_rendition(
    videos: &serde_json::Value,
    want: VideoQuality,
) -> Option<(String, String, u32, u32, u64, String)> {
    let (key, node) = want
        .chain()
        .iter()
        .find_map(|k| rendition(videos, k).map(|n| (*k, n)))?;

    let url = node.get("url")?.as_str()?.to_string();
    let width = node.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let height = node.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let size = node.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

    // 海报图取**最小档**：列表只需要一张 ~340px 的图，
    // 用 large 的 thumbnail 会白白多下几百 KB。
    let thumb = ["tiny", "small", "medium", "large"]
        .iter()
        .find_map(|k| {
            videos
                .get(*k)
                .and_then(|n| n.get("thumbnail"))
                .and_then(|t| t.as_str())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("")
        .to_string();

    Some((url, key.to_string(), width, height, size, thumb))
}

/// 视频最小可用**长边**（像素）。
///
/// 为什么是长边而不是宽度：竖屏视频的宽度只有 720，用"宽度 ≥1280"会把
/// 所有竖屏素材一起误杀（`720×1280` 是标准的竖屏 720p）。
///
/// 为什么是 1280 而不是 1920（实测数据，4 页 × 24 条样本）：
///
/// | 阈值 | 空搜索 | `夜空` | `ocean` |
/// |------|--------|--------|---------|
/// | 1920 | 31%（每页 7.5 条） | 53% | 49% |
/// | 1280 | 73%（每页 17.5 条） | 96% | 81% |
/// | 960  | 73%（每页 17.5 条） | 97% | 81% |
///
/// **1280 和 960 的结果完全一样** —— 说明 Pixabay 横屏视频基本只有两档：
/// 1280 以上，或者干脆是竖屏。取 1920 会让空搜索首屏只剩 7 条，翻页体验崩掉。
///
/// 720p 在 1080p 屏上确实偏糊，但前端每张卡片都标了实际档位（`quality`），
/// 用户能看见自己在看什么。**宁可让他看见 720p 并自己决定，也不要让他搜不到东西。**
const MIN_VIDEO_LONG_EDGE: u32 = 1280;

/// 视频方向筛选。
///
/// **接口不支持** `orientation`（实测 `vertical` 返回的仍 96% 横屏），
/// 所以这里只能在解析响应时按宽高筛。
///
/// 为什么值得给用户这个开关：竖屏视频当横屏壁纸会被裁成中间一条，
/// 但**竖屏显示器**（旋转的副屏）上正好相反。默认横向 = 旧行为，不受影响。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoOrientation {
    Horizontal,
    Vertical,
    All,
}

impl VideoOrientation {
    /// 空 / 未知值一律回退到**横向** —— 与改造前"竖屏一律丢弃"的行为一致，
    /// 升级后老用户看到的东西不会突然变样。
    fn parse(s: Option<&str>) -> Self {
        match s.map(str::trim) {
            Some("vertical") => Self::Vertical,
            Some("all") => Self::All,
            _ => Self::Horizontal,
        }
    }

    fn accepts(self, w: u32, h: u32) -> bool {
        match self {
            Self::Horizontal => w > h,
            Self::Vertical => h > w,
            // 方形视频在横竖两边都是"边缘情况"，只有明确要"全部"时才放行
            Self::All => w > 0 && h > 0,
        }
    }
}

/// 解析一页视频响应（纯函数，不联网可测 —— 档位降级全靠它）。
///
/// # 方向过滤（接口帮不了，只能自己来）
///
/// 视频接口**没有 `orientation` 参数**（那是图片接口才有的）。实测传
/// `orientation=vertical` 返回的 50 条里仍有 48 条是横屏，与 `horizontal`
/// 的分布完全一致 ⇒ 这个参数等于不存在，只能在解析时按宽高筛。
/// 竖屏素材本身是存在的（空搜索占 26%，如 `1440×2560`、`720×1280`）。
///
/// # `has_more` 为什么用**过滤前**的条数
///
/// 过滤会让一页从 24 条变成 12 条甚至 0 条。若用过滤后的 `count` 判断，
/// 整页被滤光时会误判成"没有下一页"，后面明明还有合格内容却再也翻不到。
fn parse_video_page(
    json: &serde_json::Value,
    page: u32,
    per_page: u32,
    want: VideoQuality,
    orientation: VideoOrientation,
) -> PixabayVideoPage {
    let total_hits = json
        .get("totalHits")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        .min(MAX_TOTAL_HITS as u64) as u32;

    let hits = json.get("hits").and_then(|v| v.as_array());
    // 过滤前的原始条数：判断"这一页到底有没有内容"只看它
    let raw_count = hits.map(|a| a.len()).unwrap_or(0);
    let mut items: Vec<PixabayVideo> = Vec::new();

    for h in hits.into_iter().flatten() {
        let Some(id) = h.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(videos) = h.get("videos") else {
            continue;
        };
        // 连 tiny 都拿不到：这条用不了，丢弃而不是留个点不动的空卡片
        let Some((url, quality, width, height, size, thumb)) = pick_rendition(videos, want) else {
            continue;
        };
        // 方向：接口筛不了，按选中档位自己判（默认横向 = 旧行为）
        if !orientation.accepts(width, height) {
            continue;
        }
        // 太小的宁可不要 —— 用户选"1080p"却看到 720p 的糊图，比没有更糟。
        // 用**长边**判：竖屏视频的宽度只有 720，按宽度判会把它们全误杀。
        if width.max(height) < MIN_VIDEO_LONG_EDGE {
            continue;
        }

        let tags = h.get("tags").and_then(|v| v.as_str()).unwrap_or("").trim();
        let user = h.get("user").and_then(|v| v.as_str()).unwrap_or("").trim();
        let user_id = h.get("user_id").and_then(|v| v.as_u64()).unwrap_or(0);

        items.push(PixabayVideo {
            id,
            title: first_tag(tags),
            tags: tags.to_string(),
            user: user.to_string(),
            user_url: user_profile_url(user, user_id),
            page_url: h
                .get("pageURL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            url,
            thumb,
            width,
            height,
            duration: h.get("duration").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            quality,
            size,
        });
    }

    let count = items.len() as u32;
    let has_more = raw_count > 0 && (page as u64) * (per_page as u64) < total_hits as u64;

    PixabayVideoPage {
        items,
        count,
        total_hits,
        page,
        has_more,
    }
}

// ---------- 下载与本地扫描 ----------

/// Pixabay 下载目录：`<壁纸目录>\Pixabay`。
///
/// 与必应同样做成**派生**而非独立配置 —— 独立目录会在用户改壁纸目录之后与
/// 「本地」列表脱钩（文件落在壁纸目录之外，本地选项卡看不到，等于下载完就找不到）。
pub fn download_dir(local_dir: Option<String>) -> PathBuf {
    crate::resolve_save_dir(local_dir).join("Pixabay")
}

/// 下载原图到 `dir` 并返回本地绝对路径。
///
/// 同名文件已存在时直接复用，不重复走网络（同一张图反复点"设为壁纸"很常见）。
/// 先写 `.part` 再改名，避免半截文件被后续流程当成完整图片使用。
///
/// 扩展名按 URL 判断而不是写死 `.jpg`：Pixabay 的 CDN 图基本是 jpg，
/// 但 `imageURL` 偶尔给 png，写死扩展名会让系统壁纸接口按错误格式解码。
pub fn download_wallpaper(url: &str, id: u64, dir: &Path) -> Result<String, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("壁纸地址无效".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("创建下载目录失败 {}: {e}", dir.display()))?;

    let ext = if url.to_ascii_lowercase().contains(".png") {
        "png"
    } else {
        "jpg"
    };
    let dest: PathBuf = dir.join(format!("Pixabay_{id}.{ext}"));

    if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(normalize(&dest));
    }

    let resp = http::get(url, "Pixabay 图片")?;
    if !resp.status().is_success() {
        return Err(format!("下载 Pixabay 图片失败：HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("读取 Pixabay 图片数据失败：{e}"))?;
    // 正常原图数百 KB，小于 4KB 视为异常响应（错误页/占位图）
    if bytes.len() < 4096 {
        return Err("下载的图片数据异常（文件过小）".into());
    }

    let tmp = dir.join(format!("Pixabay_{id}.{ext}.part"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入壁纸文件失败：{e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("保存壁纸文件失败：{e}")
    })?;

    Ok(normalize(&dest))
}

/// 从 `Pixabay_{id}.{jpg|png}` 解析图片 id；不匹配返回 None。
///
/// 只认自家命名 —— 用户自己的 `IMG_20260917.jpg` 之类绝不能误判成 Pixabay 图。
fn parse_cached_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_prefix("pixabay_")?;
    let stem = stem
        .strip_suffix(".jpg")
        .or_else(|| stem.strip_suffix(".png"))?;
    if !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()) {
        Some(stem.to_string())
    } else {
        None
    }
}

/// 在**本地壁纸目录**里递归找已下载的 Pixabay 图，返回 `(id, 本地绝对路径)`，按 id 倒序。
///
/// 与必应同理：扫的是本地壁纸目录而非下载目录，这样「已下载」徽标与
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

/// 视频落盘文件名前缀。
///
/// **必须与图片的 `Pixabay_` 区分开**：Pixabay 的图片 id 与视频 id 是两个独立
/// 命名空间，可能出现同一个数字。共用前缀会让 `Pixabay_123.jpg` 与
/// `Pixabay_123.mp4` 被当成同一个资源，徽标互相点亮、打开目录跳错文件。
const VIDEO_PREFIX: &str = "PixabayVideo_";

/// 视频下载超时：单独放宽到 5 分钟。
///
/// 全局客户端是 20 秒（拉 JSON / 小图够用），而 4K 视频动辄几十 MB，
/// 慢速网络上 20 秒必然断，报错却只是"请求失败"，看不出是超时。
const VIDEO_TIMEOUT_SECS: u64 = 300;

/// 下载视频到 `dir`，返回本地绝对路径。
///
/// 与图片的差异：
/// - **流式写入**而不是先 `bytes()` 到内存 —— 一条 4K 视频几十 MB，
///   全部读进内存再落盘在小内存机器上会明显卡顿；
/// - 最小体积阈值放到 64KB（图片是 4KB）：视频不可能这么小，
///   低于它基本是错误页或被截断的响应。
pub fn download_video(url: &str, id: u64, dir: &Path) -> Result<String, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("视频地址无效".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("创建下载目录失败 {}: {e}", dir.display()))?;

    // 扩展名按 URL 判断：Pixabay 视频有 mp4，极少数老素材是 webm
    let lower = url.to_ascii_lowercase();
    let ext = if lower.contains(".webm") { "webm" } else { "mp4" };
    let dest: PathBuf = dir.join(format!("{VIDEO_PREFIX}{id}.{ext}"));

    // 同名文件已存在直接复用 —— 同一段视频反复"设为动态壁纸"很常见，
    // 每次都重下几十 MB 既慢又浪费配额。
    if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(normalize(&dest));
    }

    let mut resp = http::get_long(url, "Pixabay 视频", VIDEO_TIMEOUT_SECS)?;
    if !resp.status().is_success() {
        return Err(format!("下载 Pixabay 视频失败：HTTP {}", resp.status()));
    }

    let tmp = dir.join(format!("{VIDEO_PREFIX}{id}.{ext}.part"));
    {
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("创建视频文件失败：{e}"))?;
        std::io::copy(&mut resp, &mut file).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("写入视频文件失败：{e}")
        })?;
    }

    let written = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if written < 64 * 1024 {
        let _ = std::fs::remove_file(&tmp);
        return Err("下载的视频数据异常（文件过小）".into());
    }

    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("保存视频文件失败：{e}")
    })?;

    Ok(normalize(&dest))
}

/// 从 `PixabayVideo_{id}.{mp4|webm}` 解析视频 id；不匹配返回 None。
fn parse_video_cached_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_prefix("pixabayvideo_")?;
    let stem = stem
        .strip_suffix(".mp4")
        .or_else(|| stem.strip_suffix(".webm"))?;
    if !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit()) {
        Some(stem.to_string())
    } else {
        None
    }
}

/// 扫本地壁纸目录里已下载的 Pixabay **视频**，返回 `(id, 路径)`，按 id 倒序。
///
/// 与图片分开一个函数而不是给 `scan_downloaded` 加参数：
/// 前缀、扩展名、返回语义都不一样，合并只会让两边都变难读。
pub fn scan_downloaded_videos(dir: &Path) -> Vec<(String, String)> {
    let Ok(files) = crate::wallpaper::walk_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for path in files {
        let Some(name) = Path::new(&path).file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(id) = parse_video_cached_name(name) else {
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

    /// 造一条 hit。三个 URL 字段按需开关，用来模拟免费 / 付费两种 key。
    fn hit(id: u64, with_original: bool, with_hd: bool, with_large: bool) -> serde_json::Value {
        let mut v = json!({
            "id": id,
            "tags": "blossom, bloom, flower",
            "user": "Josch13",
            "user_id": 48777,
            "pageURL": format!("https://pixabay.com/photos/blossom-{id}/"),
            "webformatURL": format!("https://pixabay.com/get/abc_{id}_640.jpg"),
            "imageWidth": 4000,
            "imageHeight": 2250,
        });
        if with_original {
            v["imageURL"] = json!(format!("https://cdn.pixabay.com/{id}_orig.jpg"));
        }
        if with_hd {
            v["fullHDURL"] = json!(format!("https://cdn.pixabay.com/{id}_1920.jpg"));
        }
        if with_large {
            v["largeImageURL"] = json!(format!("https://cdn.pixabay.com/{id}_1280.jpg"));
        }
        v
    }

    /// 有 full access：优先用原图，且不标记降级
    #[test]
    fn prefers_original_when_available() {
        let json = json!({ "totalHits": 1, "hits": [hit(1, true, true, true)] });
        let page = parse_page(&json, 1, 24);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].url, "https://cdn.pixabay.com/1_orig.jpg");
        assert!(!page.degraded, "有 imageURL 时不该标记降级");
    }

    /// 只有 fullHD：退到 1920，仍算有 full access
    #[test]
    fn falls_back_to_fullhd() {
        let json = json!({ "totalHits": 1, "hits": [hit(2, false, true, true)] });
        let page = parse_page(&json, 1, 24);
        assert_eq!(page.items[0].url, "https://cdn.pixabay.com/2_1920.jpg");
        assert!(!page.degraded);
    }

    /// 免费 key：只有 largeImageURL，必须降级并如实标记
    #[test]
    fn degrades_to_large_on_free_key() {
        let json = json!({ "totalHits": 1, "hits": [hit(3, false, false, true)] });
        let page = parse_page(&json, 1, 24);
        assert_eq!(page.items[0].url, "https://cdn.pixabay.com/3_1280.jpg");
        assert!(page.degraded, "只有 largeImageURL 时必须标记降级");
    }

    /// 三档全空：丢弃该条目，不能留一张点不动的空卡片
    #[test]
    fn drops_items_without_any_url() {
        let json = json!({ "totalHits": 2, "hits": [hit(4, false, false, false), hit(5, false, false, true)] });
        let page = parse_page(&json, 1, 24);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, 5);
        assert_eq!(page.count, 1);
    }

    /// 混在一页里：只要有一条带原图，整页就不算降级
    #[test]
    fn mixed_page_is_not_degraded() {
        let json = json!({ "totalHits": 2, "hits": [hit(6, false, false, true), hit(7, true, true, true)] });
        let page = parse_page(&json, 1, 24);
        assert!(!page.degraded);
    }

    /// 缩略图：_640 → _340；格式不符时原样返回而不是拼坏
    #[test]
    fn thumb_rewrites_640_to_340() {
        assert_eq!(
            thumb_url("https://pixabay.com/get/abc_123_640.jpg"),
            "https://pixabay.com/get/abc_123_340.jpg"
        );
        assert_eq!(
            thumb_url("https://pixabay.com/get/abc_123.jpg"),
            "https://pixabay.com/get/abc_123.jpg"
        );
        assert_eq!(thumb_url(""), "");
    }

    /// 标题取 tags 第一段；作者主页按 `{user}-{id}` 拼
    #[test]
    fn title_and_profile_url() {
        let json = json!({ "totalHits": 1, "hits": [hit(8, false, false, true)] });
        let item = &parse_page(&json, 1, 24).items[0];
        assert_eq!(item.title, "blossom");
        assert_eq!(item.tags, "blossom, bloom, flower");
        assert_eq!(item.user_url, "https://pixabay.com/users/Josch13-48777/");
    }

    /// 没有 user 字段时不能拼出 `https://pixabay.com/users/-0/` 这种死链
    #[test]
    fn empty_user_yields_empty_profile_url() {
        assert_eq!(user_profile_url("", 0), "");
    }

    /// 有 user 但缺 `user_id`：同样不能拼 —— `/users/Josch13-0/` 是必然 404 的死链。
    /// `user_id` 不在图片接口的文档字段表里（只在示例 JSON 出现），确实可能缺。
    #[test]
    fn missing_user_id_yields_empty_profile_url() {
        assert_eq!(user_profile_url("Josch13", 0), "");
    }

    /// 分页：page*per_page < totalHits 才有下一页
    #[test]
    fn has_more_follows_total_hits() {
        let json = json!({ "totalHits": 100, "hits": [hit(9, false, false, true)] });
        assert!(parse_page(&json, 1, 24).has_more); // 24 < 100
        assert!(parse_page(&json, 4, 24).has_more); // 96 < 100
        assert!(!parse_page(&json, 5, 24).has_more); // 120 >= 100
    }

    /// 空页不能报"还有更多"，否则前端会一直转圈
    #[test]
    fn empty_page_has_no_more() {
        let json = json!({ "totalHits": 500, "hits": [] });
        let page = parse_page(&json, 3, 24);
        assert_eq!(page.count, 0);
        assert!(!page.has_more);
        // 也不能判成降级 —— 否则"没搜到结果"会被显示成"你的 key 只能拿 1280px"
        assert!(!page.degraded, "空页不该标记降级");
    }

    /// totalHits 超过 500 时按 500 计（Pixabay 硬上限）
    #[test]
    fn total_hits_capped_at_500() {
        let json = json!({ "totalHits": 99999, "hits": [] });
        assert_eq!(parse_page(&json, 1, 24).total_hits, MAX_TOTAL_HITS);
    }

    /// 缺 totalHits 也不能 panic
    #[test]
    fn missing_total_hits_is_zero() {
        let json = json!({ "hits": [hit(10, false, false, true)] });
        let page = parse_page(&json, 1, 24);
        assert_eq!(page.total_hits, 0);
        assert!(!page.has_more);
    }

    /// 文件名解析：只认自家命名，且**绝不能**误伤用户自己的文件
    #[test]
    fn parse_cached_name_only_accepts_own_output() {
        let hits = [
            ("Pixabay_123456.jpg", "123456"),
            ("pixabay_123456.PNG", "123456"),
            ("PIXABAY_42.png", "42"),
        ];
        for (name, want) in hits {
            assert_eq!(
                parse_cached_name(name).as_deref(),
                Some(want),
                "应命中：{name}"
            );
        }

        let misses = [
            // 下载中途的半截文件
            "Pixabay_123456.jpg.part",
            "Pixabay_.jpg",
            "Pixabay_abc.jpg",
            "Pixabay_123456.webp",
            // 必应 / 用户自己的文件
            "BingWallpaper_20260917.jpg",
            "IMG_20260917.jpg",
            "日落_123456.jpg",
        ];
        for name in misses {
            assert_eq!(parse_cached_name(name), None, "应落选：{name}");
        }
    }

    // ---------- 视频 ----------

    /// 造一条视频 hit。四个档位按需开关 —— **关掉时节点仍然存在但 url 为空**，
    /// 这正是 Pixabay 的真实行为（`large` 节点常驻，只是老素材的 url 是空串）。
    fn vhit(id: u64, large: bool, medium: bool, small: bool, tiny: bool) -> serde_json::Value {
        fn node(id: u64, key: &str, on: bool, w: u32, h: u32) -> serde_json::Value {
            json!({
                "url": if on { format!("https://cdn.pixabay.com/video/{id}_{key}.mp4") } else { String::new() },
                "width": if on { w } else { 0 },
                "height": if on { h } else { 0 },
                "size": if on { 1_000_000u64 } else { 0 },
                "thumbnail": if on { format!("https://cdn.pixabay.com/video/{id}_{key}.jpg") } else { String::new() },
            })
        }
        json!({
            "id": id,
            "pageURL": format!("https://pixabay.com/videos/id-{id}/"),
            "type": "film",
            "tags": "flowers, yellow, blossom",
            "duration": 12,
            "user": "Coverr-Free-Footage",
            "user_id": 1281706,
            "videos": {
                "large": node(id, "large", large, 3840, 2160),
                "medium": node(id, "medium", medium, 1920, 1080),
                "small": node(id, "small", small, 1280, 720),
                "tiny": node(id, "tiny", tiny, 960, 540),
            }
        })
    }

    /// 偏好 4K 且确实有 4K：用 large，档位如实回报
    #[test]
    fn video_prefers_large_when_available() {
        let json = json!({ "totalHits": 1, "hits": [vhit(1, true, true, true, true)] });
        let page = parse_video_page(&json, 1, 24, VideoQuality::Large, VideoOrientation::Horizontal);
        assert_eq!(page.items.len(), 1);
        let v = &page.items[0];
        assert_eq!(v.quality, "large");
        assert_eq!(v.url, "https://cdn.pixabay.com/video/1_large.mp4");
        assert_eq!((v.width, v.height), (3840, 2160));
    }

    /// **最关键的一条**：`large` 节点存在但 url 是空串 ⇒ 必须降到 medium。
    /// 只判节点是否存在会拿到空 URL，下载阶段才失败且报错完全误导。
    #[test]
    fn video_falls_back_when_large_url_empty() {
        let json = json!({ "totalHits": 1, "hits": [vhit(2, false, true, true, true)] });
        let page = parse_video_page(&json, 1, 24, VideoQuality::Large, VideoOrientation::Horizontal);
        let v = &page.items[0];
        assert_eq!(v.quality, "medium", "4K 不可用时必须降到 1080p");
        assert_eq!(v.url, "https://cdn.pixabay.com/video/2_medium.mp4");
        assert_eq!((v.width, v.height), (1920, 1080));
    }

    /// 偏好 1080p 时**不该**去拿 4K（省流量，也是用户明确选的）
    #[test]
    fn video_medium_pref_ignores_large() {
        let json = json!({ "totalHits": 1, "hits": [vhit(3, true, true, true, true)] });
        let page = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal);
        assert_eq!(page.items[0].quality, "medium");
        assert_eq!(page.items[0].url, "https://cdn.pixabay.com/video/3_medium.mp4");
    }

    /// 造一条**指定尺寸**的视频 hit（只挂一个 medium 档）。
    ///
    /// 专供方向 / 分辨率过滤的测试用 —— 那两条只看"选中档位"的宽高，
    /// 用完整四档的 `vhit` 反而要多写一堆无关字段。
    fn vhit_wh(id: u64, w: u32, h: u32) -> serde_json::Value {
        json!({
            "id": id,
            "pageURL": format!("https://pixabay.com/videos/id-{id}/"),
            "type": "film",
            "tags": "test",
            "duration": 10,
            "user": "tester",
            "user_id": 1,
            "videos": {
                "medium": {
                    "url": format!("https://cdn.pixabay.com/video/{id}_medium.mp4"),
                    "width": w,
                    "height": h,
                    "size": 1_000_000u64,
                    "thumbnail": format!("https://cdn.pixabay.com/video/{id}_medium.jpg"),
                }
            }
        })
    }

    /// 连 medium 都没有时继续往下退，不能直接丢掉。
    ///
    /// **直接测 `pick_rendition`，不经过 `parse_video_page`**：后者会按
    /// `MIN_VIDEO_WIDTH` 过滤，small(1280)/tiny(960) 必然被丢掉，那样就测不到
    /// 降级链本身了。"能不能拿到 URL" 与 "拿到的能不能用" 是两层，要分开测。
    #[test]
    fn video_degrades_through_small_and_tiny() {
        let hit = vhit(4, false, false, true, true);
        let (_, q, w, h, _, _) =
            pick_rendition(&hit["videos"], VideoQuality::Large).expect("应退到 small");
        assert_eq!(q, "small");
        assert_eq!((w, h), (1280, 720));

        let hit = vhit(5, false, false, false, true);
        let (_, q, w, h, _, _) =
            pick_rendition(&hit["videos"], VideoQuality::Large).expect("应退到 tiny");
        assert_eq!(q, "tiny");
        assert_eq!((w, h), (960, 540));
    }

    /// 竖屏 / 方形一律丢弃。
    ///
    /// 视频接口**没有 `orientation` 参数**（那是图片接口才有的），所以竖屏素材
    /// 会直接混进来（实测空搜索 26%）。当壁纸用会被裁成中间一条，比糊更难看。
    #[test]
    fn video_drops_portrait_and_square() {
        let json = json!({ "totalHits": 3, "hits": [
            vhit_wh(21, 1080, 1920), // 竖屏
            vhit_wh(22, 1080, 1080), // 方形
            vhit_wh(23, 1920, 1080), // 正常横屏
        ]});
        // 默认（横向）= 旧行为：竖屏与方形都丢掉
        let page = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal);
        assert_eq!(page.count, 1);
        assert_eq!(page.items[0].id, 23);
    }

    /// 方向三档：横向只留横屏、竖向只留竖屏、全部两者都留（方形只在"全部"里出现）。
    ///
    /// 视频接口没有 `orientation` 参数（实测无效），这一层是**唯一的**方向过滤，
    /// 所以三档必须逐一锁死 —— 写反了界面上看不出来，只是"选竖向没反应"。
    #[test]
    fn video_orientation_three_modes() {
        let json = json!({ "totalHits": 3, "hits": [
            vhit_wh(41, 1920, 1080), // 横
            vhit_wh(42, 1080, 1920), // 竖
            vhit_wh(43, 1440, 1440), // 方（长边 1440 ≥ 1280，不会被分辨率门槛拦掉）
        ]});

        let h = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal);
        assert_eq!(h.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![41]);

        let v = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Vertical);
        assert_eq!(v.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![42]);

        let a = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::All);
        assert_eq!(
            a.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![41, 42, 43]
        );
    }

    /// 竖屏视频的宽度只有 720，**不能**按宽度判"太小" —— 那会把整档竖屏全误杀。
    /// 判据必须是长边。
    #[test]
    fn video_vertical_kept_despite_narrow_width() {
        let json = json!({ "totalHits": 2, "hits": [
            vhit_wh(51, 720, 1280),  // 竖屏 720p：宽 720 但长边 1280，应当保留
            vhit_wh(52, 480, 640),   // 竖屏低清：长边 640，应当丢弃
        ]});
        let page = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Vertical);
        assert_eq!(page.count, 1);
        assert_eq!(page.items[0].id, 51);
    }

    /// 空 / 未知的 orientation 必须回退成横向（= 改造前的行为）
    #[test]
    fn video_orientation_defaults_to_horizontal() {
        assert_eq!(VideoOrientation::parse(None), VideoOrientation::Horizontal);
        assert_eq!(
            VideoOrientation::parse(Some("")),
            VideoOrientation::Horizontal
        );
        assert_eq!(
            VideoOrientation::parse(Some("sideways")),
            VideoOrientation::Horizontal
        );
        assert_eq!(
            VideoOrientation::parse(Some("vertical")),
            VideoOrientation::Vertical
        );
        assert_eq!(VideoOrientation::parse(Some("all")), VideoOrientation::All);
    }

    /// 低于 1280 宽的一律丢弃。
    ///
    /// 阈值为什么不是 1920 见 `MIN_VIDEO_WIDTH` 的表 —— 简言之 1920 会把空搜索
    /// 首屏砍到只剩 7 条，而 1280 和 960 的保留率完全一样（说明横屏素材
    /// 天然就分在 1280 以上）。
    #[test]
    fn video_drops_low_resolution() {
        let json = json!({ "totalHits": 2, "hits": [
            vhit_wh(31, 960, 540),
            vhit_wh(32, 1280, 720),
        ]});
        let page = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal);
        assert_eq!(page.count, 1);
        assert_eq!(page.items[0].id, 32);
    }

    /// 整页被滤光时**绝不能**把 `has_more` 判成 false —— 后面还有合格内容。
    /// 这就是为什么判据用**过滤前**的原始条数。
    #[test]
    fn video_has_more_uses_raw_count() {
        let json = json!({ "totalHits": 500, "hits": [
            vhit_wh(41, 1080, 1920),
            vhit_wh(42, 1080, 1920),
        ]});
        let page = parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal);
        assert_eq!(page.count, 0, "两条都该被滤掉");
        assert!(page.has_more, "整页滤光也必须继续翻页");
    }

    /// 四档全空：丢弃，不留一张点不动的空卡片
    #[test]
    fn video_drops_items_without_any_rendition() {
        let json = json!({ "totalHits": 2, "hits": [vhit(6, false, false, false, false), vhit(7, true, true, true, true)] });
        let page = parse_video_page(&json, 1, 24, VideoQuality::Large, VideoOrientation::Horizontal);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, 7);
        assert_eq!(page.count, 1);
    }

    /// 海报图取**最小档**的 thumbnail —— 列表只要 ~340px，用 4K 那张会白下几百 KB
    #[test]
    fn video_thumb_uses_smallest_rendition() {
        let json = json!({ "totalHits": 1, "hits": [vhit(8, true, true, true, true)] });
        let page = parse_video_page(&json, 1, 24, VideoQuality::Large, VideoOrientation::Horizontal);
        assert_eq!(
            page.items[0].thumb,
            "https://cdn.pixabay.com/video/8_tiny.jpg"
        );
    }

    /// 时长 / 大小 / 标题 / 作者主页都要搬对
    #[test]
    fn video_maps_meta_fields() {
        let json = json!({ "totalHits": 1, "hits": [vhit(9, false, true, true, true)] });
        let v = &parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal).items[0];
        assert_eq!(v.duration, 12);
        assert_eq!(v.size, 1_000_000);
        assert_eq!(v.title, "flowers");
        assert_eq!(v.tags, "flowers, yellow, blossom");
        assert_eq!(v.user_url, "https://pixabay.com/users/Coverr-Free-Footage-1281706/");
        assert_eq!(v.page_url, "https://pixabay.com/videos/id-9/");
    }

    /// 未知清晰度值一律回退 1080p —— 拼错一个字符串不该让整页空掉
    #[test]
    fn video_quality_parse_is_lenient() {
        assert_eq!(VideoQuality::parse("large"), VideoQuality::Large);
        assert_eq!(VideoQuality::parse("4K"), VideoQuality::Large);
        assert_eq!(VideoQuality::parse("medium"), VideoQuality::Medium);
        assert_eq!(VideoQuality::parse(""), VideoQuality::Medium);
        assert_eq!(VideoQuality::parse("ultra"), VideoQuality::Medium);
    }

    /// 分页与 totalHits 上限（与图片同一套规则）
    #[test]
    fn video_paging_rules() {
        let json = json!({ "totalHits": 100, "hits": [vhit(10, false, true, true, true)] });
        assert!(parse_video_page(&json, 1, 24, VideoQuality::Medium, VideoOrientation::Horizontal).has_more);
        assert!(!parse_video_page(&json, 5, 24, VideoQuality::Medium, VideoOrientation::Horizontal).has_more);
        // 空页不能报"还有更多"，否则前端一直转圈
        let empty = json!({ "totalHits": 500, "hits": [] });
        assert!(
            !parse_video_page(&empty, 3, 24, VideoQuality::Medium, VideoOrientation::All).has_more
        );
        // 超过 500 按 500 计
        let big = json!({ "totalHits": 99999, "hits": [] });
        assert_eq!(
            parse_video_page(&big, 1, 24, VideoQuality::Medium, VideoOrientation::All).total_hits,
            MAX_TOTAL_HITS
        );
    }

    /// 视频文件名解析：只认自家命名。
    /// **必须与图片互相不认** —— 图片 id 与视频 id 是独立命名空间，可能撞号。
    #[test]
    fn parse_video_cached_name_only_accepts_own_output() {
        for (name, want) in [
            ("PixabayVideo_125.mp4", "125"),
            ("pixabayvideo_42.MP4", "42"),
            ("PIXABAYVIDEO_7.webm", "7"),
        ] {
            assert_eq!(
                parse_video_cached_name(name).as_deref(),
                Some(want),
                "应命中：{name}"
            );
        }

        for name in [
            "PixabayVideo_125.mp4.part",
            "PixabayVideo_.mp4",
            "PixabayVideo_abc.mp4",
            "PixabayVideo_125.mkv",
            // 图片的命名：绝不能当成视频
            "Pixabay_125.jpg",
            "Pixabay_125.png",
            "BingWallpaper_20260917.jpg",
            "IMG_20260917.mp4",
        ] {
            assert_eq!(parse_video_cached_name(name), None, "应落选：{name}");
        }

        // 反向：视频文件也不能被图片那条解析认成图片
        assert_eq!(parse_cached_name("PixabayVideo_125.mp4"), None);
    }

    // ---- 筛选参数下发（对应网页筛选栏）----
    //
    // 这组测试存在的理由：筛选参数漏下发时**没有任何报错**，
    // 表现只是"用户改了筛选、结果一点没变"。前端桩测试也发现不了
    // （它拦的是 invoke 参数，不是真正发给 Pixabay 的 query）。

    fn param<'a>(params: &'a [(&'static str, String)], key: &str) -> Option<&'a str> {
        params
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    fn base_image_query() -> Query {
        Query {
            key: "K".into(),
            query: "city night".into(),
            page: 1,
            per_page: 24,
            ..Default::default()
        }
    }

    #[test]
    fn image_params_carry_every_filter() {
        let q = Query {
            category: Some("nature".into()),
            orientation: Some("vertical".into()),
            min_width: Some(3840),
            min_height: Some(2160),
            colors: Some("red,blue".into()),
            order: Some("latest".into()),
            image_type: Some("illustration".into()),
            editors_choice: true,
            ..base_image_query()
        };
        let p = build_image_params(&q);
        assert_eq!(param(&p, "category"), Some("nature"));
        assert_eq!(param(&p, "orientation"), Some("vertical"));
        assert_eq!(param(&p, "min_width"), Some("3840"));
        assert_eq!(param(&p, "min_height"), Some("2160"));
        assert_eq!(param(&p, "colors"), Some("red,blue"));
        assert_eq!(param(&p, "order"), Some("latest"));
        assert_eq!(param(&p, "image_type"), Some("illustration"));
        assert_eq!(param(&p, "editors_choice"), Some("true"));
    }

    /// `orientation=all` / 0 尺寸 / 空颜色 都**不能**下发：
    /// 传 `all` 与省略等价，但会白白让缓存指纹分叉（同一结果两份缓存）。
    #[test]
    fn image_params_skip_neutral_filters() {
        let q = Query {
            orientation: Some("all".into()),
            min_width: Some(0),
            min_height: Some(0),
            colors: Some(String::new()),
            ..base_image_query()
        };
        let p = build_image_params(&q);
        assert_eq!(param(&p, "orientation"), None);
        assert_eq!(param(&p, "min_width"), None);
        assert_eq!(param(&p, "min_height"), None);
        assert_eq!(param(&p, "colors"), None);
    }

    /// 视频接口没有 colors / orientation / image_type，
    /// 传过去不报错也不生效 —— 所以这里必须一个都不出现。
    #[test]
    fn video_params_never_include_image_only_filters() {
        let q = VideoQuery {
            key: "K".into(),
            query: "nature".into(),
            video_type: Some("animation".into()),
            min_width: Some(1920),
            min_height: Some(1080),
            order: Some("latest".into()),
            page: 1,
            per_page: 24,
            quality: "medium".into(),
            ..Default::default()
        };
        let p = build_video_params(&q);
        assert_eq!(param(&p, "video_type"), Some("animation"));
        assert_eq!(param(&p, "min_width"), Some("1920"));
        assert_eq!(param(&p, "min_height"), Some("1080"));
        for forbidden in ["colors", "orientation", "image_type"] {
            assert_eq!(
                param(&p, forbidden),
                None,
                "视频接口不支持 {forbidden}，不能下发"
            );
        }
    }

    /// `video_type=all` 也不下发（同 `orientation=all`）
    #[test]
    fn video_params_skip_video_type_all() {
        let q = VideoQuery {
            key: "K".into(),
            video_type: Some("all".into()),
            page: 1,
            per_page: 24,
            quality: "medium".into(),
            ..Default::default()
        };
        assert_eq!(param(&build_video_params(&q), "video_type"), None);
    }

    /// 新增的筛选字段必须进指纹，否则改了筛选会命中旧缓存
    /// —— 表现同样是"改了没反应"，且完全看不出是缓存的问题。
    #[test]
    fn fingerprint_covers_new_filters() {
        let base = base_image_query();
        let fp0 = fingerprint(&base);

        for (name, q) in [
            (
                "colors",
                Query {
                    colors: Some("blue".into()),
                    ..base_image_query()
                },
            ),
            (
                "min_height",
                Query {
                    min_height: Some(2160),
                    ..base_image_query()
                },
            ),
        ] {
            assert_ne!(fingerprint(&q), fp0, "{name} 没进指纹，会命中旧缓存");
        }

        let vbase = VideoQuery {
            key: "K".into(),
            page: 1,
            per_page: 24,
            quality: "medium".into(),
            ..Default::default()
        };
        let vfp0 = video_fingerprint(&vbase);
        let vfp1 = video_fingerprint(&VideoQuery {
            min_height: Some(2160),
            ..vbase.clone()
        });
        assert_ne!(vfp1, vfp0, "视频的 min_height 没进指纹");
    }
}
