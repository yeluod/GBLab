export type ExecutionMode = 'localSimulation' | 'platform';
export type OperationStatus =
  'pending' | 'running' | 'succeeded' | 'failed' | 'timeout' | 'cancelled';
export type ConnectivityState = 'online' | 'offline' | 'restarting';
export type RuntimeEventLevel = 'info' | 'warning' | 'error';
export type ScenarioStatus = 'idle' | 'running' | 'paused' | 'completed' | 'stopped' | 'failed';
export type PositionSimulationMode = 'fixed' | 'route' | 'randomWalk';
export type PtzMotion =
  | 'stop'
  | 'up'
  | 'down'
  | 'left'
  | 'right'
  | 'zoomIn'
  | 'zoomOut'
  | 'focusNear'
  | 'focusFar'
  | 'irisOpen'
  | 'irisClose';

export interface OperationRecord {
  id: string;
  kind: string;
  mode: ExecutionMode;
  target: { deviceId: string | null; channelId: string | null };
  status: OperationStatus;
  startedAt: number;
  completedAt: number | null;
  durationMillis: number | null;
  errorCode: string | null;
  errorMessage: string | null;
  transactionId: string | null;
}

export interface AlarmRuntimeState {
  active: boolean;
  priority: string | null;
  method: string | null;
  alarmType: string | null;
  description: string | null;
  occurredAt: number | null;
  restoredAt: number | null;
  intervalSeconds: number | null;
  nextTriggerAt: number | null;
}

export interface PositionRuntimeState {
  longitude: number;
  latitude: number;
  speed: number;
  direction: number;
  altitude: number;
  updatedAt: number | null;
  mode: PositionSimulationMode;
  running: boolean;
  intervalSeconds: number | null;
  nextReportAt: number | null;
}

export interface PtzPreset {
  id: number;
  name: string;
  pan: number;
  tilt: number;
  zoom: number;
}

export interface PtzRuntimeState {
  motion: PtzMotion;
  speed: number;
  pan: number;
  tilt: number;
  zoom: number;
  focus: number;
  iris: number;
  activePreset: number | null;
  presets: PtzPreset[];
  updatedAt: number | null;
}

export interface RecordingRuntimeState {
  status: 'idle' | 'recording' | 'paused' | 'failed';
  currentFile: string | null;
  startedAt: number | null;
  durationMillis: number;
  lastError: string | null;
}

export interface ChannelRuntimeState {
  channelId: string;
  name: string;
  online: boolean;
  alarm: AlarmRuntimeState;
  position: PositionRuntimeState;
  ptz: PtzRuntimeState;
  recording: RecordingRuntimeState;
  subscriptions: Array<{
    kind: string;
    status: string;
    expiresAt: number | null;
    lastNotifiedAt: number | null;
    lastError: string | null;
  }>;
  lastOperationId: string | null;
}

export interface DeviceRuntimeState {
  deviceId: string;
  name: string;
  connectivity: ConnectivityState;
  guarded: boolean;
  clockOffsetMillis: number;
  lastPlatformRequestAt: number | null;
  lastOperationId: string | null;
  channels: ChannelRuntimeState[];
}

export interface FaultProfile {
  delayMillis: number;
  forceTimeout: boolean;
  packetLossPercent: number;
  rejectStatus: number | null;
  forceDeviceOffline: boolean;
}

export interface SimulatorRuntimeSnapshot {
  revision: number;
  devices: DeviceRuntimeState[];
  activeScenarios: number;
  faultProfile: FaultProfile;
}

export type DeviceControlCommand =
  | { kind: 'restart'; durationSeconds: number }
  | { kind: 'guard' }
  | { kind: 'unguard' }
  | { kind: 'alarmReset' }
  | { kind: 'setTime'; offsetMillis: number }
  | { kind: 'setOnline' }
  | { kind: 'setOffline' };

export type PtzCommand =
  | { kind: 'move'; motion: PtzMotion; speed: number }
  | { kind: 'stop' }
  | { kind: 'setPreset'; id: number; name: string }
  | { kind: 'callPreset'; id: number }
  | { kind: 'deletePreset'; id: number };

export interface AlarmCommand {
  active: boolean;
  priority: string;
  method: string;
  alarmType: string | null;
  description: string;
  intervalSeconds: number | null;
}

export interface PositionCommand {
  longitude: number;
  latitude: number;
  speed: number;
  direction: number;
  altitude: number;
  mode: PositionSimulationMode;
  running: boolean;
  intervalSeconds: number | null;
}

export type RecordingCommand =
  { kind: 'start'; name: string } | { kind: 'pause' } | { kind: 'resume' } | { kind: 'stop' };

export type SubscriptionCommand =
  | { kind: 'upsert'; subscriptionKind: string; expiresSeconds: number }
  | { kind: 'cancel'; subscriptionKind: string }
  | { kind: 'fail'; subscriptionKind: string; error: string };

export type QueryKind =
  | 'catalog'
  | 'deviceInfo'
  | 'deviceStatus'
  | 'deviceCapability'
  | 'deviceTime'
  | 'deviceParameter'
  | 'configDownload'
  | 'alarmStatus'
  | 'mobilePosition'
  | 'presetQuery'
  | 'recordInfo';

export interface QueryRequest {
  deviceId: string;
  channelId: string | null;
  kind: QueryKind;
  parameters: Record<string, unknown>;
  mode: ExecutionMode;
}

export interface QueryResult {
  id: string;
  request: QueryRequest;
  status: OperationStatus;
  response: unknown | null;
  error: string | null;
  startedAt: number;
  completedAt: number;
  durationMillis: number;
  operationId: string;
}

export interface RuntimeEventRecord {
  id: number;
  timestamp: number;
  kind: string;
  level: RuntimeEventLevel;
  deviceId: string | null;
  channelId: string | null;
  operationId: string | null;
  message: string;
}

export interface TransactionRecord {
  id: string;
  callId: string;
  cseq: number;
  method: string;
  viaBranch: string;
  status: string;
  responseStatus: number | null;
  error: string | null;
  startedAt: number;
  completedAt: number | null;
}

export interface RecordingEntry {
  id: string;
  deviceId: string;
  channelId: string;
  name: string;
  startedAt: number;
  endedAt: number;
  recordType: string;
  sizeBytes: number;
  filePath: string | null;
}

export type ScenarioAction =
  | { kind: 'delay'; durationMillis: number }
  | { kind: 'deviceControl'; command: DeviceControlCommand }
  | { kind: 'ptz'; command: PtzCommand }
  | { kind: 'alarm'; command: AlarmCommand }
  | { kind: 'position'; command: PositionCommand }
  | { kind: 'recording'; command: RecordingCommand }
  | { kind: 'subscription'; command: SubscriptionCommand }
  | { kind: 'query'; request: QueryRequest };

export interface ScenarioStep {
  name: string;
  deviceId: string;
  channelId: string | null;
  action: ScenarioAction;
}

export interface ScenarioDefinition {
  id: string | null;
  name: string;
  steps: ScenarioStep[];
  repeat: boolean;
}

export interface ScenarioRuntimeState {
  id: string;
  name: string;
  status: ScenarioStatus;
  currentStep: number;
  totalSteps: number;
  nextStepAt: number | null;
  lastError: string | null;
}
