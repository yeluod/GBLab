//! `GBLab` 的高性能 GB28181 多设备模拟核心。
//!
//! 本 crate 独立于 Tauri，负责领域模型、设备与场景编排、Tokio 生命周期、
//! SIP/媒体适配、JSON 配置存储及可观测性。

#![deny(missing_docs)]

pub mod application;
pub mod configuration;
pub mod domain;
pub mod media;
pub mod observability;
pub mod runtime;
pub mod sip;

mod error;

pub use application::{CoreInfo, CoreService};
pub use error::{CoreError, Result};
