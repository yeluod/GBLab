//! 本地模拟器单所有者 Actor 与集中式时间驱动。

use std::collections::VecDeque;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use crate::SimulatedDevice;

use super::types::{
    AlarmCommand, DeviceControlCommand, ExecutionMode, FaultProfile, OperationRecord,
    PositionCommand, PtzCommand, QueryRequest, QueryResult, RecordingCommand, RecordingEntry,
    RuntimeEventRecord, ScenarioAction, ScenarioDefinition, ScenarioId, ScenarioRuntimeState,
    ScenarioStatus, SimulatorRuntimeSnapshot, SubscriptionCommand, TransactionRecord,
};
use super::{SimulatorRuntimeError, state::SimulatorState};
use crate::runtime::time::now_millis;

pub(super) const COMMAND_CAPACITY: usize = 256;
const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub(super) enum SimulatorCommand {
    SyncDevices {
        devices: Vec<SimulatedDevice>,
        reply: oneshot::Sender<()>,
    },
    GetOperations {
        reply: oneshot::Sender<Vec<OperationRecord>>,
    },
    GetEvents {
        reply: oneshot::Sender<Vec<RuntimeEventRecord>>,
    },
    GetQueries {
        reply: oneshot::Sender<Vec<QueryResult>>,
    },
    GetTransactions {
        reply: oneshot::Sender<Vec<TransactionRecord>>,
    },
    GetRecordings {
        reply: oneshot::Sender<Vec<RecordingEntry>>,
    },
    GetScenarios {
        reply: oneshot::Sender<Vec<ScenarioRuntimeState>>,
    },
    SetFaultProfile {
        profile: FaultProfile,
        reply: oneshot::Sender<Result<(), SimulatorRuntimeError>>,
    },
    DeviceControl {
        device_id: String,
        command: DeviceControlCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    PtzControl {
        device_id: String,
        channel_id: String,
        command: PtzCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    Alarm {
        device_id: String,
        channel_id: String,
        command: AlarmCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    Position {
        device_id: String,
        channel_id: String,
        command: PositionCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    Recording {
        device_id: String,
        channel_id: String,
        command: RecordingCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    Subscription {
        device_id: String,
        channel_id: String,
        command: SubscriptionCommand,
        mode: ExecutionMode,
        reply: oneshot::Sender<Result<OperationRecord, SimulatorRuntimeError>>,
    },
    Query {
        request: QueryRequest,
        reply: oneshot::Sender<Result<QueryResult, SimulatorRuntimeError>>,
    },
    SaveScenario {
        definition: ScenarioDefinition,
        reply: oneshot::Sender<Result<ScenarioRuntimeState, SimulatorRuntimeError>>,
    },
    StartScenario {
        id: ScenarioId,
        reply: oneshot::Sender<Result<ScenarioRuntimeState, SimulatorRuntimeError>>,
    },
    SetScenarioStatus {
        id: ScenarioId,
        status: ScenarioStatus,
        reply: oneshot::Sender<Result<ScenarioRuntimeState, SimulatorRuntimeError>>,
    },
}

pub(super) async fn run(
    devices: Vec<SimulatedDevice>,
    mut command_rx: mpsc::Receiver<SimulatorCommand>,
    snapshot_tx: watch::Sender<SimulatorRuntimeSnapshot>,
    cancellation: CancellationToken,
) {
    let mut state = SimulatorState::new(devices);
    let mut delayed_commands = VecDeque::<(u64, SimulatorCommand)>::new();
    let mut last_revision = state.snapshot().revision;
    let _ = snapshot_tx.send(state.snapshot());
    let mut ticker = interval(TICK_INTERVAL);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            command = command_rx.recv() => {
                let Some(command) = command else { break };
                let delay = state.command_delay_millis();
                if delay > 0 && is_delayable(&command) {
                    delayed_commands.push_back((now_millis().saturating_add(delay), command));
                } else {
                    handle_command(&mut state, command);
                }
            }
            _ = ticker.tick() => {
                let now = now_millis();
                while delayed_commands.front().is_some_and(|(due, _)| *due <= now) {
                    if let Some((_, command)) = delayed_commands.pop_front() {
                        handle_command(&mut state, command);
                    }
                }
                state.tick(now);
                run_due_scenarios(&mut state, now);
            }
        }
        let snapshot = state.snapshot();
        if snapshot.revision != last_revision {
            last_revision = snapshot.revision;
            let _ = snapshot_tx.send(snapshot);
        }
    }
}

const fn is_delayable(command: &SimulatorCommand) -> bool {
    matches!(
        command,
        SimulatorCommand::DeviceControl { .. }
            | SimulatorCommand::PtzControl { .. }
            | SimulatorCommand::Alarm { .. }
            | SimulatorCommand::Position { .. }
            | SimulatorCommand::Recording { .. }
            | SimulatorCommand::Subscription { .. }
            | SimulatorCommand::Query { .. }
    )
}

fn handle_command(state: &mut SimulatorState, command: SimulatorCommand) {
    match command {
        SimulatorCommand::SyncDevices { devices, reply } => {
            state.sync_devices(devices);
            let _ = reply.send(());
        }
        SimulatorCommand::GetOperations { reply } => {
            let _ = reply.send(state.operations());
        }
        SimulatorCommand::GetEvents { reply } => {
            let _ = reply.send(state.events());
        }
        SimulatorCommand::GetQueries { reply } => {
            let _ = reply.send(state.queries());
        }
        SimulatorCommand::GetTransactions { reply } => {
            let _ = reply.send(state.transactions());
        }
        SimulatorCommand::GetRecordings { reply } => {
            let _ = reply.send(state.recordings());
        }
        SimulatorCommand::GetScenarios { reply } => {
            let _ = reply.send(state.scenarios());
        }
        SimulatorCommand::SetFaultProfile { profile, reply } => {
            let _ = reply.send(state.set_fault_profile(profile));
        }
        SimulatorCommand::DeviceControl {
            device_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.control_device(&device_id, command, mode));
        }
        SimulatorCommand::PtzControl {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.control_ptz(&device_id, &channel_id, command, mode));
        }
        SimulatorCommand::Alarm {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.update_alarm(&device_id, &channel_id, command, mode));
        }
        SimulatorCommand::Position {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.update_position(&device_id, &channel_id, command, mode));
        }
        SimulatorCommand::Recording {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.control_recording(&device_id, &channel_id, command, mode));
        }
        SimulatorCommand::Subscription {
            device_id,
            channel_id,
            command,
            mode,
            reply,
        } => {
            let _ = reply.send(state.control_subscription(&device_id, &channel_id, command, mode));
        }
        SimulatorCommand::Query { request, reply } => {
            let _ = reply.send(state.execute_query(request));
        }
        SimulatorCommand::SaveScenario { definition, reply } => {
            let _ = reply.send(state.save_scenario(definition));
        }
        SimulatorCommand::StartScenario { id, reply } => {
            let _ = reply.send(state.start_scenario(&id));
        }
        SimulatorCommand::SetScenarioStatus { id, status, reply } => {
            let _ = reply.send(state.set_scenario_status(&id, status));
        }
    }
}

