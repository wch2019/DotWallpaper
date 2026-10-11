// 必应每日壁纸后端实现：
// - fetch_wallpapers：调用 Bing HPImageArchive 接口拉取每日壁纸列表（idx 0 / 8 两批，共 16 天）
// - fetch_archive_month：从社区归档源拉取某个**年月**的历史壁纸（官方接口只能回溯 16 天，
//   更早的靠 `Zhu-junwei/bing-wallpaper-archive`，2016-03 至今；按月分片，前端滚动触发）
// - download_wallpaper：设为桌面壁纸时把原图下载到本地，返回本地路径
// - download_dir：下载目录 = **本地壁纸目录下的 `BingWallpaper` 子目录**（派生，不可独立配置）
// - scan_downloaded：在本地壁纸目录里递归找已下载的壁纸（供"已下载"徽标与跳转定位）
//
// 列表阶段不下载原图：前端直接用远程 URL 展示（列表用 400x240 小图变体，
// 仅约 30KB/张），只有点击"设为壁纸"才拉取 1920x1080 原图 —— 既省流量也省磁盘。
//
// **职责划分（2026-10-10 拍板）**：
//   「必应」选项卡 = 纯在线图库（官方 16 天 + 归档历史，都可滚动加载）
//   下载下来的文件归「本地」选项卡管（下载目录就在本地壁纸目录里面，天然可见）
//   在线卡片上用"已下载"徽标回指本地已存在的那张
// 所以**不要把本地缓存并进必应列表** —— 那会让同一个文件在两个选项卡里各出现一次。

use serde::Serialize;
use std::path::{Path, PathBuf};
use crate::resolve_save_dir;

/// 必应壁纸归档接口（n 上限 8，通过 idx 分批取更多历史）
const API_TMPL: &str = "https://www.bing.com/HPImageArchive.aspx?format=js&idx={idx}&n=8&mkt=zh-CN";
/// 图片资源站前缀（接口返回的 url 为 /th?id=... 相对路径）
const ORIGIN: &str = "https://www.bing.com";

/// 必应壁纸**归档源**基址（社区项目 `Zhu-junwei/bing-wallpaper-archive`）。
///
/// 官方 `HPImageArchive` 有硬上限：`n` 被服务端钳制在 8，`idx` 超过 8 之后返回的
/// **完全是同一批数据**（实测试到 idx=80 都如此），换 mkt、传负 idx 也无效 ⇒
/// 16 天（去重 15 天）就是天花板。更早的历史只能靠社区归档：
/// 2016-03-05 至今 3800+ 张，GitHub Actions 每日 00:00 更新，免费无 key。
/// 数据形状与官方接口**同构**，故解析逻辑复用 [`parse_images`]。
///
/// 同一仓库挂了多个 jsdelivr 镜像 —— 国内网络对单一 CDN 节点的可达性不稳定，
/// 依次降级比赌一个域名靠谱。`cdn` 是主节点，另两个是备胎。
const ARCHIVE_BASES: &[&str] = &[
    "https://cdn.jsdelivr.net/gh/Zhu-junwei/bing-wallpaper-archive",
    "https://fastly.jsdelivr.net/gh/Zhu-junwei/bing-wallpaper-archive",
    "https://gcore.jsdelivr.net/gh/Zhu-junwei/bing-wallpaper-archive",
];

/// 列表条目：前端直接用 url / thumb 远程展示
#[derive(Serialize, Clone)]
pub struct BingWallpaper {
    /// 日期标识 YYYYMMDD（必应每日一张，同时作为本地缓存文件名）
    pub date: String,
    /// 图片标题（中文简述）
    pub title: String,
    /// 版权说明（含作者）
    pub copyright: String,
    /// 1920x1080 原图 URL（设为桌面时下载）
    pub url: String,
    /// 400x240 缩略图 URL（列表展示用）
    pub thumb: String,
}

