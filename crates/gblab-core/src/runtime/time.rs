//! 运行时统一时间工具。

use std::time::SystemTime;

/// 当前 Unix 毫秒；系统时间早于 Epoch 时返回 0。
#[must_use]
pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
