//! 注册运行时共享调度器。
//!
//! 调度器只产生统一节拍，不为每台设备创建独立的 Tokio 定时器；设备状态机
//! 根据节拍和自身截止时间决定执行注册刷新、Keepalive 或重试动作。

use std::time::Duration;

use tokio::{sync::broadcast, task::JoinHandle, time::MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use super::time::now_millis;

const TICK_INTERVAL: Duration = Duration::from_secs(1);
const CHANNEL_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug)]
pub(super) struct SchedulerTick {
    pub(super) now_millis: u64,
}

pub(super) struct Scheduler {
    tick_tx: broadcast::Sender<SchedulerTick>,
    task: JoinHandle<()>,
}

impl Scheduler {
    pub(super) fn start(cancellation: CancellationToken) -> Self {
        let (tick_tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        let publisher = tick_tx.clone();
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(TICK_INTERVAL);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let _ = publisher.send(SchedulerTick { now_millis: now_millis() });
                    }
                }
            }
        });
        Self { tick_tx, task }
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<SchedulerTick> {
        self.tick_tx.subscribe()
    }

    pub(super) async fn join(self) {
        let _ = self.task.await;
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::{Duration, timeout};
    use tokio_util::sync::CancellationToken;

    use super::Scheduler;

    #[tokio::test]
    async fn scheduler_should_emit_ticks_and_exit_after_cancellation() {
        let cancellation = CancellationToken::new();
        let scheduler = Scheduler::start(cancellation.clone());
        let mut ticks = scheduler.subscribe();
        let Ok(Ok(tick)) = timeout(Duration::from_secs(2), ticks.recv()).await else {
            cancellation.cancel();
            scheduler.join().await;
            return;
        };
        assert!(tick.now_millis > 0);
        cancellation.cancel();
        scheduler.join().await;
    }
}
