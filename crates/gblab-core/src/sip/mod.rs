//! `siprs` 的 GB28181 设备端隔离层。

/// 返回当前采用的 SIP 协议栈名称。
#[must_use]
pub const fn stack_name() -> &'static str {
    "siprs"
}
