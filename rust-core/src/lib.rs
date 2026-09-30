// DotWallpaper 壁纸工具 - 核心库入口（macOS）
// 通过 C FFI 对外暴露功能。

pub mod cfmedia;
pub mod desktop;
pub mod displays;
pub mod engine;
pub mod ffi;
pub mod media;
pub mod runtime;
pub mod settings;
pub mod thumbs;
pub mod types;
pub use types::{AppSnapshot, FrontendSettings};

#[cfg(test)]
mod unit_tests;
