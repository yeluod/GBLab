//! NOTIFY 请求需要的最小对话上下文。
//!
//! 此类型刻意不依赖运行时订阅模型，避免 SIP 适配层反向依赖 Runtime。

/// 发送一条 NOTIFY 所需的对话字段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotifyDialogContext {
    pub(crate) call_id: Option<String>,
    pub(crate) remote_tag: Option<String>,
    pub(crate) local_tag: Option<String>,
    pub(crate) event: Option<String>,
    pub(crate) cseq: u32,
}
