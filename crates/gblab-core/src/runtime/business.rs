//! 报警、移动位置、设备控制与订阅 NOTIFY 的有界业务执行器。

use std::{collections::BTreeMap, fmt::Write, sync::Arc};

use siprs::siprs_gb28181_codec::DeviceId as CodecDeviceId;
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::{
    SimulatedDevice, SipServiceConfiguration,
    runtime::{PlatformCommandType, SubscriptionSnapshot},
    sip::{DeviceSipSession, NotifyDialogContext, SipRegistrationClient, SipRegistrationError},
};

use super::{
    registration::{BusinessCommand, InternalEvent},
    time::now_millis,
    types::{AlarmTrigger, DeviceControlAction, RegistrationRuntimeError},
};

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "有界业务执行器需要完整的会话、传输、设备和事件上下文"
)]
pub(super) async fn run_business_commands(
    mut rx: mpsc::Receiver<BusinessCommand>,
    sessions: SessionMap,
    client: Arc<SipRegistrationClient>,
    catalog_devices: Arc<std::collections::HashMap<String, SimulatedDevice>>,
    configuration: SipServiceConfiguration,
    cancellation: CancellationToken,
    internal_tx: mpsc::Sender<InternalEvent>,
) {
    let executor = Arc::new(Semaphore::new(32));
    let mut tasks = JoinSet::new();
    while let Some(command) = tokio::select! {
        command = rx.recv() => command,
        () = cancellation.cancelled() => None,
    } {
        let permit = tokio::select! {
            () = cancellation.cancelled() => break,
            permit = Arc::clone(&executor).acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let sessions = Arc::clone(&sessions);
        let client = Arc::clone(&client);
        let catalog_devices = Arc::clone(&catalog_devices);
        let configuration = configuration.clone();
        let cancellation = cancellation.clone();
        let internal_tx = internal_tx.clone();
        tasks.spawn(async move {
            let _permit = permit;
            match command {
            BusinessCommand::Alarm {
                alarm,
                subscription,
                reply,
            } => {
                let sn = sessions
                    .lock()
                    .await
                    .get(&alarm.device_id)
                    .map_or(1, |session| session.next_sn());
                let result = match build_alarm_notify_body(&alarm, sn) {
                    Ok(body) => {
                        send_business_notify(
                            &sessions,
                            &client,
                            cancellation.clone(),
                            &alarm.device_id,
                            Some(&alarm.channel_id),
                            &subscription,
                            body,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: alarm.device_id.clone(),
                        channel_id: Some(alarm.channel_id.clone()),
                        command_type: PlatformCommandType::Alarm,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: alarm.device_id.clone(),
                            action: None,
                            ptz_action: None,
                            guarded: None,
                            alarm_active: Some(true),
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::MobilePosition {
                device_id,
                channel_id,
                longitude,
                latitude,
                subscription,
                reply,
            } => {
                let sn = sessions
                    .lock()
                    .await
                    .get(&device_id)
                    .map_or(1, |session| session.next_sn());
                let result =
                    match build_mobile_position_notify_body(&channel_id, longitude, latitude, sn) {
                        Ok(body) => {
                            send_business_notify(
                                &sessions,
                                &client,
                                cancellation.clone(),
                                &device_id,
                                Some(&channel_id),
                                &subscription,
                                body,
                            )
                            .await
                        }
                        Err(error) => Err(error),
                    };
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: device_id.clone(),
                        channel_id: Some(channel_id.clone()),
                        command_type: PlatformCommandType::MobilePosition,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                let _ = reply.send(result);
            }
            BusinessCommand::DeviceControl {
                device_id,
                action,
                reply,
            } => {
                let result = send_business_message(
                    &sessions,
                    &client,
                    &configuration,
                    cancellation.clone(),
                    &device_id,
                    None,
                    format!(
                        "<Control><CmdType>DeviceControl</CmdType><SN>{}</SN><DeviceID>{device_id}</DeviceID><Type>{}</Type></Control>",
                        next_session_sn(&sessions, &device_id).await,
                        action.as_xml()
                    ),
                )
                .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: device_id.clone(),
                            action: Some(action.as_xml().to_owned()),
                            ptz_action: None,
                            guarded: match action {
                                DeviceControlAction::Guard => Some(true),
                                DeviceControlAction::Unguard | DeviceControlAction::AlarmReset => {
                                    Some(false)
                                }
                                DeviceControlAction::Restart => None,
                            },
                            alarm_active: matches!(action, DeviceControlAction::AlarmReset)
                                .then_some(false),
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::PtzControl {
                device_id,
                channel_id,
                action,
                reply,
            } => {
                let result = send_business_message(
                    &sessions,
                    &client,
                    &configuration,
                    cancellation.clone(),
                    &device_id,
                    Some(&channel_id),
                    format!(
                        "<Control><CmdType>DeviceControl</CmdType><SN>{}</SN><DeviceID>{channel_id}</DeviceID><PTZCmd>{}</PTZCmd></Control>",
                        next_session_sn(&sessions, &device_id).await,
                        action.as_xml()
                    ),
                )
                .await;
                if result.is_ok() {
                    let _ = internal_tx
                        .send(InternalEvent::ControlState {
                            device_id: device_id.clone(),
                            action: None,
                            ptz_action: Some(action.as_xml().to_owned()),
                            guarded: None,
                            alarm_active: None,
                        })
                        .await;
                }
                let _ = reply.send(result);
            }
            BusinessCommand::SubscriptionNotify {
                device_id,
                channel_id,
                command_type,
                subscription,
                reply,
            } => {
                let result = send_subscription_notify(
                    &sessions,
                    &client,
                    &catalog_devices,
                    cancellation.clone(),
                    &device_id,
                    channel_id.as_deref(),
                    command_type,
                    &subscription,
                )
                .await;
                let notification_error = result.as_ref().err().map(ToString::to_string);
                let _ = internal_tx
                    .send(InternalEvent::SubscriptionNotification {
                        device_id: device_id.clone(),
                        channel_id: channel_id.clone(),
                        command_type,
                        success: result.is_ok(),
                        error: notification_error,
                        timestamp: now_millis(),
                    })
                    .await;
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            }
        });
    }
    while tasks.join_next().await.is_some() {}
}

#[expect(
    clippy::too_many_arguments,
    reason = "订阅通知发送需要完整的订阅、设备和传输上下文"
)]
async fn send_subscription_notify(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    devices: &Arc<std::collections::HashMap<String, SimulatedDevice>>,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    command_type: PlatformCommandType,
    subscription: &SubscriptionSnapshot,
) -> Result<(), RegistrationRuntimeError> {
    let body_device_id = channel_id.unwrap_or(device_id);
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    let sn = session.next_sn();
    let body = match command_type {
        PlatformCommandType::Catalog => build_catalog_notify_body(body_device_id, devices, sn),
        _ => return Err(RegistrationRuntimeError::BusinessUnavailable),
    };
    session
        .send_notify(
            client,
            body,
            &cancellation,
            channel_id.map(str::to_owned),
            &notify_dialog_context(subscription),
        )
        .await
        .map_err(|error| map_business_error(&error))
}

fn build_alarm_notify_body(
    alarm: &AlarmTrigger,
    sn: u32,
) -> Result<String, RegistrationRuntimeError> {
    let codec_device_id = CodecDeviceId::parse(&alarm.channel_id)
        .map_err(|error| RegistrationRuntimeError::BusinessFailed(error.to_string()))?;
    Ok(format!(
        "<Notify><CmdType>Alarm</CmdType><SN>{sn}</SN><DeviceID>{}</DeviceID><AlarmPriority>{}</AlarmPriority><AlarmMethod>{}</AlarmMethod><AlarmTime>{}</AlarmTime><AlarmDescription>{}</AlarmDescription><Longitude>{:.6}</Longitude><Latitude>{:.6}</Latitude><Info><AlarmType>{}</AlarmType><AlarmStatus>{}</AlarmStatus></Info></Notify>",
        xml_escape(codec_device_id.as_ref()),
        xml_escape(&alarm.alarm_priority),
        xml_escape(&alarm.alarm_method),
        simulation_timestamp(),
        xml_escape(&alarm.description),
        alarm.longitude,
        alarm.latitude,
        xml_escape(&alarm.alarm_type),
        xml_escape(&alarm.alarm_status),
    ))
}

fn build_mobile_position_notify_body(
    device_id: &str,
    longitude: f64,
    latitude: f64,
    sn: u32,
) -> Result<String, RegistrationRuntimeError> {
    let codec_device_id = CodecDeviceId::parse(device_id)
        .map_err(|error| RegistrationRuntimeError::BusinessFailed(error.to_string()))?;
    Ok(format!(
        "<Notify><CmdType>MobilePosition</CmdType><SN>{sn}</SN><DeviceID>{}</DeviceID><Time>{}</Time><Longitude>{longitude:.6}</Longitude><Latitude>{latitude:.6}</Latitude><Speed>0.0</Speed><Direction>0.0</Direction><Altitude>0.0</Altitude></Notify>",
        xml_escape(codec_device_id.as_ref()),
        simulation_timestamp(),
    ))
}

fn simulation_timestamp() -> String {
    let seconds = i64::try_from(now_millis() / 1_000).unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * mp + 2).div_euclid(5) + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn build_catalog_notify_body(
    device_id: &str,
    devices: &std::collections::HashMap<String, SimulatedDevice>,
    sn: u32,
) -> String {
    let Some(device) = devices.get(device_id) else {
        return format!(
            "<Notify><CmdType>Catalog</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>0</SumNum><DeviceList Num=\"0\"></DeviceList></Notify>"
        );
    };
    let channels = crate::domain::derive_channels_for_device(device).unwrap_or_default();
    let count = channels.len();
    let mut items = String::new();
    for channel in channels {
        let _ = write!(
            items,
            "<Device><DeviceID>{}</DeviceID><Name>{}</Name><Manufacturer>{}</Manufacturer><Model>{}</Model><Status>ON</Status><ParentID>{}</ParentID></Device>",
            xml_escape(&channel.id.to_string()),
            xml_escape(&channel.name),
            xml_escape(&device.manufacturer),
            xml_escape(&device.model),
            xml_escape(&device.id.to_string())
        );
    }
    format!(
        "<Notify><CmdType>Catalog</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>{count}</SumNum><DeviceList Num=\"{count}\">{items}</DeviceList></Notify>"
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) type SessionMap = Arc<tokio::sync::Mutex<BTreeMap<String, Arc<DeviceSipSession>>>>;

async fn next_session_sn(sessions: &SessionMap, device_id: &str) -> u32 {
    sessions
        .lock()
        .await
        .get(device_id)
        .map_or(1, |session| session.next_sn())
}

#[expect(
    clippy::too_many_arguments,
    reason = "业务消息需要完整的设备、通道与传输上下文"
)]
async fn send_business_message(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    _configuration: &SipServiceConfiguration,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    body: String,
) -> Result<(), RegistrationRuntimeError> {
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    session
        .send_message(client, body, &cancellation, channel_id.map(str::to_owned))
        .await
        .map_err(|error| map_business_error(&error))
}

#[expect(
    clippy::too_many_arguments,
    reason = "业务通知发送需要完整的设备、订阅和传输上下文"
)]
async fn send_business_notify(
    sessions: &SessionMap,
    client: &Arc<SipRegistrationClient>,
    cancellation: CancellationToken,
    device_id: &str,
    channel_id: Option<&str>,
    subscription: &SubscriptionSnapshot,
    body: String,
) -> Result<(), RegistrationRuntimeError> {
    let session = sessions
        .lock()
        .await
        .get(device_id)
        .cloned()
        .ok_or(RegistrationRuntimeError::BusinessUnavailable)?;
    session
        .send_notify(
            client,
            body,
            &cancellation,
            channel_id.map(str::to_owned),
            &notify_dialog_context(subscription),
        )
        .await
        .map_err(|error| map_business_error(&error))
}

fn map_business_error(error: &SipRegistrationError) -> RegistrationRuntimeError {
    RegistrationRuntimeError::BusinessFailed(error.to_string())
}

fn notify_dialog_context(subscription: &SubscriptionSnapshot) -> NotifyDialogContext {
    NotifyDialogContext {
        call_id: subscription.call_id.clone(),
        remote_tag: subscription.remote_tag.clone(),
        local_tag: subscription.local_tag.clone(),
        event: subscription.event.clone(),
        cseq: subscription.notify_cseq,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{
        DeviceKind, SimulatedDevice,
        domain::{DeviceId, DeviceIdError},
    };

    use super::{
        AlarmTrigger, RegistrationRuntimeError, build_alarm_notify_body, build_catalog_notify_body,
        build_mobile_position_notify_body,
    };

    #[test]
    fn flat_alarm_notify_should_include_platform_required_root_fields()
    -> Result<(), RegistrationRuntimeError> {
        let xml = build_alarm_notify_body(
            &AlarmTrigger {
                device_id: "34020000001320000001".to_owned(),
                channel_id: "34020000001320000001".to_owned(),
                alarm_priority: "1".to_owned(),
                alarm_method: "2".to_owned(),
                alarm_type: "1".to_owned(),
                alarm_status: "Occur".to_owned(),
                description: "摄像机 <测试> & 报警".to_owned(),
                longitude: 116.397,
                latitude: 39.908,
            },
            7,
        )?;

        assert!(xml.contains("<AlarmPriority>1</AlarmPriority>"));
        assert!(xml.contains("<AlarmMethod>2</AlarmMethod>"));
        assert!(xml.contains("<AlarmTime>"));
        assert!(
            xml.contains("<AlarmDescription>摄像机 &lt;测试&gt; &amp; 报警</AlarmDescription>")
        );
        assert!(
            xml.contains("<Info><AlarmType>1</AlarmType><AlarmStatus>Occur</AlarmStatus></Info>")
        );
        assert!(!xml.contains("<AlarmList"));
        Ok(())
    }

    #[test]
    fn flat_mobile_position_notify_should_include_root_fields()
    -> Result<(), RegistrationRuntimeError> {
        let xml = build_mobile_position_notify_body("34020000001320000001", 116.397, 39.908, 8)?;

        assert!(xml.contains("<CmdType>MobilePosition</CmdType>"));
        assert!(xml.contains("<Time>"));
        assert!(xml.contains("<Longitude>116.397000</Longitude>"));
        assert!(xml.contains("<Latitude>39.908000</Latitude>"));
        assert!(!xml.contains("<DeviceList"));
        Ok(())
    }

    #[test]
    fn catalog_notify_should_only_contain_real_channels() -> Result<(), DeviceIdError> {
        let device = SimulatedDevice {
            id: DeviceId::new("34020000002000000100")?,
            name: "模拟摄像机-001".to_owned(),
            kind: DeviceKind::Camera,
            manufacturer: "GBLab".to_owned(),
            model: "SIM-CAM-100".to_owned(),
            firmware_version: "V1.0.0".to_owned(),
            channel_count: 1,
            created_at: 0,
        };
        let devices = HashMap::from([(device.id.to_string(), device)]);

        let xml = build_catalog_notify_body("34020000002000000100", &devices, 9);

        assert!(xml.contains("<SumNum>1</SumNum><DeviceList Num=\"1\">"));
        assert!(!xml.contains("<DeviceID>34020000002000000100</DeviceID><Name>"));
        assert!(xml.contains("<DeviceID>34020000002000100001</DeviceID>"));
        Ok(())
    }
}
