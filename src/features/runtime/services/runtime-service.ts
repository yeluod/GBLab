import { invokeCommand } from '@/infrastructure/tauri';

import type {
  AlarmCommand,
  DeviceControlCommand,
  ExecutionMode,
  FaultProfile,
  OperationRecord,
  PositionCommand,
  PtzCommand,
  QueryRequest,
  QueryResult,
  RecordingCommand,
  RecordingEntry,
  RuntimeEventRecord,
  ScenarioDefinition,
  ScenarioRuntimeState,
  ScenarioStatus,
  SimulatorRuntimeSnapshot,
  SubscriptionCommand,
  TransactionRecord,
} from '../types/runtime-types';

export const runtimeService = {
  snapshot: () => invokeCommand<SimulatorRuntimeSnapshot>('get_simulator_runtime_snapshot'),
  operations: () => invokeCommand<OperationRecord[]>('get_simulator_operations'),
  events: () => invokeCommand<RuntimeEventRecord[]>('get_simulator_events'),
  queries: () => invokeCommand<QueryResult[]>('get_simulator_queries'),
  transactions: () => invokeCommand<TransactionRecord[]>('get_simulator_transactions'),
  recordings: () => invokeCommand<RecordingEntry[]>('get_simulator_recordings'),
  scenarios: () => invokeCommand<ScenarioRuntimeState[]>('get_simulator_scenarios'),
  setFaultProfile: (profile: FaultProfile) =>
    invokeCommand<void>('set_simulator_fault_profile', { profile }),
  controlDevice: (
    deviceId: string,
    command: DeviceControlCommand,
    mode: ExecutionMode = 'localSimulation',
  ) => invokeCommand<OperationRecord>('simulate_device_control', { deviceId, command, mode }),
  controlPtz: (
    deviceId: string,
    channelId: string,
    command: PtzCommand,
    mode: ExecutionMode = 'localSimulation',
  ) =>
    invokeCommand<OperationRecord>('simulate_ptz_control', {
      deviceId,
      channelId,
      command,
      mode,
    }),
  updateAlarm: (
    deviceId: string,
    channelId: string,
    command: AlarmCommand,
    mode: ExecutionMode = 'localSimulation',
  ) => invokeCommand<OperationRecord>('simulate_alarm', { deviceId, channelId, command, mode }),
  updatePosition: (
    deviceId: string,
    channelId: string,
    command: PositionCommand,
    mode: ExecutionMode = 'localSimulation',
  ) => invokeCommand<OperationRecord>('simulate_position', { deviceId, channelId, command, mode }),
  controlRecording: (
    deviceId: string,
    channelId: string,
    command: RecordingCommand,
    mode: ExecutionMode = 'localSimulation',
  ) =>
    invokeCommand<OperationRecord>('simulate_recording', {
      deviceId,
      channelId,
      command,
      mode,
    }),
  controlSubscription: (
    deviceId: string,
    channelId: string,
    command: SubscriptionCommand,
    mode: ExecutionMode = 'localSimulation',
  ) =>
    invokeCommand<OperationRecord>('simulate_subscription', {
      deviceId,
      channelId,
      command,
      mode,
    }),
  executeQuery: (request: QueryRequest) =>
    invokeCommand<QueryResult>('execute_simulator_query', { request }),
  saveScenario: (definition: ScenarioDefinition) =>
    invokeCommand<ScenarioRuntimeState>('save_simulator_scenario', { definition }),
  startScenario: (id: string) =>
    invokeCommand<ScenarioRuntimeState>('start_simulator_scenario', { id }),
  setScenarioStatus: (id: string, status: ScenarioStatus) =>
    invokeCommand<ScenarioRuntimeState>('set_simulator_scenario_status', { id, status }),
};
