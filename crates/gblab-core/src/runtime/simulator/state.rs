//! 本地模拟运行时的单所有者状态与状态转换。

use std::collections::{BTreeMap, VecDeque};

use serde_json::json;

use crate::{SimulatedDevice, domain::derive_channels_for_device};

use super::SimulatorRuntimeError;
use super::types::{
    AlarmCommand, ChannelRuntimeState, ConnectivityState, DeviceControlCommand, DeviceRuntimeState,
    ExecutionMode, FaultProfile, OperationId, OperationRecord, OperationStatus, OperationTarget,
    PositionCommand, PositionSimulationMode, PtzCommand, PtzMotion, PtzPreset, QueryId, QueryKind,
    QueryRequest, QueryResult, RecordingCommand, RecordingEntry, RecordingRuntimeStatus,
    RuntimeEventLevel, RuntimeEventRecord, ScenarioDefinition, ScenarioId, ScenarioRuntimeState,
    ScenarioStatus, SimulatorRuntimeSnapshot, SubscriptionCommand, TransactionRecord,
};
use crate::runtime::time::now_millis;

const MAX_OPERATIONS: usize = 2_000;
const MAX_EVENTS: usize = 5_000;
const MAX_QUERIES: usize = 1_000;
const MAX_TRANSACTIONS: usize = 2_000;
const MAX_RECORDINGS: usize = 5_000;

pub(super) struct SimulatorState {
    revision: u64,
    next_id: u64,
    devices: BTreeMap<String, DeviceRuntimeState>,
    device_configs: BTreeMap<String, SimulatedDevice>,
    operations: VecDeque<OperationRecord>,
    events: VecDeque<RuntimeEventRecord>,
    queries: VecDeque<QueryResult>,
    transactions: VecDeque<TransactionRecord>,
    recordings: VecDeque<RecordingEntry>,
    fault_profile: FaultProfile,
    restart_deadlines: BTreeMap<String, u64>,
    scenarios: BTreeMap<ScenarioId, ScenarioDefinition>,
    scenario_states: BTreeMap<ScenarioId, ScenarioRuntimeState>,
}

impl SimulatorState {
    pub(super) fn new(devices: Vec<SimulatedDevice>) -> Self {
        let mut state = Self {
            revision: 0,
            next_id: 1,
            devices: BTreeMap::new(),
            device_configs: BTreeMap::new(),
            operations: VecDeque::with_capacity(MAX_OPERATIONS),
            events: VecDeque::with_capacity(MAX_EVENTS),
            queries: VecDeque::with_capacity(MAX_QUERIES),
            transactions: VecDeque::with_capacity(MAX_TRANSACTIONS),
            recordings: VecDeque::with_capacity(MAX_RECORDINGS),
            fault_profile: FaultProfile::default(),
            restart_deadlines: BTreeMap::new(),
            scenarios: BTreeMap::new(),
            scenario_states: BTreeMap::new(),
        };
        state.sync_devices(devices);
        state
    }

    pub(super) fn sync_devices(&mut self, devices: Vec<SimulatedDevice>) {
        let mut next_states = BTreeMap::new();
        let mut next_configs = BTreeMap::new();
        for device in devices {
            let device_id = device.id.to_string();
            let channels = derive_channels_for_device(&device).unwrap_or_default();
            let previous = self.devices.remove(&device_id);
            let channel_states = channels
                .into_iter()
                .map(|channel| {
                    previous
                        .as_ref()
                        .and_then(|state| {
                            state
                                .channels
                                .iter()
                                .find(|item| item.channel_id == channel.id.to_string())
                                .cloned()
                        })
                        .unwrap_or_else(|| ChannelRuntimeState {
                            channel_id: channel.id.to_string(),
                            name: channel.name,
                            online: true,
                            ..ChannelRuntimeState::default()
                        })
                })
                .collect();
            let runtime = match previous {
                None => DeviceRuntimeState {
                    device_id: device_id.clone(),
                    name: device.name.clone(),
                    connectivity: ConnectivityState::Online,
                    guarded: false,
                    clock_offset_millis: 0,
                    last_platform_request_at: None,
                    last_operation_id: None,
                    channels: channel_states,
                },
                Some(mut state) => {
                    state.name.clone_from(&device.name);
                    state.channels = channel_states;
                    state
                }
            };
            next_states.insert(device_id.clone(), runtime);
            next_configs.insert(device_id, device);
        }
        self.devices = next_states;
        self.device_configs = next_configs;
        self.restart_deadlines
            .retain(|device_id, _| self.devices.contains_key(device_id));
        self.touch();
    }

    pub(super) fn snapshot(&self) -> SimulatorRuntimeSnapshot {
        SimulatorRuntimeSnapshot {
            revision: self.revision,
            devices: self.devices.values().cloned().collect(),
            active_scenarios: self
                .scenario_states
                .values()
                .filter(|state| state.status == ScenarioStatus::Running)
                .count(),
            fault_profile: self.fault_profile.clone(),
        }
    }

    pub(super) fn operations(&self) -> Vec<OperationRecord> {
        self.operations.iter().rev().cloned().collect()
    }

    pub(super) fn events(&self) -> Vec<RuntimeEventRecord> {
        self.events.iter().rev().cloned().collect()
    }

    pub(super) fn queries(&self) -> Vec<QueryResult> {
        self.queries.iter().rev().cloned().collect()
    }

    pub(super) fn transactions(&self) -> Vec<TransactionRecord> {
        self.transactions.iter().rev().cloned().collect()
    }

    pub(super) fn recordings(&self) -> Vec<RecordingEntry> {
        self.recordings.iter().rev().cloned().collect()
    }

    pub(super) fn scenarios(&self) -> Vec<ScenarioRuntimeState> {
        self.scenario_states.values().cloned().collect()
    }

    pub(super) const fn command_delay_millis(&self) -> u64 {
        self.fault_profile.delay_millis
    }

    pub(super) fn set_fault_profile(
        &mut self,
        mut profile: FaultProfile,
    ) -> Result<(), SimulatorRuntimeError> {
        if profile.packet_loss_percent > 100 {
            return Err(SimulatorRuntimeError::InvalidInput(
                "丢包比例必须位于 0 至 100".to_owned(),
            ));
        }
        if profile
            .reject_status
            .is_some_and(|status| !(400..=699).contains(&status))
        {
            return Err(SimulatorRuntimeError::InvalidInput(
                "拒绝状态码必须位于 400 至 699".to_owned(),
            ));
        }
        profile.delay_millis = profile.delay_millis.min(60_000);
        self.fault_profile = profile;
        self.touch();
        self.push_event(
            "faultProfileUpdated",
            RuntimeEventLevel::Info,
            None,
            None,
            None,
            "故障注入配置已更新".to_owned(),
        );
        Ok(())
    }

