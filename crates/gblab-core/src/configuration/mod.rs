//! JSON 配置文件的读取与写入。

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 保存在 JSON 文件中的应用配置。
///
/// 仅保存用户配置；设备注册状态、SIP 消息和交互日志等运行时数据不会落盘。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AppConfiguration {
    /// 配置文件格式版本。
    pub schema_version: u8,
}

/// 只承载配置文件路径与已校验配置的 JSON 存储。
pub struct ConfigurationStore {
    path: PathBuf,
    configuration: AppConfiguration,
}

impl ConfigurationStore {
    /// 读取已有 JSON 配置；文件不存在时创建默认配置。
    ///
    /// # Errors
    ///
    /// 无法创建目录、读写文件或解析 JSON 时返回错误。
    pub fn open(path: &Path) -> Result<Self, ConfigurationError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let configuration = if path.exists() {
            let contents = fs::read_to_string(path)?;
            serde_json::from_str(&contents)?
        } else {
            let configuration = AppConfiguration { schema_version: 1 };
            let contents = serde_json::to_string_pretty(&configuration)?;
            fs::write(path, contents)?;
            configuration
        };

        Ok(Self {
            path: path.to_owned(),
            configuration,
        })
    }

    /// 返回当前 JSON 配置是否已就绪。
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.path.is_file() && self.configuration.schema_version > 0
    }
}

/// JSON 配置文件的访问错误。
#[derive(Debug, Error)]
pub enum ConfigurationError {
    /// 文件系统访问失败。
    #[error("配置文件访问失败: {0}")]
    Io(#[from] io::Error),
    /// JSON 格式不合法。
    #[error("配置 JSON 格式错误: {0}")]
    Json(#[from] serde_json::Error),
}
