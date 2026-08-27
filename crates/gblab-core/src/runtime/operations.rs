//! 注册、刷新、Keepalive、重试与注销的生命周期编排。
//!
//! Supervisor 只投递命令和接收结果；本模块通过受限 transient task 执行网络事务。

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use siprs::{siprs_gb28181_codec::DeviceId as CodecDeviceId, siprs_gb28181_xml::Notify};
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time::sleep,
};
use tokio_util::sync::CancellationToken;

use crate::{
    SimulatedDevice, SipServiceConfiguration,
    runtime::scheduler::Scheduler,
    sip::{DeviceSipSession, SipRegistrationClient, SipRegistrationError},
};

use super::{
    business::{SessionMap, run_business_commands},
    registration::{INTERNAL_EVENT_QUEUE_CAPACITY, InternalEvent},
    time::now_millis,
    types::DeviceRegistrationStatus,
};

const REGISTRATION_ATTEMPTS: u8 = 3;
const RETRY_CYCLE_DELAY: Duration = Duration::from_secs(30);
const TRANSIENT_OPERATION_QUEUE_CAPACITY: usize = 512;

#[expect(
    clippy::too_many_lines,
    reason = "单一 Runtime owner 统一编排注册、刷新、Keepalive、重试和注销"
)]
pub(super) async fn run_registration_operation(
    configuration: SipServiceConfiguration,
    devices: Vec<SimulatedDevice>,
    concurrency: usize,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    let (transport_event_tx, mut transport_event_rx) = mpsc::channel(INTERNAL_EVENT_QUEUE_CAPACITY);
    let client = match SipRegistrationClient::connect(&configuration, transport_event_tx).await {
        Ok(client) => client,
        Err(error) => {
            for device in devices {
                send_device_state(
                    &internal_tx,
                    device.id.as_str(),
                    DeviceRegistrationStatus::Failed,
                    Some(error.to_string()),
                    None,
                )
                .await;
                let _ = internal_tx.send(InternalEvent::InitialSettled).await;
            }
            let _ = internal_tx.send(InternalEvent::OperationFinished).await;
            return;
        }
    };

    let transport_cancellation = CancellationToken::new();
    let scheduler = Scheduler::start(cancellation.clone());
    let catalog_devices = Arc::new(
        devices
            .iter()
            .map(|device| (device.id.to_string(), device.clone()))
            .collect::<std::collections::HashMap<_, _>>(),
    );
    let receiver_task = tokio::spawn(
        Arc::clone(&client)
            .receive_loop(transport_cancellation.clone(), Arc::clone(&catalog_devices)),
    );
    let transport_forward_tx = internal_tx.clone();
    let transport_forward_task = tokio::spawn(async move {
        while let Some(event) = transport_event_rx.recv().await {
            if transport_forward_tx
                .try_send(InternalEvent::Sip(event))
                .is_err()
            {
                let _ = transport_forward_tx.try_send(InternalEvent::DroppedLog);
            }
        }
    });

    let (business_tx, business_rx) = mpsc::channel(32);
    let _ = internal_tx
        .send(InternalEvent::BusinessChannel(business_tx))
        .await;

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let session_map = Arc::new(tokio::sync::Mutex::new(BTreeMap::new()));

    // 业务消费者必须在首轮注册开始前就绪；已注册设备可以立即处理平台请求。
    let business_task = tokio::spawn(run_business_commands(
        business_rx,
        Arc::clone(&session_map),
        Arc::clone(&client),
        Arc::clone(&catalog_devices),
        configuration.clone(),
        cancellation.clone(),
        internal_tx.clone(),
    ));

    let mut runtime_devices = BTreeMap::new();
    let mut device_queue = devices.into_iter();
    let mut registration_tasks = JoinSet::new();
    while registration_tasks.len() < concurrency.max(1) {
        let Some(device) = device_queue.next() else {
            break;
        };
        let _ = queue_initial_registration(
            device,
            &configuration,
            &client,
            &semaphore,
            &cancellation,
            &internal_tx,
            &session_map,
            &mut registration_tasks,
        )
        .await;
    }
    while let Some(joined) = registration_tasks.join_next().await {
        let Ok((device_id, session, registration)) = joined else {
            continue;
        };
        let now = now_millis();
        match registration {
            Ok(expires) => {
                send_device_state(
                    &internal_tx,
                    &device_id,
                    DeviceRegistrationStatus::Registered,
                    None,
                    Some(now.saturating_add(duration_millis(expires))),
                )
                .await;
                runtime_devices.insert(
                    device_id.clone(),
                    RuntimeDeviceState {
                        session,
                        next_refresh_at: now
                            .saturating_add(duration_millis_u64(refresh_delay(expires).as_secs())),
                        next_keepalive_at: now.saturating_add(duration_millis(
                            configuration.keepalive_interval.max(1),
                        )),
                        next_retry_at: None,
                        in_flight: None,
                    },
                );
            }
            Err(error) => {
                send_device_state(
                    &internal_tx,
                    &device_id,
                    DeviceRegistrationStatus::Failed,
                    Some(error.to_string()),
                    None,
                )
                .await;
                runtime_devices.insert(
                    device_id.clone(),
                    RuntimeDeviceState {
                        session,
                        next_refresh_at: u64::MAX,
                        next_keepalive_at: u64::MAX,
                        next_retry_at: Some(
                            now.saturating_add(duration_millis_u64(RETRY_CYCLE_DELAY.as_secs())),
                        ),
                        in_flight: None,
                    },
                );
            }
        }
        let _ = internal_tx.send(InternalEvent::InitialSettled).await;
        while registration_tasks.len() < concurrency.max(1) {
            let Some(device) = device_queue.next() else {
                break;
            };
            let _ = queue_initial_registration(
                device,
                &configuration,
                &client,
                &semaphore,
                &cancellation,
                &internal_tx,
                &session_map,
                &mut registration_tasks,
            )
            .await;
        }
    }
    // 所有设备共享一个 owner loop：设备只保存会话和截止时间，不创建生命周期 task。
    let mut scheduler_rx = scheduler.subscribe();
    let operation_limit = Arc::new(Semaphore::new(concurrency.max(1)));
    let (operation_tx, mut operation_rx) =
        mpsc::channel::<OperationCompleted>(TRANSIENT_OPERATION_QUEUE_CAPACITY);
    let mut transient_tasks = JoinSet::new();
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            completed = operation_rx.recv() => {
                let Some(completed) = completed else { break };
                let Some(runtime_device) = runtime_devices.get_mut(&completed.device_id) else { continue };
                runtime_device.in_flight = None;
                match completed.result {
                    Ok(OperationResult::Registered(expires)) => {
                        let now = now_millis();
                        runtime_device.next_retry_at = None;
                        runtime_device.next_refresh_at = now.saturating_add(duration_millis_u64(refresh_delay(expires).as_secs()));
                        runtime_device.next_keepalive_at = now.saturating_add(duration_millis(configuration.keepalive_interval.max(1)));
                        send_device_state(&internal_tx, &completed.device_id, DeviceRegistrationStatus::Registered, None, Some(now.saturating_add(duration_millis(expires)))).await;
                    }
                    Ok(OperationResult::Heartbeat) => {
                        let _ = internal_tx.send(InternalEvent::Heartbeat { device_id: completed.device_id.clone(), success: true, timestamp: now_millis() }).await;
                        runtime_device.next_keepalive_at = now_millis().saturating_add(duration_millis(configuration.keepalive_interval.max(1)));
                    }
                    Err(error) => {
                        if !matches!(error, SipRegistrationError::Cancelled) {
                            let now = now_millis();
                            if matches!(completed.operation, RuntimeOperation::Keepalive) {
                                let _ = internal_tx.send(InternalEvent::Heartbeat { device_id: completed.device_id.clone(), success: false, timestamp: now }).await;
                                runtime_device.next_keepalive_at = now.saturating_add(duration_millis(configuration.keepalive_interval.max(1)));
                            } else {
                                runtime_device.next_retry_at = Some(now.saturating_add(duration_millis_u64(RETRY_CYCLE_DELAY.as_secs())));
                                send_device_state(&internal_tx, &completed.device_id, DeviceRegistrationStatus::Failed, Some(error.to_string()), None).await;
                            }
                        }
                    }
                }
            }
            tick = scheduler_rx.recv() => {
                let Ok(tick) = tick else { break };
                while transient_tasks.try_join_next().is_some() {}
                if tick.now_millis == 0 { continue; }
                for (device_id, runtime_device) in &mut runtime_devices {
                    if runtime_device.in_flight.is_some() { continue; }
                    let operation = if runtime_device.next_retry_at.is_some_and(|deadline| tick.now_millis >= deadline) {
                        Some(RuntimeOperation::Retry)
                    } else if runtime_device.next_retry_at.is_none() && tick.now_millis >= runtime_device.next_refresh_at {
                        Some(RuntimeOperation::Refresh)
                    } else if tick.now_millis >= runtime_device.next_keepalive_at {
                        Some(RuntimeOperation::Keepalive)
                    } else { None };
                    let Some(operation) = operation else { continue };
                    let Ok(permit) = Arc::clone(&operation_limit).try_acquire_owned() else { continue };
                    runtime_device.in_flight = Some(operation);
                    let session = Arc::clone(&runtime_device.session);
                    let client = Arc::clone(&client);
                    let configuration = configuration.clone();
                    let cancellation = cancellation.clone();
                    let operation_tx = operation_tx.clone();
                    let device_id = device_id.clone();
                    transient_tasks.spawn(async move {
                        let _permit = permit;
                        let result = match operation {
                            RuntimeOperation::Refresh | RuntimeOperation::Retry => register_with_retry_unbounded(&session, &client, &configuration, &cancellation).await.map(OperationResult::Registered),
                            RuntimeOperation::Keepalive => {
                                match CodecDeviceId::parse(&device_id) {
                                    Ok(codec_id) => {
                                        let sn = session.next_sn();
                                        session
                                            .send_message(
                                                &client,
                                                Notify::keepalive(sn, codec_id).to_xml(),
                                                &cancellation,
                                                None,
                                            )
                                            .await
                                            .map(|()| OperationResult::Heartbeat)
                                    }
                                    Err(error) => Err(SipRegistrationError::Build(error.to_string())),
                                }
                            }
                        };
                        let _ = operation_tx.send(OperationCompleted { device_id, operation, result }).await;
                    });
                }
            }
        }
    }

    transient_tasks.abort_all();
    while transient_tasks.join_next().await.is_some() {}

    // 停止阶段也走同一个有界 transient executor，不再按设备串行等待。
    let mut unregister_tasks = JoinSet::new();
    for (device_id, runtime_device) in runtime_devices {
        send_device_state(
            &internal_tx,
            &device_id,
            DeviceRegistrationStatus::Unregistering,
            None,
            None,
        )
        .await;
        let session = Arc::clone(&runtime_device.session);
        let client = Arc::clone(&client);
        let configuration = configuration.clone();
        let cancellation = CancellationToken::new();
        let internal_tx = internal_tx.clone();
        unregister_tasks.spawn(async move {
            let result = session
                .unregister(&client, &configuration, &cancellation)
                .await;
            let (status, error) = match result {
                Ok(()) => (DeviceRegistrationStatus::Unregistered, None),
                Err(error) => (
                    DeviceRegistrationStatus::Failed,
                    Some(format!("注销失败: {error}")),
                ),
            };
            send_device_state(&internal_tx, &device_id, status, error, None).await;
        });
    }
    while unregister_tasks.join_next().await.is_some() {}

    transport_cancellation.cancel();
    let _ = receiver_task.await;
    client.clear_server_transactions().await;
    drop(client);
    let _ = transport_forward_task.await;
    scheduler.join().await;
    business_task.abort();
    let _ = internal_tx.send(InternalEvent::OperationFinished).await;
}

