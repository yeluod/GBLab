//! `GBLab` 的唯一 SIP 事务上下文管理器。
//!
//! `siprs-transaction` 提供了完整 RFC 状态机，但当前共享 UDP 适配层需要把
//! 业务上下文、日志字段和 oneshot 响应等待统一绑定，因此这里保留一个最小、
//! 明确的 Non-INVITE/INVITE 基础事务表。重传和超时由同一发送入口负责。

use std::collections::HashMap;

use siprs::siprs_message::{Method, SipResponse};
use tokio::sync::{Mutex, oneshot};

/// SIP 事务匹配键。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TransactionKey {
    /// Call-ID。
    pub(crate) call_id: String,
    /// `CSeq` 序号。
    pub(crate) cseq: u32,
    /// `CSeq` 方法。
    pub(crate) method: Method,
    /// 顶层 Via branch。
    pub(crate) branch: String,
}

/// 事务向应用层暴露的上下文。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionContext {
    pub(crate) device_id: String,
    pub(crate) channel_id: Option<String>,
    pub(crate) method: Option<String>,
    pub(crate) command_type: Option<String>,
}

/// UDP 事务当前所处的最小状态集合。
///
/// 这里只覆盖模拟器当前真正使用到的客户端等待、完成和超时语义；
/// INVITE 的 ACK/CANCEL 关联信息仍由同一键保存，下一阶段可在此基础上扩展
/// RFC 3261 的完整状态机，而不再引入第二套 pending 表。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    /// 已创建但尚未收到最终响应。
    Trying,
    /// 已收到 2xx/3xx/4xx/5xx/6xx 最终响应。
    Completed,
    /// 等待窗口耗尽。
    TimedOut,
    /// 被调用方取消。
    Cancelled,
}

struct PendingTransaction {
    context: TransactionContext,
    sender: oneshot::Sender<SipResponse>,
    state: TransactionState,
    retransmissions: u8,
}

/// 单一 SIP 事务管理器。
#[derive(Default)]
pub struct TransactionManager {
    pending: Mutex<HashMap<TransactionKey, PendingTransaction>>,
}

impl TransactionManager {
    pub(crate) async fn register(
        &self,
        key: TransactionKey,
        context: TransactionContext,
        sender: oneshot::Sender<SipResponse>,
    ) {
        self.pending.lock().await.insert(
            key,
            PendingTransaction {
                context,
                sender,
                state: TransactionState::Trying,
                retransmissions: 0,
            },
        );
    }

    pub(crate) async fn context(&self, key: &TransactionKey) -> Option<TransactionContext> {
        self.pending
            .lock()
            .await
            .get(key)
            .map(|pending| pending.context.clone())
    }

    pub(crate) async fn take_final(
        &self,
        key: Option<&TransactionKey>,
    ) -> Option<oneshot::Sender<SipResponse>> {
        let key = key?;
        let sender = {
            let mut pending = self.pending.lock().await;
            let mut transaction = pending.remove(key)?;
            transaction.state = TransactionState::Completed;
            let sender = transaction.sender;
            drop(pending);
            sender
        };
        Some(sender)
    }

    /// 记录一次 UDP 重传，并返回当前累计次数。
    pub(crate) async fn record_retransmission(&self, key: &TransactionKey) -> Option<u8> {
        let count = {
            let mut pending = self.pending.lock().await;
            let transaction = pending.get_mut(key)?;
            transaction.retransmissions = transaction.retransmissions.saturating_add(1);
            let count = transaction.retransmissions;
            drop(pending);
            count
        };
        Some(count)
    }

    /// 将事务标记为超时或取消并移除，返回之前的状态。
    pub(crate) async fn finish_without_response(
        &self,
        key: &TransactionKey,
        requested_state: TransactionState,
    ) -> Option<TransactionState> {
        let state = {
            let mut pending = self.pending.lock().await;
            let mut transaction = pending.remove(key)?;
            transaction.state = requested_state;
            let state = transaction.state;
            drop(pending);
            state
        };
        Some(state)
    }

    /// 返回事务重传次数和状态，用于诊断和指标投影。
    #[cfg(test)]
    async fn metrics(&self, key: &TransactionKey) -> Option<(u8, TransactionState)> {
        let metrics = {
            let pending = self.pending.lock().await;
            let transaction = pending.get(key)?;
            let metrics = (transaction.retransmissions, transaction.state);
            drop(pending);
            metrics
        };
        Some(metrics)
    }

    pub(crate) async fn remove(&self, key: &TransactionKey) {
        self.pending.lock().await.remove(key);
    }

    #[cfg(test)]
    async fn len(&self) -> usize {
        self.pending.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use siprs::siprs_message::Method;
    use tokio::sync::oneshot;

    use super::{TransactionContext, TransactionKey, TransactionManager, TransactionState};

    fn key(cseq: u32, method: Method, branch: &str) -> TransactionKey {
        TransactionKey {
            call_id: "call-1".to_owned(),
            cseq,
            method,
            branch: branch.to_owned(),
        }
    }

    #[tokio::test]
    async fn manager_should_keep_transactions_distinct_by_cseq_method_and_branch() {
        let manager = TransactionManager::default();
        let context = TransactionContext {
            device_id: "34020000002000000100".to_owned(),
            channel_id: None,
            method: Some("MESSAGE".to_owned()),
            command_type: None,
        };
        let (sender_a, _receiver_a) = oneshot::channel();
        let (sender_b, _receiver_b) = oneshot::channel();
        manager
            .register(
                key(1, Method::Message, "branch-a"),
                context.clone(),
                sender_a,
            )
            .await;
        manager
            .register(key(2, Method::Message, "branch-a"), context, sender_b)
            .await;
        assert_eq!(manager.len().await, 2);
        assert!(
            manager
                .context(&key(1, Method::Message, "branch-a"))
                .await
                .is_some()
        );
        assert!(
            manager
                .context(&key(1, Method::Notify, "branch-a"))
                .await
                .is_none()
        );
        assert!(
            manager
                .context(&key(1, Method::Message, "branch-b"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn manager_should_track_retransmission_and_remove_timeout() {
        let manager = TransactionManager::default();
        let transaction_key = key(1, Method::Message, "branch-timeout");
        let context = TransactionContext {
            device_id: "34020000002000000100".to_owned(),
            channel_id: None,
            method: Some("MESSAGE".to_owned()),
            command_type: None,
        };
        let (sender, _receiver) = oneshot::channel();
        manager
            .register(transaction_key.clone(), context, sender)
            .await;
        assert_eq!(
            manager.record_retransmission(&transaction_key).await,
            Some(1)
        );
        assert_eq!(
            manager.record_retransmission(&transaction_key).await,
            Some(2)
        );
        let metrics = manager.metrics(&transaction_key).await;
        assert!(
            metrics.is_some(),
            "registered transaction should have metrics"
        );
        let (retransmissions, state) = metrics.unwrap_or((0, TransactionState::TimedOut));
        assert_eq!(retransmissions, 2);
        assert_eq!(state, TransactionState::Trying);
        assert_eq!(
            manager
                .finish_without_response(&transaction_key, TransactionState::TimedOut)
                .await,
            Some(TransactionState::TimedOut)
        );
        assert_eq!(manager.len().await, 0);
    }
}