/// 统一的阻塞式 GET（复用连接池，带超时与 UA，避免被必应拦截）
fn http_get(url: &str) -> Result<reqwest::blocking::Response, String> {
    static CLIENT: std::sync::OnceLock<reqwest::blocking::Client> = std::sync::OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            // 默认 UA 会被必应返回 403，伪装成浏览器 UA
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
            )
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    });
    client
        .get(url)
        .send()
        .map_err(|e| format!("请求必应壁纸接口失败：{e}"))
}

/// 把 1920x1080 原图 URL 改写为 400x240 缩略图 URL（失败时回退原 URL）
///
/// 必应 th?id 接口支持同一 id 的多档分辨率（尺寸体现在文件名中），
/// 因此直接改写尺寸段即可拿到小图，无需服务端参数。
fn to_thumb_url(full: &str) -> String {
    let replaced = full
        .replace("_1920x1080.jpg", "_400x240.jpg")
        .replace("_1920x1200.jpg", "_400x240.jpg")
        .replace("_UHD.jpg", "_400x240.jpg");
    if replaced == full {
        full.to_string()
    } else {
        replaced
    }
}

/// 把接口 JSON 解析成条目列表。
///
/// 官方 `HPImageArchive` 与社区归档源的响应**同构**
/// （`{images:[{enddate,url,urlbase,title,copyright}]}`），所以共用这一份。
///
/// 缩略图**优先用 `urlbase` 拼**：官方给的 `url` 带 `_1920x1080.jpg`，归档给的带
/// `_UHD.jpg&…&w=1920&h=1080` —— 后者做字符串替换会被尺寸参数抵消（实测改写后
/// 仍有 228KB）。`urlbase` 是干净的 `/th?id=OHR.xxx`，拼 `_400x240.jpg` 稳定得到
/// 约 31KB 的真缩略图。`urlbase` 缺失时才退回字符串改写。
fn parse_images(json: &serde_json::Value) -> Vec<BingWallpaper> {
    let Some(images) = json.get("images").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut list: Vec<BingWallpaper> = Vec::with_capacity(images.len());
    for img in images {
        let raw_url = img.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let date = img.get("enddate").and_then(|v| v.as_str()).unwrap_or("");
        if raw_url.is_empty() || date.is_empty() {
            continue;
        }
        let full = if raw_url.starts_with("http") {
            raw_url.to_string()
        } else {
            format!("{ORIGIN}{raw_url}")
        };
        let urlbase = img.get("urlbase").and_then(|v| v.as_str()).unwrap_or("");
        let thumb = if urlbase.is_empty() {
            to_thumb_url(&full)
        } else if urlbase.starts_with("http") {
            format!("{urlbase}_400x240.jpg")
        } else {
            format!("{ORIGIN}{urlbase}_400x240.jpg")
        };
        let title = img
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("必应壁纸")
            .trim()
            .to_string();
        let copyright = img
            .get("copyright")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        list.push(BingWallpaper {
            date: date.to_string(),
            title,
            copyright,
            url: full,
            thumb,
        });
    }
    list
}

/// 拉取单批壁纸（idx 为起始偏移）
fn fetch_batch(idx: usize) -> Result<Vec<BingWallpaper>, String> {
    let url = API_TMPL.replace("{idx}", &idx.to_string());
    let resp = http_get(&url)?;
    if !resp.status().is_success() {
        return Err(format!("必应壁纸接口返回 HTTP {}", resp.status()));
    }
    let text = resp
        .text()
        .map_err(|e| format!("读取必应壁纸响应失败：{e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析必应壁纸响应失败：{e}"))?;
    Ok(parse_images(&json))
}

/// 拉取必应每日壁纸列表（idx 0 + idx 8 两批；第二批失败不影响第一批结果）
pub fn fetch_wallpapers() -> Result<Vec<BingWallpaper>, String> {
    let mut out: Vec<BingWallpaper> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut first_err: Option<String> = None;

    for idx in [0usize, 8] {
        match fetch_batch(idx) {
            Ok(list) => {
                for w in list {
                    if seen.insert(w.date.clone()) {
                        out.push(w);
                    }
                }
            }
            // 首批失败说明网络/接口不可用，需向上报错；后续批次失败则静默忽略
            Err(e) => {
                if idx == 0 {
                    first_err = Some(e);
                }
            }
        }
    }

    if out.is_empty() {
        return Err(first_err.unwrap_or_else(|| "未获取到必应壁纸".into()));
    }
    Ok(out)
}