#[expect(
    clippy::too_many_arguments,
    reason = "首轮注册槽位需要共享配置、会话表、取消令牌和统一事件通道"
)]
async fn queue_initial_registration(
    device: SimulatedDevice,
    configuration: &SipServiceConfiguration,
    client: &Arc<SipRegistrationClient>,
    semaphore: &Arc<Semaphore>,
    cancellation: &CancellationToken,
    internal_tx: &mpsc::Sender<InternalEvent>,
    session_map: &SessionMap,
    registration_tasks: &mut JoinSet<(
        String,
        Arc<DeviceSipSession>,
        Result<u32, SipRegistrationError>,
    )>,
) -> bool {
    let device_id = device.id.to_string();
    let session = match DeviceSipSession::new(device_id.clone(), configuration, client) {
        Ok(session) => Arc::new(session),
        Err(error) => {
            send_device_state(
                internal_tx,
                &device_id,
                DeviceRegistrationStatus::Failed,
                Some(error.to_string()),
                None,
            )
            .await;
            let _ = internal_tx.send(InternalEvent::InitialSettled).await;
            return false;
        }
    };
    session_map
        .lock()
        .await
        .insert(device_id.clone(), Arc::clone(&session));
    send_device_state(
        internal_tx,
        &device_id,
        DeviceRegistrationStatus::Registering,
        None,
        None,
    )
    .await;
    let device_id_for_task = device_id.clone();
    let session_for_task = Arc::clone(&session);
    let client_for_task = Arc::clone(client);
    let configuration_for_task = configuration.clone();
    let semaphore_for_task = Arc::clone(semaphore);
    let cancellation_for_task = cancellation.clone();
    registration_tasks.spawn(async move {
        let registration = register_with_retry(
            &session_for_task,
            &client_for_task,
            &configuration_for_task,
            &semaphore_for_task,
            &cancellation_for_task,
        )
        .await;
        (device_id_for_task, session_for_task, registration)
    });
    true
}

