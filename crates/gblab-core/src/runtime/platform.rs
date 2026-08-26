use std::collections::BTreeMap;

use serde::Serialize;

/// SIP 方法经过运行时归一化后的分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PlatformRequestMethod {
    /// MESSAGE 请求。
    Message,
    /// SUBSCRIBE 请求。
    Subscribe,
    /// NOTIFY 请求。
    Notify,
    /// OPTIONS 请求。
    Options,
    /// 未识别的方法。
    Unknown,
}

/// MANSCDP `CmdType` 经过归一化后的分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlatformCommandType {
    /// 目录查询。
    Catalog,
    /// 设备信息查询。
    DeviceInfo,
    /// 设备状态查询。
    DeviceStatus,
    /// 设备控制。
    DeviceControl,
    /// 录像信息查询。
    RecordInfo,
    /// 报警业务。
    Alarm,
    /// 移动位置业务。
    MobilePosition,
    /// 保活消息。
    Keepalive,
    /// 未识别的命令。
    Unknown,
}

/// 平台请求的运行时摘要。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRequest {
    /// SIP 方法。
    pub method: PlatformRequestMethod,
    /// XML 命令类型。
    pub command_type: PlatformCommandType,
    /// 请求中的设备编号。
    pub device_id: Option<String>,
    /// 请求中的通道编号。
    pub channel_id: Option<String>,
    /// 请求序号。
    pub sn: Option<String>,
    /// SIP Call-ID。
    pub call_id: Option<String>,
    /// 订阅有效期。
    pub expires: Option<u32>,
    /// 平台 From 头中的 tag。
    pub from_tag: Option<String>,
    /// 设备 200 OK 返回的 To tag。
    pub local_tag: Option<String>,
    /// 平台请求使用的 Event 头。
    pub event: Option<String>,
    /// 平台请求目标 URI。
    pub request_uri: Option<String>,
    /// Query 请求对应的设备响应 XML。
    pub response_body: Option<String>,
    /// 订阅建立后的首个 NOTIFY XML。
    pub initial_notify_body: Option<String>,
}

/// 订阅运行时状态。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionRuntimeStatus {
    /// 正在建立。
    Pending,
    /// 当前有效。
    Active,
    /// 正在刷新。
    Refreshing,
    /// 已取消。
    Cancelled,
    /// 已过期。
    Expired,
    /// 最近一次操作失败。
    Failed,
}

/// 单条平台订阅快照。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionSnapshot {
    /// 设备编号。
    pub device_id: String,
    /// 通道编号。
    pub channel_id: Option<String>,
    /// 订阅类型。
    pub command_type: PlatformCommandType,
    /// 平台 Call-ID。
    pub call_id: Option<String>,
    /// 当前状态。
    pub status: SubscriptionRuntimeStatus,
    /// 到期时间，Unix 毫秒。
    pub expires_at: Option<u64>,
    /// 最近一次通知时间，Unix 毫秒。
    pub last_notified_at: Option<u64>,
    /// 最近一次错误。
    pub last_error: Option<String>,
    /// 平台 SUBSCRIBE 的 From tag。
    pub remote_tag: Option<String>,
    /// 设备返回 200 OK 的 To tag。
    pub local_tag: Option<String>,
    /// 订阅 Event 头。
    pub event: Option<String>,
    /// 下一条 NOTIFY 使用的 `CSeq`。
    pub notify_cseq: u32,
}

/// 运行时订阅管理器。
#[derive(Default)]
pub struct SubscriptionManager {
    entries: BTreeMap<String, SubscriptionSnapshot>,
}