/// 从**社区归档源**拉取某一年月的全部壁纸（按月分片，约 30 条）。
///
/// 官方接口只能回溯 16 天，滚动看更早的必须走这里。依次尝试 [`ARCHIVE_BASES`]
/// 里的镜像；**某个月没有归档文件返回空列表而不是错误**（超出覆盖范围、早期缺月
/// 都是正常情况），调用方据此判断"再往前也没有了"。
///
/// 归档的月分片响应与官方接口同构，故直接喂给 [`parse_images`]。
pub fn fetch_archive_month(year: u32, month: u32) -> Result<Vec<BingWallpaper>, String> {
    let path = format!("/{year}/{month:02}/all.json");
    let mut last_err = String::from("归档源不可用");
    for base in ARCHIVE_BASES {
        let url = format!("{base}{path}");
        let resp = match http_get(&url) {
            Ok(r) => r,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        // 该月没有归档文件（超出仓库覆盖范围）⇒ 空结果，不是错误
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !resp.status().is_success() {
            last_err = format!("归档源返回 HTTP {}", resp.status());
            continue;
        }
        let text = match resp.text() {
            Ok(t) => t,
            Err(e) => {
                last_err = format!("读取归档数据失败：{e}");
                continue;
            }
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(json) => return Ok(parse_images(&json)),
            Err(e) => last_err = format!("解析归档数据失败：{e}"),
        }
    }
    Err(last_err)
}

/// 把必应原图下载到 `dir` 并返回本地绝对路径。
///
/// 同一天重复点击"设为壁纸"时直接复用已下载文件，不重复走网络。
/// 先写 `.part` 再改名，避免半截文件被后续流程当成完整图片使用。
pub fn download_wallpaper(url: &str, date: &str, dir: &Path) -> Result<String, String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("壁纸地址无效".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("创建下载目录失败 {}: {e}", dir.display()))?;

    let safe_date = if !date.is_empty() && date.chars().all(|c| c.is_ascii_digit()) {
        date.to_string()
    } else {
        "latest".to_string()
    };
    let dest: PathBuf = dir.join(format!("BingWallpaper_{safe_date}.jpg"));

    // 已下载且非空：直接复用（同一天多次设置壁纸不重复下载）
    if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(normalize(&dest));
    }

    let resp = http_get(url)?;
    if !resp.status().is_success() {
        return Err(format!("下载必应壁纸失败：HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("读取必应壁纸数据失败：{e}"))?;
    // 正常原图数百 KB，小于 4KB 视为异常响应（错误页/占位）
    if bytes.len() < 4096 {
        return Err("下载的图片数据异常（文件过小）".into());
    }

    let tmp = dir.join(format!("BingWallpaper_{safe_date}.jpg.part"));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入壁纸文件失败：{e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("保存壁纸文件失败：{e}")
    })?;

    Ok(normalize(&dest))
}

/// 路径统一为 Windows 反斜杠风格（与项目其它返回保持一致）
fn normalize(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\")
}

/// 从 `BingWallpaper_YYYYMMDD.jpg` 解析日期；不匹配返回 None。
///
/// 只认 [`download_wallpaper`] 自己的命名：`.jpg.part` 这类下载中途的半截文件
/// 结尾不是 `.jpg`，自然落选；用户手动塞进来的其它图片也不会被当成必应壁纸。
/// 扩展名大小写不敏感（Windows 上文件系统本就大小写不敏感）。
fn parse_cached_name(name: &str) -> Option<String> {
    let stem = name
        .to_ascii_lowercase()
        .strip_prefix("bingwallpaper_")?
        .strip_suffix(".jpg")?
        .to_string();
    if stem.len() == 8 && stem.bytes().all(|b| b.is_ascii_digit()) {
        Some(stem)
    } else {
        None
    }
}

/// 在**本地壁纸目录**里递归找出已下载的必应壁纸，返回 `(日期 YYYYMMDD, 本地绝对路径)`，
/// 按日期倒序。
///
/// **为什么扫本地壁纸目录，而不是扫下载目录？**
/// 这个结果要同时服务两件事：① 必应卡片上的"已下载"徽标；② 点徽标跳到「本地」
/// 选项卡后能定位到那张。只有让判据与"本地选项卡能否看到"共用同一个事实来源
/// （= 本地壁纸目录的递归扫描），两者才永远不会互相矛盾 —— 扫下载目录会在
/// 用户改过壁纸目录之后说谎（文件在下载目录里，却不在本地列表里）。
///
/// 顺带：判定是**实时扫描而非记忆**，所以目录来回改不会留下"幽灵徽标"。
/// 目录不存在 / 无权限一律返回空集合 —— 那不是错误。
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
        let Some(date) = parse_cached_name(name) else {
            continue;
        };
        // 一个日期一个文件；同日期重复出现时保留先遇到的（防御大小写敏感的
        // 文件系统上出现 BingWallpaper_/bingwallpaper_ 两份）
        if !seen.insert(date.clone()) {
            continue;
        }
        out.push((date, normalize(Path::new(&path))));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// 必应壁纸的下载目录：**始终是本地壁纸目录下的 `BingWallpaper` 子目录**。
///
/// 刻意做成"派生"而不是"独立可配置" —— 独立下载目录会在用户改过壁纸目录之后与
/// 本地列表脱钩（文件落在壁纸目录之外，「本地」选项卡看不到，而「必应」选项卡只放在线图），
/// 等于下载完就再也找不到。派生之后这个不变式结构上无法被破坏：
/// **下载下来的壁纸一定在本地壁纸目录里**。
///
/// `local_dir` 为空时退回默认图片目录（`%USERPROFILE%\Pictures`），
/// 与 `wallpaper::wallpaper_dirs()` 的默认口径一致。
pub fn download_dir(local_dir: Option<String>) -> PathBuf {
    resolve_save_dir(local_dir).join("BingWallpaper")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文件名解析：只认自家命名，且**绝不能**误伤用户自己的文件。
    #[test]
    fn parse_cached_name_only_accepts_own_output() {
        // 命中：`BingWallpaper_` + 恰好 8 位数字 + `.jpg`（扩展名大小写不敏感）
        let hits = [
            ("BingWallpaper_20260917.jpg", "20260917"),
            ("BingWallpaper_20261004.JPG", "20261004"),
            ("BINGWALLPAPER_20261010.jpg", "20261010"),
        ];
        for (name, want) in hits {
            assert_eq!(
                parse_cached_name(name).as_deref(),
                Some(want),
                "应命中：{name}"
            );
        }

        // 必须落选的：半截文件、日期位数不对、非本模块产物
        let misses = [
            "BingWallpaper_20261010.jpg.part",
            "BingWallpaper_2026101.jpg",
            "BingWallpaper_2026.jpg",
            "BingWallpaper_.jpg",
            "BingWallpaper_20261010.jpeg",
            "BingWallpaper_20261010",
            "BingWallpaper.jpg",
            // ★ 最关键的两条：用户自己的照片不能被当成必应壁纸
            "IMG_20260917.jpg",
            "日落_20260917.jpg",
            "my-photo.jpg",
        ];
        for name in misses {
            assert_eq!(parse_cached_name(name), None, "应落选：{name}");
        }
    }

    /// `download_wallpaper` 内联的命名格式必须与 [`parse_cached_name`] 对齐 ——
    /// 这是"下载完徽标能立刻点亮"的前提。命名是内联 `format!`（无独立函数），
    /// 故这里复刻同一行；**改命名规则时这里要同步改**。
    #[test]
    fn download_name_matches_parser() {
        for date in ["20260917", "20261010"] {
            let name = format!("BingWallpaper_{date}.jpg");
            assert_eq!(parse_cached_name(&name).as_deref(), Some(date));
        }
    }
}