    pub(super) fn control_device(
        &mut self,
        device_id: &str,
        command: DeviceControlCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "deviceControl", mode, now)?;
        let mut restart_deadline = None;
        let device = self.device_mut(device_id)?;
        match command {
            DeviceControlCommand::Restart { duration_seconds } => {
                if !(1..=300).contains(&duration_seconds) {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "重启时长必须位于 1 至 300 秒".to_owned(),
                    ));
                }
                device.connectivity = ConnectivityState::Restarting;
                for channel in &mut device.channels {
                    channel.online = false;
                }
                restart_deadline = Some(now.saturating_add(u64::from(duration_seconds) * 1_000));
            }
            DeviceControlCommand::Guard => device.guarded = true,
            DeviceControlCommand::Unguard => device.guarded = false,
            DeviceControlCommand::AlarmReset => {
                for channel in &mut device.channels {
                    restore_alarm(channel, now);
                }
            }
            DeviceControlCommand::SetTime { offset_millis } => {
                device.clock_offset_millis = offset_millis;
            }
            DeviceControlCommand::SetOnline => {
                device.connectivity = ConnectivityState::Online;
                for channel in &mut device.channels {
                    channel.online = true;
                }
            }
            DeviceControlCommand::SetOffline => {
                device.connectivity = ConnectivityState::Offline;
                for channel in &mut device.channels {
                    channel.online = false;
                }
            }
        }
        device.last_operation_id = Some(operation_id.clone());
        if let Some(deadline) = restart_deadline {
            self.restart_deadlines
                .insert(device_id.to_owned(), deadline);
        }
        let record = self.complete_operation(
            operation_id,
            "deviceControl",
            mode,
            Some(device_id),
            None,
            now,
        );
        self.push_event(
            "deviceControl",
            RuntimeEventLevel::Info,
            Some(device_id),
            None,
            Some(record.id.clone()),
            "设备控制已应用到本地运行状态".to_owned(),
        );
        Ok(record)
    }

    pub(super) fn control_ptz(
        &mut self,
        device_id: &str,
        channel_id: &str,
        command: PtzCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "ptz", mode, now)?;
        let channel = self.channel_mut(device_id, channel_id)?;
        if !channel.online {
            return Err(SimulatorRuntimeError::DeviceOffline(device_id.to_owned()));
        }
        apply_ptz_command(channel, command, now)?;
        channel.last_operation_id = Some(operation_id.clone());
        let record = self.complete_operation(
            operation_id,
            "ptz",
            mode,
            Some(device_id),
            Some(channel_id),
            now,
        );
        self.push_event(
            "ptzChanged",
            RuntimeEventLevel::Info,
            Some(device_id),
            Some(channel_id),
            Some(record.id.clone()),
            "PTZ 状态已更新".to_owned(),
        );
        Ok(record)
    }

    pub(super) fn update_alarm(
        &mut self,
        device_id: &str,
        channel_id: &str,
        command: AlarmCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        validate_alarm(&command)?;
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "alarm", mode, now)?;
        let channel = self.channel_mut(device_id, channel_id)?;
        if command.active {
            channel.alarm.active = true;
            channel.alarm.priority = Some(command.priority);
            channel.alarm.method = Some(command.method);
            channel.alarm.alarm_type = command.alarm_type;
            channel.alarm.description = Some(command.description);
            channel.alarm.occurred_at = Some(now);
            channel.alarm.restored_at = None;
            channel.alarm.interval_seconds = command.interval_seconds;
            channel.alarm.next_trigger_at = command
                .interval_seconds
                .map(|seconds| now.saturating_add(u64::from(seconds) * 1_000));
        } else {
            restore_alarm(channel, now);
        }
        channel.last_operation_id = Some(operation_id.clone());
        let record = self.complete_operation(
            operation_id,
            if command.active {
                "alarmOccur"
            } else {
                "alarmRestore"
            },
            mode,
            Some(device_id),
            Some(channel_id),
            now,
        );
        self.push_event(
            if command.active {
                "alarmOccurred"
            } else {
                "alarmRestored"
            },
            RuntimeEventLevel::Info,
            Some(device_id),
            Some(channel_id),
            Some(record.id.clone()),
            if command.active {
                "通道报警已发生".to_owned()
            } else {
                "通道报警已恢复".to_owned()
            },
        );
        Ok(record)
    }

    pub(super) fn update_position(
        &mut self,
        device_id: &str,
        channel_id: &str,
        command: PositionCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        validate_position(&command)?;
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "mobilePosition", mode, now)?;
        let channel = self.channel_mut(device_id, channel_id)?;
        channel.position.longitude = command.longitude;
        channel.position.latitude = command.latitude;
        channel.position.speed = command.speed;
        channel.position.direction = command.direction;
        channel.position.altitude = command.altitude;
        channel.position.mode = command.mode;
        channel.position.running = command.running;
        channel.position.interval_seconds = command.interval_seconds;
        channel.position.updated_at = Some(now);
        channel.position.next_report_at = if command.running {
            command
                .interval_seconds
                .map(|seconds| now.saturating_add(u64::from(seconds) * 1_000))
        } else {
            None
        };
        channel.last_operation_id = Some(operation_id.clone());
        let record = self.complete_operation(
            operation_id,
            "mobilePosition",
            mode,
            Some(device_id),
            Some(channel_id),
            now,
        );
        self.push_event(
            "positionUpdated",
            RuntimeEventLevel::Info,
            Some(device_id),
            Some(channel_id),
            Some(record.id.clone()),
            "通道移动位置已更新".to_owned(),
        );
        Ok(record)
    }

    pub(super) fn control_recording(
        &mut self,
        device_id: &str,
        channel_id: &str,
        command: RecordingCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "recording", mode, now)?;
        let mut completed_entry = None;
        let channel = self.channel_mut(device_id, channel_id)?;
        match command {
            RecordingCommand::Start { name } => {
                if name.trim().is_empty() {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "录像名称不能为空".to_owned(),
                    ));
                }
                if channel.recording.status != RecordingRuntimeStatus::Idle {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "当前通道已有录像任务".to_owned(),
                    ));
                }
                channel.recording.status = RecordingRuntimeStatus::Recording;
                channel.recording.current_file = Some(name);
                channel.recording.started_at = Some(now);
                channel.recording.duration_millis = 0;
                channel.recording.last_error = None;
            }
            RecordingCommand::Pause => {
                if channel.recording.status != RecordingRuntimeStatus::Recording {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "只有录制中的任务可以暂停".to_owned(),
                    ));
                }
                channel.recording.duration_millis = channel
                    .recording
                    .started_at
                    .map_or(0, |started_at| now.saturating_sub(started_at));
                channel.recording.status = RecordingRuntimeStatus::Paused;
            }
            RecordingCommand::Resume => {
                if channel.recording.status != RecordingRuntimeStatus::Paused {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "只有暂停的任务可以继续".to_owned(),
                    ));
                }
                channel.recording.started_at =
                    Some(now.saturating_sub(channel.recording.duration_millis));
                channel.recording.status = RecordingRuntimeStatus::Recording;
            }
            RecordingCommand::Stop => {
                if channel.recording.status == RecordingRuntimeStatus::Idle {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "当前通道没有录像任务".to_owned(),
                    ));
                }
                let started_at = channel.recording.started_at.unwrap_or(now);
                let duration_millis =
                    if channel.recording.status == RecordingRuntimeStatus::Recording {
                        now.saturating_sub(started_at)
                    } else {
                        channel.recording.duration_millis
                    };
                completed_entry = Some((
                    channel
                        .recording
                        .current_file
                        .clone()
                        .unwrap_or_else(|| "模拟录像".to_owned()),
                    started_at,
                    duration_millis,
                ));
                channel.recording = super::types::RecordingRuntimeState::default();
            }
        }
        channel.last_operation_id = Some(operation_id.clone());
        if let Some((name, started_at, duration_millis)) = completed_entry {
            let entry = RecordingEntry {
                id: format!("recording-{}", self.allocate_id()),
                device_id: device_id.to_owned(),
                channel_id: channel_id.to_owned(),
                name,
                started_at,
                ended_at: started_at.saturating_add(duration_millis),
                record_type: "manual".to_owned(),
                size_bytes: 0,
                file_path: None,
            };
            push_bounded(&mut self.recordings, entry, MAX_RECORDINGS);
        }
        let record = self.complete_operation(
            operation_id,
            "recording",
            mode,
            Some(device_id),
            Some(channel_id),
            now,
        );
        self.push_event(
            "recordingChanged",
            RuntimeEventLevel::Info,
            Some(device_id),
            Some(channel_id),
            Some(record.id.clone()),
            "通道录像状态已更新".to_owned(),
        );
        Ok(record)
    }

    pub(super) fn control_subscription(
        &mut self,
        device_id: &str,
        channel_id: &str,
        command: SubscriptionCommand,
        mode: ExecutionMode,
    ) -> Result<OperationRecord, SimulatorRuntimeError> {
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(device_id, &operation_id, "subscription", mode, now)?;
        let channel = self.channel_mut(device_id, channel_id)?;
        let subscription_kind = match &command {
            SubscriptionCommand::Upsert {
                subscription_kind,
                expires_seconds,
            } => {
                if !(1..=86_400).contains(expires_seconds) {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "订阅有效期必须位于 1 至 86400 秒".to_owned(),
                    ));
                }
                subscription_kind.clone()
            }
            SubscriptionCommand::Cancel { subscription_kind }
            | SubscriptionCommand::Fail {
                subscription_kind, ..
            } => subscription_kind.clone(),
        };
        if !["Catalog", "Alarm", "MobilePosition"].contains(&subscription_kind.as_str()) {
            return Err(SimulatorRuntimeError::InvalidInput(
                "订阅类型只支持 Catalog、Alarm 或 MobilePosition".to_owned(),
            ));
        }
        let index = channel
            .subscriptions
            .iter()
            .position(|item| item.kind == subscription_kind);
        match command {
            SubscriptionCommand::Upsert {
                expires_seconds, ..
            } => {
                let value = super::types::ChannelSubscriptionState {
                    kind: subscription_kind,
                    status: "active".to_owned(),
                    expires_at: Some(
                        now.saturating_add(u64::from(expires_seconds).saturating_mul(1_000)),
                    ),
                    last_notified_at: None,
                    last_error: None,
                };
                if let Some(index) = index {
                    channel.subscriptions[index] = value;
                } else {
                    channel.subscriptions.push(value);
                }
            }
            SubscriptionCommand::Cancel { .. } => {
                let Some(index) = index else {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "目标订阅不存在".to_owned(),
                    ));
                };
                "cancelled".clone_into(&mut channel.subscriptions[index].status);
                channel.subscriptions[index].expires_at = None;
            }
            SubscriptionCommand::Fail { error, .. } => {
                if error.trim().is_empty() {
                    return Err(SimulatorRuntimeError::InvalidInput(
                        "订阅失败原因不能为空".to_owned(),
                    ));
                }
                let value = super::types::ChannelSubscriptionState {
                    kind: subscription_kind,
                    status: "failed".to_owned(),
                    expires_at: None,
                    last_notified_at: None,
                    last_error: Some(error),
                };
                if let Some(index) = index {
                    channel.subscriptions[index] = value;
                } else {
                    channel.subscriptions.push(value);
                }
            }
        }
        channel.last_operation_id = Some(operation_id.clone());
        let record = self.complete_operation(
            operation_id,
            "subscription",
            mode,
            Some(device_id),
            Some(channel_id),
            now,
        );
        self.push_event(
            "subscriptionChanged",
            RuntimeEventLevel::Info,
            Some(device_id),
            Some(channel_id),
            Some(record.id.clone()),
            "通道订阅状态已更新".to_owned(),
        );
        Ok(record)
    }

    pub(super) fn execute_query(
        &mut self,
        request: QueryRequest,
    ) -> Result<QueryResult, SimulatorRuntimeError> {
        let now = now_millis();
        let operation_id = self.next_operation_id();
        self.check_fault(
            &request.device_id,
            &operation_id,
            "query",
            request.mode,
            now,
        )?;
        if request.mode == ExecutionMode::Platform {
            return Err(SimulatorRuntimeError::PlatformAdapterUnavailable);
        }
        let response = self.build_query_response(&request)?;
        let completed_at = now_millis();
        let result = QueryResult {
            id: QueryId(format!("query-{}", self.allocate_id())),
            request,
            status: OperationStatus::Succeeded,
            response: Some(response),
            error: None,
            started_at: now,
            completed_at,
            duration_millis: completed_at.saturating_sub(now),
            operation_id: operation_id.clone(),
        };
        push_bounded(&mut self.queries, result.clone(), MAX_QUERIES);
        let _ = self.complete_operation(
            operation_id.clone(),
            "query",
            ExecutionMode::LocalSimulation,
            Some(&result.request.device_id),
            result.request.channel_id.as_deref(),
            now,
        );
        self.push_event(
            "queryCompleted",
            RuntimeEventLevel::Info,
            Some(&result.request.device_id),
            result.request.channel_id.as_deref(),
            Some(operation_id),
            format!("{:?} 查询完成", result.request.kind),
        );
        Ok(result)
    }

    pub(super) fn save_scenario(
        &mut self,
        mut definition: ScenarioDefinition,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        if definition.name.trim().is_empty() || definition.steps.is_empty() {
            return Err(SimulatorRuntimeError::InvalidInput(
                "场景名称和步骤不能为空".to_owned(),
            ));
        }
        for step in &definition.steps {
            if step.name.trim().is_empty() || !self.devices.contains_key(&step.device_id) {
                return Err(SimulatorRuntimeError::InvalidInput(
                    "场景步骤名称或目标设备无效".to_owned(),
                ));
            }
        }
        let id = definition
            .id
            .clone()
            .unwrap_or_else(|| ScenarioId(format!("scenario-{}", self.allocate_id())));
        definition.id = Some(id.clone());
        let runtime = ScenarioRuntimeState {
            id: id.clone(),
            name: definition.name.clone(),
            status: ScenarioStatus::Idle,
            current_step: 0,
            total_steps: definition.steps.len(),
            next_step_at: None,
            last_error: None,
        };
        self.scenarios.insert(id.clone(), definition);
        self.scenario_states.insert(id, runtime.clone());
        self.touch();
        Ok(runtime)
    }

    pub(super) fn start_scenario(
        &mut self,
        id: &ScenarioId,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        if !self.scenarios.contains_key(id) {
            return Err(SimulatorRuntimeError::ScenarioNotFound(id.0.clone()));
        }
        let now = now_millis();
        let state = self
            .scenario_states
            .get_mut(id)
            .ok_or_else(|| SimulatorRuntimeError::ScenarioNotFound(id.0.clone()))?;
        state.status = ScenarioStatus::Running;
        state.current_step = 0;
        state.next_step_at = Some(now);
        state.last_error = None;
        let result = state.clone();
        self.touch();
        Ok(result)
    }

    pub(super) fn set_scenario_status(
        &mut self,
        id: &ScenarioId,
        status: ScenarioStatus,
    ) -> Result<ScenarioRuntimeState, SimulatorRuntimeError> {
        let state = self
            .scenario_states
            .get_mut(id)
            .ok_or_else(|| SimulatorRuntimeError::ScenarioNotFound(id.0.clone()))?;
        match status {
            ScenarioStatus::Running => {
                state.status = ScenarioStatus::Running;
                state.next_step_at = Some(now_millis());
            }
            ScenarioStatus::Paused => {
                state.status = ScenarioStatus::Paused;
                state.next_step_at = None;
            }
            ScenarioStatus::Stopped => {
                state.status = ScenarioStatus::Stopped;
                state.current_step = 0;
                state.next_step_at = None;
            }
            _ => {
                return Err(SimulatorRuntimeError::InvalidInput(
                    "不支持的场景状态变更".to_owned(),
                ));
            }
        }
        let result = state.clone();
        self.touch();
        Ok(result)
    }

    pub(super) fn tick(&mut self, now: u64) {
        self.finish_restarts(now);
        self.tick_alarm_and_position(now);
        self.tick_recordings(now);
        self.tick_subscriptions(now);
    }

    pub(super) fn due_scenarios(&self, now: u64) -> Vec<ScenarioId> {
        self.scenario_states
            .values()
            .filter(|state| {
                state.status == ScenarioStatus::Running
                    && state.next_step_at.is_some_and(|due| due <= now)
            })
            .map(|state| state.id.clone())
            .collect()
    }

    pub(super) fn take_scenario_step(
        &mut self,
        id: &ScenarioId,
        now: u64,
    ) -> Result<Option<super::types::ScenarioStep>, SimulatorRuntimeError> {
        let definition = self
            .scenarios
            .get(id)
            .cloned()
            .ok_or_else(|| SimulatorRuntimeError::ScenarioNotFound(id.0.clone()))?;
        let state = self
            .scenario_states
            .get_mut(id)
            .ok_or_else(|| SimulatorRuntimeError::ScenarioNotFound(id.0.clone()))?;
        if state.current_step >= definition.steps.len() {
            if definition.repeat {
                state.current_step = 0;
            } else {
                state.status = ScenarioStatus::Completed;
                state.next_step_at = None;
                self.touch();
                return Ok(None);
            }
        }
        let step = definition.steps.get(state.current_step).cloned();
        state.current_step = state.current_step.saturating_add(1);
        state.next_step_at = Some(now);
        Ok(step)
    }

    pub(super) fn delay_scenario(&mut self, id: &ScenarioId, due_at: u64) {
        if let Some(state) = self.scenario_states.get_mut(id) {
            state.next_step_at = Some(due_at);
        }
    }

    pub(super) fn fail_scenario(&mut self, id: &ScenarioId, error: String) {
        if let Some(state) = self.scenario_states.get_mut(id) {
            state.status = ScenarioStatus::Failed;
            state.next_step_at = None;
            state.last_error = Some(error.clone());
        }
        self.push_event(
            "scenarioFailed",
            RuntimeEventLevel::Error,
            None,
            None,
            None,
            error,
        );
    }

    fn build_query_response(
        &self,
        request: &QueryRequest,
    ) -> Result<serde_json::Value, SimulatorRuntimeError> {
        let device = self
            .devices
            .get(&request.device_id)
            .ok_or_else(|| SimulatorRuntimeError::DeviceNotFound(request.device_id.clone()))?;
        let config = self
            .device_configs
            .get(&request.device_id)
            .ok_or_else(|| SimulatorRuntimeError::DeviceNotFound(request.device_id.clone()))?;
        let channel = request
            .channel_id
            .as_deref()
            .map(|id| {
                device
                    .channels
                    .iter()
                    .find(|channel| channel.channel_id == id)
                    .ok_or_else(|| SimulatorRuntimeError::ChannelNotFound(id.to_owned()))
            })
            .transpose()?;
        let response = match request.kind {
            QueryKind::Catalog => json!({
                "deviceId": device.device_id,
                "channels": device.channels.iter().map(|channel| json!({
                    "channelId": channel.channel_id,
                    "name": channel.name,
                    "online": channel.online,
                })).collect::<Vec<_>>()
            }),
            QueryKind::DeviceInfo => json!({
                "deviceId": device.device_id,
                "name": device.name,
                "manufacturer": config.manufacturer,
                "model": config.model,
                "firmwareVersion": config.firmware_version,
                "channelCount": device.channels.len(),
            }),
            QueryKind::DeviceStatus => json!({
                "deviceId": device.device_id,
                "online": device.connectivity == ConnectivityState::Online,
                "connectivity": device.connectivity,
                "guarded": device.guarded,
                "alarmActive": device.channels.iter().any(|item| item.alarm.active),
            }),
            QueryKind::DeviceCapability => json!({
                "deviceId": device.device_id,
                "supports": ["catalog", "deviceInfo", "deviceStatus", "deviceControl", "alarm", "mobilePosition", "ptz", "recordInfo"],
                "videoInputCount": device.channels.len(),
            }),
            QueryKind::DeviceTime => {
                let system = i128::from(now_millis());
                let adjusted = system.saturating_add(i128::from(device.clock_offset_millis));
                json!({
                    "deviceId": device.device_id,
                    "unixMillis": adjusted.max(0),
                    "offsetMillis": device.clock_offset_millis,
                })
            }
            QueryKind::DeviceParameter | QueryKind::ConfigDownload => json!({
                "deviceId": device.device_id,
                "parameters": {
                    "manufacturer": config.manufacturer,
                    "model": config.model,
                    "firmwareVersion": config.firmware_version,
                    "channelCount": config.channel_count,
                }
            }),
            QueryKind::AlarmStatus => json!({
                "deviceId": device.device_id,
                "channelId": channel.map(|item| item.channel_id.clone()),
                "alarms": channel.map_or_else(
                    || device.channels.iter().map(|item| json!({"channelId": item.channel_id, "alarm": item.alarm})).collect::<Vec<_>>(),
                    |item| vec![json!({"channelId": item.channel_id, "alarm": item.alarm})],
                ),
            }),
            QueryKind::MobilePosition => json!({
                "deviceId": device.device_id,
                "channelId": channel.map(|item| item.channel_id.clone()),
                "position": channel.map(|item| &item.position),
            }),
            QueryKind::PresetQuery => json!({
                "deviceId": device.device_id,
                "channelId": channel.map(|item| item.channel_id.clone()),
                "presets": channel.map_or_else(Vec::new, |item| item.ptz.presets.clone()),
            }),
            QueryKind::RecordInfo => {
                let start_time = request
                    .parameters
                    .get("startTime")
                    .and_then(serde_json::Value::as_u64);
                let end_time = request
                    .parameters
                    .get("endTime")
                    .and_then(serde_json::Value::as_u64);
                let record_type = request
                    .parameters
                    .get("recordType")
                    .and_then(serde_json::Value::as_str);
                let entries = self
                    .recordings
                    .iter()
                    .filter(|entry| {
                        entry.device_id == request.device_id
                            && request
                                .channel_id
                                .as_ref()
                                .is_none_or(|id| entry.channel_id == *id)
                            && start_time.is_none_or(|start| entry.ended_at >= start)
                            && end_time.is_none_or(|end| entry.started_at <= end)
                            && record_type.is_none_or(|kind| entry.record_type == kind)
                    })
                    .collect::<Vec<_>>();
                json!({"deviceId": device.device_id, "records": entries})
            }
        };
        Ok(response)
    }

    fn finish_restarts(&mut self, now: u64) {
        let finished = self
            .restart_deadlines
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(device_id, _)| device_id.clone())
            .collect::<Vec<_>>();
        for device_id in finished {
            self.restart_deadlines.remove(&device_id);
            if let Some(device) = self.devices.get_mut(&device_id) {
                device.connectivity = ConnectivityState::Online;
                for channel in &mut device.channels {
                    channel.online = true;
                }
            }
            self.push_event(
                "deviceRestarted",
                RuntimeEventLevel::Info,
                Some(&device_id),
                None,
                None,
                "设备重启完成并恢复在线".to_owned(),
            );
            self.touch();
        }
    }

    fn tick_alarm_and_position(&mut self, now: u64) {
        let mut pending_events = Vec::new();
        for device in self.devices.values_mut() {
            for channel in &mut device.channels {
                if channel
                    .alarm
                    .next_trigger_at
                    .is_some_and(|deadline| deadline <= now)
                {
                    channel.alarm.occurred_at = Some(now);
                    channel.alarm.next_trigger_at = channel
                        .alarm
                        .interval_seconds
                        .map(|seconds| now.saturating_add(u64::from(seconds) * 1_000));
                    pending_events.push((
                        "alarmPeriodic",
                        device.device_id.clone(),
                        channel.channel_id.clone(),
                        "周期报警已触发".to_owned(),
                    ));
                }
                if channel
                    .position
                    .next_report_at
                    .is_some_and(|deadline| deadline <= now)
                {
                    advance_position(&mut channel.position);
                    channel.position.updated_at = Some(now);
                    channel.position.next_report_at = channel
                        .position
                        .interval_seconds
                        .map(|seconds| now.saturating_add(u64::from(seconds) * 1_000));
                    pending_events.push((
                        "positionPeriodic",
                        device.device_id.clone(),
                        channel.channel_id.clone(),
                        "周期位置已更新".to_owned(),
                    ));
                }
            }
        }
        if !pending_events.is_empty() {
            self.touch();
        }
        for (kind, device_id, channel_id, message) in pending_events {
            self.push_event(
                kind,
                RuntimeEventLevel::Info,
                Some(&device_id),
                Some(&channel_id),
                None,
                message,
            );
        }
    }

    fn tick_recordings(&mut self, now: u64) {
        let mut changed = false;
        for device in self.devices.values_mut() {
            for channel in &mut device.channels {
                if channel.recording.status == RecordingRuntimeStatus::Recording {
                    channel.recording.duration_millis = channel
                        .recording
                        .started_at
                        .map_or(0, |started_at| now.saturating_sub(started_at));
                    changed = true;
                }
            }
        }
        if changed {
            self.touch();
        }
    }

    fn tick_subscriptions(&mut self, now: u64) {
        let mut expired = Vec::new();
        for device in self.devices.values_mut() {
            for channel in &mut device.channels {
                for subscription in &mut channel.subscriptions {
                    if subscription.status == "active"
                        && subscription
                            .expires_at
                            .is_some_and(|expires| expires <= now)
                    {
                        "expired".clone_into(&mut subscription.status);
                        subscription.expires_at = None;
                        expired.push((
                            device.device_id.clone(),
                            channel.channel_id.clone(),
                            subscription.kind.clone(),
                        ));
                    }
                }
            }
        }
        if !expired.is_empty() {
            self.touch();
        }
        for (device_id, channel_id, kind) in expired {
            self.push_event(
                "subscriptionExpired",
                RuntimeEventLevel::Warning,
                Some(&device_id),
                Some(&channel_id),
                None,
                format!("{kind} 订阅已过期"),
            );
        }
    }

    fn check_fault(
        &mut self,
        device_id: &str,
        operation_id: &OperationId,
        kind: &str,
        mode: ExecutionMode,
        now: u64,
    ) -> Result<(), SimulatorRuntimeError> {
        if self.fault_profile.force_device_offline {
            self.record_failed_operation(
                operation_id.clone(),
                kind,
                mode,
                device_id,
                now,
                "forced_offline",
                "故障注入强制设备离线",
            );
            return Err(SimulatorRuntimeError::DeviceOffline(device_id.to_owned()));
        }
        if self.fault_profile.force_timeout {
            self.record_failed_operation(
                operation_id.clone(),
                kind,
                mode,
                device_id,
                now,
                "forced_timeout",
                "故障注入强制操作超时",
            );
            return Err(SimulatorRuntimeError::ForcedTimeout);
        }
        if let Some(status) = self.fault_profile.reject_status {
            self.record_failed_operation(
                operation_id.clone(),
                kind,
                mode,
                device_id,
                now,
                "forced_rejection",
                &format!("故障注入强制返回 {status}"),
            );
            return Err(SimulatorRuntimeError::ForcedRejection(status));
        }
        if self.fault_profile.packet_loss_percent > 0
            && (self.next_id % 100) < u64::from(self.fault_profile.packet_loss_percent)
        {
            self.record_failed_operation(
                operation_id.clone(),
                kind,
                mode,
                device_id,
                now,
                "simulated_packet_loss",
                "故障注入模拟消息丢失",
            );
            return Err(SimulatorRuntimeError::SimulatedPacketLoss);
        }
        if mode == ExecutionMode::Platform {
            self.record_failed_operation(
                operation_id.clone(),
                kind,
                mode,
                device_id,
                now,
                "platform_unavailable",
                "平台适配器尚未绑定到本地模拟命令",
            );
            return Err(SimulatorRuntimeError::PlatformAdapterUnavailable);
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "统一操作记录需要完整目标和时间上下文"
    )]
    fn complete_operation(
        &mut self,
        id: OperationId,
        kind: &str,
        mode: ExecutionMode,
        device_id: Option<&str>,
        channel_id: Option<&str>,
        started_at: u64,
    ) -> OperationRecord {
        let completed_at = now_millis();
        let record = OperationRecord {
            id,
            kind: kind.to_owned(),
            mode,
            target: OperationTarget {
                device_id: device_id.map(str::to_owned),
                channel_id: channel_id.map(str::to_owned),
            },
            status: OperationStatus::Succeeded,
            started_at,
            completed_at: Some(completed_at),
            duration_millis: Some(completed_at.saturating_sub(started_at)),
            error_code: None,
            error_message: None,
            transaction_id: None,
        };
        push_bounded(&mut self.operations, record.clone(), MAX_OPERATIONS);
        self.touch();
        record
    }

    #[expect(clippy::too_many_arguments, reason = "统一失败记录需要完整操作上下文")]
    fn record_failed_operation(
        &mut self,
        id: OperationId,
        kind: &str,
        mode: ExecutionMode,
        device_id: &str,
        started_at: u64,
        error_code: &str,
        error_message: &str,
    ) {
        let completed_at = now_millis();
        push_bounded(
            &mut self.operations,
            OperationRecord {
                id,
                kind: kind.to_owned(),
                mode,
                target: OperationTarget {
                    device_id: Some(device_id.to_owned()),
                    channel_id: None,
                },
                status: if error_code == "forced_timeout" {
                    OperationStatus::Timeout
                } else {
                    OperationStatus::Failed
                },
                started_at,
                completed_at: Some(completed_at),
                duration_millis: Some(completed_at.saturating_sub(started_at)),
                error_code: Some(error_code.to_owned()),
                error_message: Some(error_message.to_owned()),
                transaction_id: None,
            },
            MAX_OPERATIONS,
        );
        self.push_event(
            "operationFailed",
            RuntimeEventLevel::Error,
            Some(device_id),
            None,
            None,
            error_message.to_owned(),
        );
        self.touch();
    }

    #[expect(clippy::too_many_arguments, reason = "事件记录需要完整关联上下文")]
    fn push_event(
        &mut self,
        kind: &str,
        level: RuntimeEventLevel,
        device_id: Option<&str>,
        channel_id: Option<&str>,
        operation_id: Option<OperationId>,
        message: String,
    ) {
        let id = self.allocate_id();
        push_bounded(
            &mut self.events,
            RuntimeEventRecord {
                id,
                timestamp: now_millis(),
                kind: kind.to_owned(),
                level,
                device_id: device_id.map(str::to_owned),
                channel_id: channel_id.map(str::to_owned),
                operation_id,
                message,
            },
            MAX_EVENTS,
        );
    }

    fn next_operation_id(&mut self) -> OperationId {
        OperationId(format!("operation-{}", self.allocate_id()))
    }

    const fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    fn device_mut(
        &mut self,
        device_id: &str,
    ) -> Result<&mut DeviceRuntimeState, SimulatorRuntimeError> {
        self.devices
            .get_mut(device_id)
            .ok_or_else(|| SimulatorRuntimeError::DeviceNotFound(device_id.to_owned()))
    }

    fn channel_mut(
        &mut self,
        device_id: &str,
        channel_id: &str,
    ) -> Result<&mut ChannelRuntimeState, SimulatorRuntimeError> {
        self.device_mut(device_id)?
            .channels
            .iter_mut()
            .find(|channel| channel.channel_id == channel_id)
            .ok_or_else(|| SimulatorRuntimeError::ChannelNotFound(channel_id.to_owned()))
    }

    const fn touch(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn apply_ptz_command(
    channel: &mut ChannelRuntimeState,
    command: PtzCommand,
    now: u64,
) -> Result<(), SimulatorRuntimeError> {
    match command {
        PtzCommand::Move { motion, speed } => {
            if speed == 0 {
                return Err(SimulatorRuntimeError::InvalidInput(
                    "PTZ 速度必须大于 0".to_owned(),
                ));
            }
            channel.ptz.motion = motion;
            channel.ptz.speed = speed;
            let delta = i16::from(speed).max(1);
            match motion {
                PtzMotion::Up => channel.ptz.tilt = channel.ptz.tilt.saturating_add(delta),
                PtzMotion::Down => channel.ptz.tilt = channel.ptz.tilt.saturating_sub(delta),
                PtzMotion::Left => channel.ptz.pan = channel.ptz.pan.saturating_sub(delta),
                PtzMotion::Right => channel.ptz.pan = channel.ptz.pan.saturating_add(delta),
                PtzMotion::ZoomIn => {
                    channel.ptz.zoom = channel.ptz.zoom.saturating_add(u16::from(speed));
                }
                PtzMotion::ZoomOut => {
                    channel.ptz.zoom = channel.ptz.zoom.saturating_sub(u16::from(speed));
                }
                PtzMotion::FocusNear => {
                    channel.ptz.focus = channel.ptz.focus.saturating_add(u16::from(speed));
                }
                PtzMotion::FocusFar => {
                    channel.ptz.focus = channel.ptz.focus.saturating_sub(u16::from(speed));
                }
                PtzMotion::IrisOpen => {
                    channel.ptz.iris = channel.ptz.iris.saturating_add(u16::from(speed));
                }
                PtzMotion::IrisClose => {
                    channel.ptz.iris = channel.ptz.iris.saturating_sub(u16::from(speed));
                }
                PtzMotion::Stop => channel.ptz.speed = 0,
            }
            channel.ptz.active_preset = None;
        }
        PtzCommand::Stop => {
            channel.ptz.motion = PtzMotion::Stop;
            channel.ptz.speed = 0;
        }
        PtzCommand::SetPreset { id, name } => {
            if id == 0 || name.trim().is_empty() {
                return Err(SimulatorRuntimeError::InvalidInput(
                    "预置位编号和名称无效".to_owned(),
                ));
            }
            let preset = PtzPreset {
                id,
                name: name.trim().to_owned(),
                pan: channel.ptz.pan,
                tilt: channel.ptz.tilt,
                zoom: channel.ptz.zoom,
            };
            if let Some(existing) = channel.ptz.presets.iter_mut().find(|item| item.id == id) {
                *existing = preset;
            } else {
                channel.ptz.presets.push(preset);
                channel.ptz.presets.sort_by_key(|item| item.id);
            }
        }
        PtzCommand::CallPreset { id } => {
            let preset = channel
                .ptz
                .presets
                .iter()
                .find(|item| item.id == id)
                .cloned()
                .ok_or(SimulatorRuntimeError::PresetNotFound(id))?;
            channel.ptz.pan = preset.pan;
            channel.ptz.tilt = preset.tilt;
            channel.ptz.zoom = preset.zoom;
            channel.ptz.active_preset = Some(id);
            channel.ptz.motion = PtzMotion::Stop;
            channel.ptz.speed = 0;
        }
        PtzCommand::DeletePreset { id } => {
            let original = channel.ptz.presets.len();
            channel.ptz.presets.retain(|item| item.id != id);
            if channel.ptz.presets.len() == original {
                return Err(SimulatorRuntimeError::PresetNotFound(id));
            }
            if channel.ptz.active_preset == Some(id) {
                channel.ptz.active_preset = None;
            }
        }
    }
    channel.ptz.updated_at = Some(now);
    Ok(())
}

fn validate_alarm(command: &AlarmCommand) -> Result<(), SimulatorRuntimeError> {
    if !matches!(command.priority.as_str(), "1" | "2" | "3" | "4")
        || !matches!(
            command.method.as_str(),
            "1" | "2" | "3" | "4" | "5" | "6" | "7"
        )
    {
        return Err(SimulatorRuntimeError::InvalidInput(
            "报警级别或报警方式不合法".to_owned(),
        ));
    }
    if command
        .interval_seconds
        .is_some_and(|seconds| !(1..=86_400).contains(&seconds))
    {
        return Err(SimulatorRuntimeError::InvalidInput(
            "报警周期必须位于 1 至 86400 秒".to_owned(),
        ));
    }
    Ok(())
}

fn validate_position(command: &PositionCommand) -> Result<(), SimulatorRuntimeError> {
    if !command.longitude.is_finite()
        || !(-180.0..=180.0).contains(&command.longitude)
        || !command.latitude.is_finite()
        || !(-90.0..=90.0).contains(&command.latitude)
        || !command.speed.is_finite()
        || command.speed < 0.0
        || !command.direction.is_finite()
        || !(0.0..=360.0).contains(&command.direction)
        || !command.altitude.is_finite()
    {
        return Err(SimulatorRuntimeError::InvalidInput(
            "移动位置参数超出允许范围".to_owned(),
        ));
    }
    if command.running && command.interval_seconds.is_none() {
        return Err(SimulatorRuntimeError::InvalidInput(
            "周期位置模拟必须设置间隔".to_owned(),
        ));
    }
    Ok(())
}

const fn restore_alarm(channel: &mut ChannelRuntimeState, now: u64) {
    channel.alarm.active = false;
    channel.alarm.restored_at = Some(now);
    channel.alarm.interval_seconds = None;
    channel.alarm.next_trigger_at = None;
}

#[expect(clippy::cast_precision_loss, reason = "地理模拟使用 f64 近似推进")]
fn advance_position(position: &mut super::types::PositionRuntimeState) {
    match position.mode {
        PositionSimulationMode::Fixed => {}
        PositionSimulationMode::Route => {
            let radians = position.direction.to_radians();
            let distance = position.speed.max(1.0) / 3_600.0;
            position.longitude =
                (position.longitude + radians.sin() * distance / 111.0).clamp(-180.0, 180.0);
            position.latitude =
                (position.latitude + radians.cos() * distance / 111.0).clamp(-90.0, 90.0);
        }
        PositionSimulationMode::RandomWalk => {
            let seed = position.updated_at.unwrap_or_default() % 17;
            let delta = (seed as f64 - 8.0) * 0.000_001;
            position.longitude = (position.longitude + delta).clamp(-180.0, 180.0);
            position.latitude = (position.latitude - delta).clamp(-90.0, 90.0);
        }
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, value: T, maximum: usize) {
    if queue.len() == maximum {
        queue.pop_front();
    }
    queue.push_back(value);
}

#[cfg(test)]
mod tests {
    use crate::domain::{DeviceId, DeviceKind};

    use super::*;

    fn device() -> Option<SimulatedDevice> {
        Some(SimulatedDevice {
            id: DeviceId::new("34020000001320000100").ok()?,
            name: "模拟球机".to_owned(),
            kind: DeviceKind::PtzCamera,
            manufacturer: "GBLab".to_owned(),
            model: "SIM-PTZ".to_owned(),
            firmware_version: "1.0.0".to_owned(),
            channel_count: 1,
            created_at: 1,
        })
    }

    fn state() -> Option<SimulatorState> {
        Some(SimulatorState::new(vec![device()?]))
    }

    fn channel_id(state: &SimulatorState) -> String {
        state.snapshot().devices[0].channels[0].channel_id.clone()
    }

    #[test]
    fn device_restart_should_transition_back_online_on_shared_tick() {
        let Some(mut state) = state() else { return };
        let start = now_millis();

        let result = state.control_device(
            "34020000001320000100",
            DeviceControlCommand::Restart {
                duration_seconds: 1,
            },
            ExecutionMode::LocalSimulation,
        );

        assert!(result.is_ok());
        assert_eq!(
            state.snapshot().devices[0].connectivity,
            ConnectivityState::Restarting
        );
        state.tick(start.saturating_add(1_100));
        assert_eq!(
            state.snapshot().devices[0].connectivity,
            ConnectivityState::Online
        );
    }

    #[test]
    fn ptz_preset_should_capture_and_restore_position() {
        let Some(mut state) = state() else { return };
        let channel_id = channel_id(&state);
        let device_id = "34020000001320000100";

        let moved = state.control_ptz(
            device_id,
            &channel_id,
            PtzCommand::Move {
                motion: PtzMotion::Right,
                speed: 8,
            },
            ExecutionMode::LocalSimulation,
        );
        assert!(moved.is_ok());
        assert!(
            state
                .control_ptz(
                    device_id,
                    &channel_id,
                    PtzCommand::SetPreset {
                        id: 1,
                        name: "入口".to_owned(),
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert!(
            state
                .control_ptz(
                    device_id,
                    &channel_id,
                    PtzCommand::Move {
                        motion: PtzMotion::Left,
                        speed: 3,
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert!(
            state
                .control_ptz(
                    device_id,
                    &channel_id,
                    PtzCommand::CallPreset { id: 1 },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );

        let ptz = &state.snapshot().devices[0].channels[0].ptz;
        assert_eq!(ptz.pan, 8);
        assert_eq!(ptz.active_preset, Some(1));
    }

    #[test]
    fn alarm_and_position_should_advance_without_per_device_tasks() {
        let Some(mut state) = state() else { return };
        let channel_id = channel_id(&state);
        let now = now_millis();

        assert!(
            state
                .update_alarm(
                    "34020000001320000100",
                    &channel_id,
                    AlarmCommand {
                        active: true,
                        priority: "1".to_owned(),
                        method: "2".to_owned(),
                        alarm_type: None,
                        description: "周期报警".to_owned(),
                        interval_seconds: Some(1),
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert!(
            state
                .update_position(
                    "34020000001320000100",
                    &channel_id,
                    PositionCommand {
                        longitude: 116.397,
                        latitude: 39.908,
                        speed: 36.0,
                        direction: 90.0,
                        altitude: 10.0,
                        mode: PositionSimulationMode::Route,
                        running: true,
                        interval_seconds: Some(1),
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        state.tick(now.saturating_add(1_100));

        let channel = &state.snapshot().devices[0].channels[0];
        assert!(channel.alarm.active);
        assert!(channel.position.longitude > 116.397);
    }

    #[test]
    fn unified_query_should_return_runtime_backed_state() {
        let Some(mut state) = state() else { return };
        let result = state.execute_query(QueryRequest {
            device_id: "34020000001320000100".to_owned(),
            channel_id: None,
            kind: QueryKind::DeviceInfo,
            parameters: BTreeMap::new(),
            mode: ExecutionMode::LocalSimulation,
        });

        assert!(result.is_ok());
        let response = result.ok().and_then(|result| result.response);
        assert_eq!(
            response.and_then(|value| value.get("manufacturer").cloned()),
            Some(json!("GBLab"))
        );
    }

    #[test]
    fn recording_lifecycle_should_create_record_info_entry() {
        let Some(mut state) = state() else { return };
        let channel_id = channel_id(&state);
        let device_id = "34020000001320000100";

        assert!(
            state
                .control_recording(
                    device_id,
                    &channel_id,
                    RecordingCommand::Start {
                        name: "门口录像".to_owned(),
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert_eq!(
            state.snapshot().devices[0].channels[0].recording.status,
            RecordingRuntimeStatus::Recording
        );
        assert!(
            state
                .control_recording(
                    device_id,
                    &channel_id,
                    RecordingCommand::Pause,
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert!(
            state
                .control_recording(
                    device_id,
                    &channel_id,
                    RecordingCommand::Resume,
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert!(
            state
                .control_recording(
                    device_id,
                    &channel_id,
                    RecordingCommand::Stop,
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert_eq!(state.recordings().len(), 1);
        assert_eq!(state.recordings()[0].name, "门口录像");
    }

    #[test]
    fn subscription_lifecycle_should_expire_on_shared_tick() {
        let Some(mut state) = state() else { return };
        let channel_id = channel_id(&state);
        let now = now_millis();

        assert!(
            state
                .control_subscription(
                    "34020000001320000100",
                    &channel_id,
                    SubscriptionCommand::Upsert {
                        subscription_kind: "Alarm".to_owned(),
                        expires_seconds: 1,
                    },
                    ExecutionMode::LocalSimulation,
                )
                .is_ok()
        );
        assert_eq!(
            state.snapshot().devices[0].channels[0].subscriptions[0].status,
            "active"
        );
        state.tick(now.saturating_add(1_100));
        assert_eq!(
            state.snapshot().devices[0].channels[0].subscriptions[0].status,
            "expired"
        );
    }

    #[test]
    fn fault_profile_should_make_failure_observable() {
        let Some(mut state) = state() else { return };
        assert!(
            state
                .set_fault_profile(FaultProfile {
                    force_timeout: true,
                    ..FaultProfile::default()
                })
                .is_ok()
        );

        let result = state.control_device(
            "34020000001320000100",
            DeviceControlCommand::Guard,
            ExecutionMode::LocalSimulation,
        );

        assert_eq!(result, Err(SimulatorRuntimeError::ForcedTimeout));
        assert_eq!(state.operations()[0].status, OperationStatus::Timeout);
        assert_eq!(state.events()[0].level, RuntimeEventLevel::Error);
    }
}
