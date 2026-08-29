import { computed, ref } from 'vue';
import { defineStore } from 'pinia';

import { runtimeService } from '../services/runtime-service';
import type {
  AlarmCommand,
  DeviceControlCommand,
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

const emptyFaultProfile: FaultProfile = {
  delayMillis: 0,
  forceTimeout: false,
  packetLossPercent: 0,
  rejectStatus: null,
  forceDeviceOffline: false,
};

const emptySnapshot: SimulatorRuntimeSnapshot = {
  revision: 0,
  devices: [],
  activeScenarios: 0,
  faultProfile: emptyFaultProfile,
};

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message;
  }
  return '本地模拟运行时暂时不可用。';
}

export const useRuntimeStore = defineStore('runtime', () => {
  const snapshot = ref<SimulatorRuntimeSnapshot>(emptySnapshot);
  const operations = ref<OperationRecord[]>([]);
  const events = ref<RuntimeEventRecord[]>([]);
  const queries = ref<QueryResult[]>([]);
  const transactions = ref<TransactionRecord[]>([]);
  const recordings = ref<RecordingEntry[]>([]);
  const scenarios = ref<ScenarioRuntimeState[]>([]);
  const isLoading = ref(false);
  const isSubmitting = ref(false);
  const errorMessage = ref<string | null>(null);
  let pollTimer: ReturnType<typeof setTimeout> | null = null;
  let isPolling = false;

  const deviceOptions = computed(() =>
    snapshot.value.devices.map((device) => ({ label: device.name, value: device.deviceId })),
  );

  async function refresh(): Promise<boolean> {
    if (isLoading.value) return true;
    isLoading.value = true;
    try {
      snapshot.value = await runtimeService.snapshot();
      errorMessage.value = null;
      return true;
    } catch (error) {
      errorMessage.value = messageOf(error);
      return false;
    } finally {
      isLoading.value = false;
    }
  }

  async function refreshProjections(): Promise<boolean> {
    try {
      const [
        nextOperations,
        nextEvents,
        nextQueries,
        nextTransactions,
        nextRecordings,
        nextScenarios,
      ] = await Promise.all([
        runtimeService.operations(),
        runtimeService.events(),
        runtimeService.queries(),
        runtimeService.transactions(),
        runtimeService.recordings(),
        runtimeService.scenarios(),
      ]);
      operations.value = nextOperations;
      events.value = nextEvents;
      queries.value = nextQueries;
      transactions.value = nextTransactions;
      recordings.value = nextRecordings;
      scenarios.value = nextScenarios;
      errorMessage.value = null;
      return true;
    } catch (error) {
      errorMessage.value = messageOf(error);
      return false;
    }
  }

  function startPolling(): void {
    if (isPolling) return;
    isPolling = true;
    const poll = async (): Promise<void> => {
      if (!isPolling) return;
      await Promise.all([refresh(), refreshProjections()]);
      pollTimer = setTimeout(() => void poll(), 500);
    };
    void poll();
  }

  function stopPolling(): void {
    isPolling = false;
    if (pollTimer !== null) clearTimeout(pollTimer);
    pollTimer = null;
  }

  async function submit<T>(operation: () => Promise<T>): Promise<T | null> {
    if (isSubmitting.value) return null;
    isSubmitting.value = true;
    try {
      const result = await operation();
      errorMessage.value = null;
      await refresh();
      return result;
    } catch (error) {
      errorMessage.value = messageOf(error);
      return null;
    } finally {
      isSubmitting.value = false;
    }
  }

  return {
    snapshot,
    operations,
    events,
    queries,
    transactions,
    recordings,
    scenarios,
    isLoading,
    isSubmitting,
    errorMessage,
    deviceOptions,
    refresh,
    refreshProjections,
    startPolling,
    stopPolling,
    setFaultProfile: (profile: FaultProfile) =>
      submit(() => runtimeService.setFaultProfile(profile)),
    controlDevice: (deviceId: string, command: DeviceControlCommand) =>
      submit(() => runtimeService.controlDevice(deviceId, command)),
    controlPtz: (deviceId: string, channelId: string, command: PtzCommand) =>
      submit(() => runtimeService.controlPtz(deviceId, channelId, command)),
    updateAlarm: (deviceId: string, channelId: string, command: AlarmCommand) =>
      submit(() => runtimeService.updateAlarm(deviceId, channelId, command)),
    updatePosition: (deviceId: string, channelId: string, command: PositionCommand) =>
      submit(() => runtimeService.updatePosition(deviceId, channelId, command)),
    controlRecording: (deviceId: string, channelId: string, command: RecordingCommand) =>
      submit(() => runtimeService.controlRecording(deviceId, channelId, command)),
    controlSubscription: (deviceId: string, channelId: string, command: SubscriptionCommand) =>
      submit(() => runtimeService.controlSubscription(deviceId, channelId, command)),
    executeQuery: (request: QueryRequest) => submit(() => runtimeService.executeQuery(request)),
    saveScenario: (definition: ScenarioDefinition) =>
      submit(() => runtimeService.saveScenario(definition)),
    startScenario: (id: string) => submit(() => runtimeService.startScenario(id)),
    setScenarioStatus: (id: string, status: ScenarioStatus) =>
      submit(() => runtimeService.setScenarioStatus(id, status)),
  };
});
