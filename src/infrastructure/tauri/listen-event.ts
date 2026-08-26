import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** 订阅类型化 Tauri 事件，只向业务层暴露 payload。 */
export function listenEvent<TPayload>(
  eventName: string,
  handler: (payload: TPayload) => void,
): Promise<UnlistenFn> {
  return listen<TPayload>(eventName, (event) => handler(event.payload));
}