fn run_due_scenarios(state: &mut SimulatorState, now: u64) {
    for scenario_id in state.due_scenarios(now) {
        let step = match state.take_scenario_step(&scenario_id, now) {
            Ok(Some(step)) => step,
            Ok(None) => continue,
            Err(error) => {
                state.fail_scenario(&scenario_id, error.to_string());
                continue;
            }
        };
        let result = match step.action {
            ScenarioAction::Delay { duration_millis } => {
                state.delay_scenario(&scenario_id, now.saturating_add(duration_millis));
                Ok(())
            }
            ScenarioAction::DeviceControl { command } => state
                .control_device(&step.device_id, command, ExecutionMode::LocalSimulation)
                .map(|_| ()),
            ScenarioAction::Ptz { command } => step.channel_id.as_deref().map_or_else(
                || {
                    Err(SimulatorRuntimeError::InvalidInput(
                        "PTZ 场景步骤必须指定通道".to_owned(),
                    ))
                },
                |channel_id| {
                    state
                        .control_ptz(
                            &step.device_id,
                            channel_id,
                            command,
                            ExecutionMode::LocalSimulation,
                        )
                        .map(|_| ())
                },
            ),
            ScenarioAction::Alarm { command } => step.channel_id.as_deref().map_or_else(
                || {
                    Err(SimulatorRuntimeError::InvalidInput(
                        "报警场景步骤必须指定通道".to_owned(),
                    ))
                },
                |channel_id| {
                    state
                        .update_alarm(
                            &step.device_id,
                            channel_id,
                            command,
                            ExecutionMode::LocalSimulation,
                        )
                        .map(|_| ())
                },
            ),
            ScenarioAction::Position { command } => step.channel_id.as_deref().map_or_else(
                || {
                    Err(SimulatorRuntimeError::InvalidInput(
                        "位置场景步骤必须指定通道".to_owned(),
                    ))
                },
                |channel_id| {
                    state
                        .update_position(
                            &step.device_id,
                            channel_id,
                            command,
                            ExecutionMode::LocalSimulation,
                        )
                        .map(|_| ())
                },
            ),
            ScenarioAction::Recording { command } => step.channel_id.as_deref().map_or_else(
                || {
                    Err(SimulatorRuntimeError::InvalidInput(
                        "录像场景步骤必须指定通道".to_owned(),
                    ))
                },
                |channel_id| {
                    state
                        .control_recording(
                            &step.device_id,
                            channel_id,
                            command,
                            ExecutionMode::LocalSimulation,
                        )
                        .map(|_| ())
                },
            ),
            ScenarioAction::Subscription { command } => step.channel_id.as_deref().map_or_else(
                || {
                    Err(SimulatorRuntimeError::InvalidInput(
                        "订阅场景步骤必须指定通道".to_owned(),
                    ))
                },
                |channel_id| {
                    state
                        .control_subscription(
                            &step.device_id,
                            channel_id,
                            command,
                            ExecutionMode::LocalSimulation,
                        )
                        .map(|_| ())
                },
            ),
            ScenarioAction::Query { request } => state.execute_query(request).map(|_| ()),
        };
        if let Err(error) = result {
            state.fail_scenario(&scenario_id, error.to_string());
        } else {
            state.delay_scenario(&scenario_id, now);
        }
    }
}
