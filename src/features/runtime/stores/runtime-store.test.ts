import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const service = vi.hoisted(() => ({
  snapshot: vi.fn(),
  operations: vi.fn(),
  events: vi.fn(),
  queries: vi.fn(),
  transactions: vi.fn(),
  recordings: vi.fn(),
  scenarios: vi.fn(),
  setFaultProfile: vi.fn(),
  controlDevice: vi.fn(),
  controlPtz: vi.fn(),
  updateAlarm: vi.fn(),
  updatePosition: vi.fn(),
  controlRecording: vi.fn(),
  executeQuery: vi.fn(),
  saveScenario: vi.fn(),
  startScenario: vi.fn(),
  setScenarioStatus: vi.fn(),
}));

vi.mock('../services/runtime-service', () => ({ runtimeService: service }));

import { useRuntimeStore } from './runtime-store';

const snapshot = {
  revision: 1,
  activeScenarios: 0,
  faultProfile: {
    delayMillis: 0,
    forceTimeout: false,
    packetLossPercent: 0,
    rejectStatus: null,
    forceDeviceOffline: false,
  },
  devices: [
    {
      deviceId: '34020000001320000100',
      name: '模拟设备',
      connectivity: 'online',
      guarded: false,
      clockOffsetMillis: 0,
      lastPlatformRequestAt: null,
      lastOperationId: null,
      channels: [],
    },
  ],
} as const;

describe('runtime store', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    service.snapshot.mockResolvedValue(structuredClone(snapshot));
    service.operations.mockResolvedValue([]);
    service.events.mockResolvedValue([]);
    service.queries.mockResolvedValue([]);
    service.transactions.mockResolvedValue([]);
    service.recordings.mockResolvedValue([]);
    service.scenarios.mockResolvedValue([]);
  });

  it('refreshes the authoritative Rust snapshot and projections together', async () => {
    const store = useRuntimeStore();

    expect(await store.refresh()).toBe(true);
    expect(store.snapshot.revision).toBe(1);
    expect(store.deviceOptions).toEqual([{ label: '模拟设备', value: '34020000001320000100' }]);
  });

  it('refreshes state after a recording command succeeds', async () => {
    service.controlRecording.mockResolvedValue({ id: 'operation-1' });
    const store = useRuntimeStore();

    const result = await store.controlRecording('34020000001320000100', 'channel-1', {
      kind: 'start',
      name: '录像 1',
    });

    expect(result).toEqual({ id: 'operation-1' });
    expect(service.controlRecording).toHaveBeenCalledOnce();
    expect(service.snapshot).toHaveBeenCalledOnce();
  });
});