impl SubscriptionManager {
    /// 根据平台订阅请求建立或刷新订阅。
    pub fn subscribe(
        &mut self,
        request: &PlatformRequest,
        now_millis: u64,
    ) -> Option<SubscriptionSnapshot> {
        let device_id = request.device_id.clone()?;
        if !matches!(
            request.command_type,
            PlatformCommandType::Catalog
                | PlatformCommandType::Alarm
                | PlatformCommandType::MobilePosition
        ) {
            return None;
        }
        let key = subscription_key(
            &device_id,
            request.channel_id.as_deref(),
            request.command_type,
        );
        let expires_at = request
            .expires
            .filter(|expires| *expires > 0)
            .map(|expires| now_millis.saturating_add(u64::from(expires) * 1_000));
        let previous = self.entries.get(&key).cloned();
        let same_dialog = previous.as_ref().is_some_and(|entry| {
            if entry.call_id != request.call_id {
                return false;
            }
            entry.remote_tag == request.from_tag
        });
        let status = if previous.is_some() {
            SubscriptionRuntimeStatus::Refreshing
        } else {
            SubscriptionRuntimeStatus::Pending
        };
        let entry = SubscriptionSnapshot {
            device_id,
            channel_id: request.channel_id.clone(),
            command_type: request.command_type,
            call_id: request
                .call_id
                .clone()
                .or_else(|| previous.as_ref().and_then(|entry| entry.call_id.clone())),
            status,
            expires_at,
            last_notified_at: previous.as_ref().and_then(|entry| entry.last_notified_at),
            last_error: None,
            remote_tag: request
                .from_tag
                .clone()
                .or_else(|| previous.as_ref().and_then(|entry| entry.remote_tag.clone())),
            local_tag: if same_dialog {
                previous.as_ref().and_then(|entry| entry.local_tag.clone())
            } else {
                request.local_tag.clone()
            },
            event: request
                .event
                .clone()
                .or_else(|| previous.as_ref().and_then(|entry| entry.event.clone())),
            notify_cseq: if same_dialog {
                previous.as_ref().map_or(1, |entry| entry.notify_cseq)
            } else {
                1
            },
        };
        self.entries.insert(key.clone(), entry);
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.status = SubscriptionRuntimeStatus::Active;
        }
        self.entries.get(&key).cloned()
    }

    /// 处理平台取消订阅。
    pub fn cancel(&mut self, request: &PlatformRequest) -> Option<SubscriptionSnapshot> {
        let device_id = request.device_id.as_deref()?;
        let key = subscription_key(
            device_id,
            request.channel_id.as_deref(),
            request.command_type,
        );
        let entry = self.entries.get_mut(&key)?;
        entry.status = SubscriptionRuntimeStatus::Cancelled;
        Some(entry.clone())
    }

    /// 查找当前仍然有效、可用于发送事件通知的订阅。
    #[must_use]
    pub fn active(
        &self,
        device_id: &str,
        channel_id: Option<&str>,
        command_type: PlatformCommandType,
        now_millis: u64,
    ) -> Option<SubscriptionSnapshot> {
        let key = subscription_key(device_id, channel_id, command_type);
        let entry = self.entries.get(&key)?;
        if entry.status != SubscriptionRuntimeStatus::Active {
            return None;
        }
        if entry
            .expires_at
            .is_some_and(|expires_at| expires_at <= now_millis)
        {
            return None;
        }
        Some(entry.clone())
    }

    /// 返回当前有效订阅并预留一条 NOTIFY 的 `CSeq`。
    pub fn next_notify(
        &mut self,
        device_id: &str,
        channel_id: Option<&str>,
        command_type: PlatformCommandType,
        now_millis: u64,
    ) -> Option<SubscriptionSnapshot> {
        let key = self
            .entries
            .contains_key(&subscription_key(device_id, channel_id, command_type))
            .then(|| subscription_key(device_id, channel_id, command_type))
            .or_else(|| {
                let fallback = subscription_key(device_id, None, command_type);
                self.entries.contains_key(&fallback).then_some(fallback)
            })?;
        let entry = self.entries.get_mut(&key)?;
        if entry.status != SubscriptionRuntimeStatus::Active
            || entry
                .expires_at
                .is_some_and(|expires_at| expires_at <= now_millis)
        {
            return None;
        }
        let snapshot = entry.clone();
        entry.notify_cseq = entry.notify_cseq.saturating_add(1);
        Some(snapshot)
    }

    /// 清理已经到期的订阅。
    pub fn expire(&mut self, now_millis: u64) {
        for entry in self.entries.values_mut() {
            if entry.status == SubscriptionRuntimeStatus::Active
                && entry
                    .expires_at
                    .is_some_and(|expires_at| expires_at <= now_millis)
            {
                entry.status = SubscriptionRuntimeStatus::Expired;
            }
        }
    }

    /// 标记一次通知发送时间。
    pub fn mark_notified(
        &mut self,
        device_id: &str,
        channel_id: Option<&str>,
        command_type: PlatformCommandType,
        now_millis: u64,
    ) {
        let key = self
            .find_key(device_id, channel_id, command_type)
            .unwrap_or_else(|| subscription_key(device_id, channel_id, command_type));
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_notified_at = Some(now_millis);
        }
    }

    /// 记录一次通知失败，保留订阅以便界面展示和后续刷新。
    pub fn mark_failed(
        &mut self,
        device_id: &str,
        channel_id: Option<&str>,
        command_type: PlatformCommandType,
        error: String,
    ) {
        let key = self
            .find_key(device_id, channel_id, command_type)
            .unwrap_or_else(|| subscription_key(device_id, channel_id, command_type));
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.status = SubscriptionRuntimeStatus::Failed;
            entry.last_error = Some(error);
        }
    }

    /// 返回当前订阅快照。
    #[must_use]
    pub fn snapshots(&self) -> Vec<SubscriptionSnapshot> {
        self.entries.values().cloned().collect()
    }

    /// 清空当前注册生命周期产生的运行时订阅。
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn find_key(
        &self,
        device_id: &str,
        channel_id: Option<&str>,
        command_type: PlatformCommandType,
    ) -> Option<String> {
        let exact = subscription_key(device_id, channel_id, command_type);
        if self.entries.contains_key(&exact) {
            return Some(exact);
        }
        let fallback = subscription_key(device_id, None, command_type);
        self.entries.contains_key(&fallback).then_some(fallback)
    }
}

fn subscription_key(
    device_id: &str,
    channel_id: Option<&str>,
    command_type: PlatformCommandType,
) -> String {
    format!(
        "{device_id}:{}:{command_type:?}",
        channel_id.unwrap_or_default()
    )
}