struct RuntimeDeviceState {
    session: Arc<DeviceSipSession>,
    next_refresh_at: u64,
    next_keepalive_at: u64,
    next_retry_at: Option<u64>,
    in_flight: Option<RuntimeOperation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeOperation {
    Refresh,
    Retry,
    Keepalive,
}

struct OperationCompleted {
    device_id: String,
    operation: RuntimeOperation,
    result: Result<OperationResult, SipRegistrationError>,
}

enum OperationResult {
    Registered(u32),
    Heartbeat,
}

async fn register_with_retry(
    session: &DeviceSipSession,
    client: &SipRegistrationClient,
    configuration: &SipServiceConfiguration,
    semaphore: &Semaphore,
    cancellation: &CancellationToken,
) -> Result<u32, SipRegistrationError> {
    let mut backoff = Duration::from_secs(1);
    let mut last_error = SipRegistrationError::Timeout;
    for attempt in 0..REGISTRATION_ATTEMPTS {
        let permit = tokio::select! {
            () = cancellation.cancelled() => return Err(SipRegistrationError::Cancelled),
            permit = semaphore.acquire() => permit.map_err(|_| SipRegistrationError::Cancelled)?,
        };
        let result = session.register(client, configuration, cancellation).await;
        drop(permit);
        match result {
            Ok(expires) => return Ok(expires),
            Err(SipRegistrationError::Rejected { code, reason }) if code == 403 => {
                return Err(SipRegistrationError::Rejected { code, reason });
            }
            Err(SipRegistrationError::Cancelled) => {
                return Err(SipRegistrationError::Cancelled);
            }
            Err(error) => last_error = error,
        }
        if attempt + 1 < REGISTRATION_ATTEMPTS {
            tokio::select! {
                () = cancellation.cancelled() => return Err(SipRegistrationError::Cancelled),
                () = sleep(backoff) => {}
            }
            backoff = backoff.saturating_mul(2);
        }
    }
    Err(last_error)
}

/// transient executor 专用的注册重试；并发上限由调用方 executor 控制，
/// 不在 Runtime owner 内部再次获取同一信号量，避免慢事务阻塞其他设备。
async fn register_with_retry_unbounded(
    session: &DeviceSipSession,
    client: &SipRegistrationClient,
    configuration: &SipServiceConfiguration,
    cancellation: &CancellationToken,
) -> Result<u32, SipRegistrationError> {
    let mut backoff = Duration::from_secs(1);
    let mut last_error = SipRegistrationError::Timeout;
    for attempt in 0..REGISTRATION_ATTEMPTS {
        match session.register(client, configuration, cancellation).await {
            Ok(expires) => return Ok(expires),
            Err(SipRegistrationError::Rejected { code, reason }) if code == 403 => {
                return Err(SipRegistrationError::Rejected { code, reason });
            }
            Err(SipRegistrationError::Cancelled) => return Err(SipRegistrationError::Cancelled),
            Err(error) => last_error = error,
        }
        if attempt + 1 < REGISTRATION_ATTEMPTS {
            tokio::select! {
                () = cancellation.cancelled() => return Err(SipRegistrationError::Cancelled),
                () = sleep(backoff) => {}
            }
            backoff = backoff.saturating_mul(2);
        }
    }
    Err(last_error)
}

async fn send_device_state(
    internal_tx: &mpsc::Sender<InternalEvent>,
    device_id: &str,
    status: DeviceRegistrationStatus,
    last_error: Option<String>,
    expires_at: Option<u64>,
) {
    let _ = internal_tx
        .send(InternalEvent::DeviceState {
            device_id: device_id.to_owned(),
            status,
            last_error,
            expires_at,
        })
        .await;
}

fn refresh_delay(expires: u32) -> Duration {
    Duration::from_secs(
        u64::from(expires)
            .saturating_mul(4)
            .saturating_div(5)
            .max(1),
    )
}

fn duration_millis(seconds: u32) -> u64 {
    u64::from(seconds).saturating_mul(1_000)
}

const fn duration_millis_u64(seconds: u64) -> u64 {
    seconds.saturating_mul(1_000)
}
