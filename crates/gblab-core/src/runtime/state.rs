//! 单 owner Supervisor 的内存状态与交互日志查询。

use std::collections::{BTreeMap, VecDeque};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{
    platform::SubscriptionManager,
    registration::BusinessCommand,
    types::{
        DeviceRegistrationSnapshot, DeviceRegistrationStatus, InteractionLog, RegistrationSnapshot,
    },
};

pub(super) const MAX_INTERACTION_LOGS: usize = 10_000;

pub(super) struct SupervisorState {
    pub(super) snapshot: RegistrationSnapshot,
    pub(super) devices: BTreeMap<String, DeviceRegistrationSnapshot>,
    pub(super) logs: VecDeque<InteractionLog>,
    pub(super) operation_cancellation: Option<CancellationToken>,
    pub(super) operation_total: usize,
    pub(super) initial_settled: usize,
    pub(super) next_operation_id: u64,
    pub(super) next_log_sequence: u64,
    pub(super) snapshot_dirty: bool,
    pub(super) pending_logs: Vec<InteractionLog>,
    pub(super) dropped_logs: u64,
    pub(super) business_tx: Option<mpsc::Sender<BusinessCommand>>,
    pub(super) subscriptions: SubscriptionManager,
}

impl SupervisorState {
    pub(super) fn new() -> Self {
        Self {
            snapshot: RegistrationSnapshot::default(),
            devices: BTreeMap::new(),
            logs: VecDeque::with_capacity(MAX_INTERACTION_LOGS),
            operation_cancellation: None,
            operation_total: 0,
            initial_settled: 0,
            next_operation_id: 1,
            next_log_sequence: 1,
            snapshot_dirty: false,
            pending_logs: Vec::new(),
            dropped_logs: 0,
            business_tx: None,
            subscriptions: SubscriptionManager::default(),
        }
    }

    pub(super) fn build_snapshot(&self) -> RegistrationSnapshot {
        let registered_count = self
            .devices
            .values()
            .filter(|device| device.status == DeviceRegistrationStatus::Registered)
            .count();
        let failed_count = self
            .devices
            .values()
            .filter(|device| device.status == DeviceRegistrationStatus::Failed)
            .count();
        let active_subscriptions = self.subscriptions.active_count();
        RegistrationSnapshot {
            operation_status: self.snapshot.operation_status,
            operation_id: self.snapshot.operation_id.clone(),
            total_devices: self.devices.len(),
            registered_count,
            failed_count,
            active_subscriptions,
            dropped_logs: self.dropped_logs,
        }
    }
}
