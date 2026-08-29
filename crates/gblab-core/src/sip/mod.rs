//! `siprs` 的 GB28181 设备端隔离层。

mod charset;
mod dispatcher;
pub(crate) mod notify;
mod registration;
mod session;
mod time;
pub(crate) mod transaction;
mod transport;

pub(crate) use dispatcher::parse_gb_xml;
pub(crate) use notify::NotifyDialogContext;
pub(crate) use registration::{
    SipLogDirection, SipRegistrationClient, SipRegistrationError, SipTransportEvent,
};
pub(crate) use session::DeviceSipSession;

/// 返回当前采用的 SIP 协议栈名称。
#[must_use]
pub const fn stack_name() -> &'static str {
    "siprs"
}
