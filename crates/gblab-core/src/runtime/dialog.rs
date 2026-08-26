//! SIP 对话层。
//!
//! 事务只负责一次请求/响应交换；对话保存跨多个事务复用的 Call-ID、tag、URI、
//! target 和 CSeq。当前媒体尚未实现，但 INVITE/BYE/ACK 可以基于此层建立正确边界。

use std::collections::BTreeMap;

use serde::Serialize;

/// SIP 对话状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DialogState {
    /// 收到 INVITE 或发送初始请求后尚未确认。
    Early,
    /// 收到 2xx/ACK，对话已建立。
    Confirmed,
    /// 对话已通过 BYE 或取消终止。
    Terminated,
}

/// SIP 对话唯一键。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogId {
    /// Call-ID。
    pub call_id: String,
    /// 本地 tag。
    pub local_tag: String,
    /// 远端 tag。
    pub remote_tag: String,
}

impl DialogId {
    /// 构造对话键。
    #[must_use]
    pub fn new(
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
pub struct Dialog {
    /// 对话键。
    pub id: DialogId,
    /// 本地 URI。
    pub local_uri: String,
    /// 远端 URI。
    pub remote_uri: String,
    /// 远端 Contact/target。
    pub remote_target: Option<String>,
    /// 本地 `CSeq`。
    pub local_cseq: u32,
    /// 远端 `CSeq`。
    pub remote_cseq: u32,
    /// 当前状态。
    pub state: DialogState,
}

/// 单所有者对话管理器。
#[derive(Default)]
pub struct DialogManager {
    dialogs: BTreeMap<DialogId, Dialog>,
}

impl DialogManager {
    /// 创建一个早期对话；若键已存在则返回已有对话。
    pub fn create_or_get(&mut self, dialog: Dialog) -> Dialog {
        self.dialogs
            .entry(dialog.id.clone())
            .or_insert(dialog)
            .clone()
    }

    /// 按完整 Call-ID/tag 查找对话。
    #[must_use]
    pub fn get(&self, id: &DialogId) -> Option<&Dialog> {
        self.dialogs.get(id)
    }

    /// ACK 确认早期对话。
    pub fn confirm(&mut self, id: &DialogId, remote_cseq: u32) -> bool {
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
    pub fn accept_remote_cseq(&mut self, id: &DialogId, remote_cseq: u32) -> bool {
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
    pub fn terminate(&mut self, id: &DialogId) -> bool {
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
    pub fn retain_active(&mut self) {
        self.dialogs
            .retain(|_, dialog| dialog.state != DialogState::Terminated);
    }

    /// 当前对话数量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.dialogs.len()
    }

    /// 是否没有对话。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dialogs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialog, DialogId, DialogManager, DialogState};

    fn dialog() -> Dialog {
        Dialog {
            id: DialogId::new("call-1", "local-1", "remote-1"),
            local_uri: "sip:device@example.com".to_owned(),
            remote_uri: "sip:platform@example.com".to_owned(),
            remote_target: Some("sip:platform@192.0.2.1:5060".to_owned()),
            local_cseq: 1,
            remote_cseq: 1,
            state: DialogState::Early,
        }
    }

    #[test]
    fn dialog_should_create_confirm_and_terminate() {
        let mut manager = DialogManager::default();
        let dialog = dialog();
        let id = dialog.id.clone();
        manager.create_or_get(dialog);
        assert_eq!(manager.len(), 1);
        assert!(manager.confirm(&id, 2));
        assert_eq!(
            manager.get(&id).map(|item| item.state),
            Some(DialogState::Confirmed)
        );
        assert!(manager.terminate(&id));
        assert!(!manager.terminate(&id));
        manager.retain_active();
        assert!(manager.is_empty());
    }

    #[test]
    fn dialog_should_reject_cseq_regression_and_unknown_dialog() {
        let mut manager = DialogManager::default();
        let id = dialog().id;
        manager.create_or_get(dialog());
        assert!(!manager.accept_remote_cseq(&id, 0));
        assert!(manager.accept_remote_cseq(&id, 3));
        assert!(!manager.confirm(&DialogId::new("missing", "l", "r"), 1));
    }
}
