//! 共享 HTTP 客户端：统一连接池、超时、UA 与错误文案。
//!
//! 抽出来的原因有两个：
//! 1. `bing.rs` 与 `pixabay.rs` 各自维护一份 `Client` 会浪费连接池与 TLS 会话；
//! 2. 原来的错误文案写死"必应"，新数据源复用时会输出误导性提示
//!    （用户看到"请求必应壁纸接口失败"却是在拉 Pixabay，排查时会被带偏）。
//!
//! 本模块只负责**传输层**，不理解任何业务字段。

use std::sync::OnceLock;
use std::time::Duration;

/// 伪装成浏览器 —— 部分图库接口（含必应）对 reqwest 默认 UA 直接返回 403。
pub const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// 全局共享的阻塞式客户端（连接池复用，避免每次请求重建 TLS 会话）。
///
/// `build()` 理论上不会失败（没有加载证书文件、没有代理配置），
/// 但仍然兜底成默认客户端 —— 宁可少一层配置，也不能让整个壁纸功能起不来。
pub fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(BROWSER_UA)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// 发一个 GET，把网络层错误包装成中文文案。
///
/// `what` 是数据源的**人类可读名称**（"必应壁纸" / "Pixabay"），
/// 只用于错误提示，不参与请求本身。
///
/// 只处理"请求发不出去"这一类错误；HTTP 状态码由调用方判断
/// （不同数据源对 4xx 的语义不同，不能在这里一刀切）。
pub fn get(url: &str, what: &str) -> Result<reqwest::blocking::Response, String> {
    client()
        .get(url)
        .send()
        .map_err(|e| format!("请求{what}失败：{e}"))
}

/// 大文件下载用：单个请求覆盖客户端默认超时。
///
/// 全局客户端是 20 秒 —— 那是给"拉一段 JSON / 一张几百 KB 的图"定的。
/// 视频动辄几十 MB，20 秒在慢速网络上必然超时，报出来的却是"请求失败"，
/// 看起来像接口坏了。这里按请求放宽，且**只放宽这一个请求**，
/// 不影响其它接口保持快速失败。
pub fn get_long(url: &str, what: &str, secs: u64) -> Result<reqwest::blocking::Response, String> {
    client()
        .get(url)
        .timeout(Duration::from_secs(secs))
        .send()
        .map_err(|e| format!("请求{what}失败：{e}"))
}
