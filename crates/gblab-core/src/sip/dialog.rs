//! SIP 对话状态。
//!
//! 事务只负责单次请求/响应交换；对话保存跨多个事务复用的 Call-ID、tag、
//! URI、target 和 CSeq。它属于 SIP 适配层，而不是设备运行时状态。

use std::collections::BTreeMap;

use serde::Serialize;

/// SIP 对话状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DialogState {
    /// 收到 2xx/ACK，对话已建立。
    Confirmed,
    /// 对话已通过 BYE 或取消终止。
    Terminated,
}

/// SIP 对话唯一键。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DialogId {
    /// Call-ID。
    pub(super) call_id: String,
    /// 本地 tag。
    pub(super) local_tag: String,
    /// 远端 tag。
    pub(super) remote_tag: String,
}

impl DialogId {
    /// 构造对话键。
    #[must_use]
    pub(super) fn new(
        call_id: impl Into<String>,
        local_tag: impl Into<String>,
        remote_tag: impl Into<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            local_tag: local_tag.into(),
            remote_tag: remote_tag.into(),
        }
    }
}

/// 媒体前的最小 SIP 对话状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Dialog {
    /// 对话键。
    pub(crate) id: DialogId,
    /// 本地 URI。
    pub(crate) local_uri: String,
    /// 远端 URI。
    pub(crate) remote_uri: String,
    /// 远端 Contact/target。
    pub(crate) remote_target: Option<String>,
    /// 本地 `CSeq`。
    pub(crate) local_cseq: u32,
    /// 远端 `CSeq`。
    pub(crate) remote_cseq: u32,
    /// 当前状态。
    pub(crate) state: DialogState,
}

/// 单所有者对话管理器。
#[derive(Default)]
pub(super) struct DialogManager {
    dialogs: BTreeMap<DialogId, Dialog>,
}

#[expect(dead_code, reason = "INVITE 媒体阶段尚未启用，对话创建将在该阶段接入")]
impl DialogManager {
    /// 创建一个早期对话；若键已存在则返回已有对话。
    pub(super) fn create_or_get(&mut self, dialog: Dialog) -> Dialog {
        self.dialogs
            .entry(dialog.id.clone())
            .or_insert(dialog)
            .clone()
    }

    /// 按完整 Call-ID/tag 查找对话。
    #[must_use]
    pub(super) fn get(&self, id: &DialogId) -> Option<&Dialog> {
        self.dialogs.get(id)
    }

    /// ACK 确认早期对话。
    pub(super) fn confirm(&mut self, id: &DialogId, remote_cseq: u32) -> bool {
        let Some(dialog) = self.dialogs.get_mut(id) else {
            return false;
        };
        if dialog.state == DialogState::Terminated {
            return false;
        }
        dialog.remote_cseq = dialog.remote_cseq.max(remote_cseq);
        dialog.state = DialogState::Confirmed;
        true
    }

    /// 记录远端请求的 CSeq，拒绝回退的序号。
    pub(super) fn accept_remote_cseq(&mut self, id: &DialogId, remote_cseq: u32) -> bool {
        let Some(dialog) = self.dialogs.get_mut(id) else {
            return false;
        };
        if dialog.state == DialogState::Terminated || remote_cseq < dialog.remote_cseq {
            return false;
        }
        dialog.remote_cseq = remote_cseq;
        true
    }

    /// 终止对话；不存在或已终止时返回 false。
    pub(super) fn terminate(&mut self, id: &DialogId) -> bool {
        let Some(dialog) = self.dialogs.get_mut(id) else {
            return false;
        };
        if dialog.state == DialogState::Terminated {
            return false;
        }
        dialog.state = DialogState::Terminated;
        true
    }

    /// 清理已终止对话。
    pub(super) fn retain_active(&mut self) {
        self.dialogs
            .retain(|_, dialog| dialog.state != DialogState::Terminated);
    }

    /// 当前对话数量。
    #[must_use]
    pub(super) fn len(&self) -> usize {
        self.dialogs.len()
    }

    /// 是否没有对话。
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.dialogs.is_empty()
    }
}
