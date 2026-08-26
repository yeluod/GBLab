use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, SystemTime},
};

use siprs::{
    siprs_core::{Host, SipVersion, TransportProtocol},
    siprs_gb28181_xml::{
        CmdType, DeviceItem, DeviceStatusInfo, Message as GbMessage, Query, Response, parse_xml,
    },
    siprs_message::{
        AuthHeader, CSeqHeader, CallId, ContactHeader, FromToHeader, HeaderCollection, HeaderName,
        HeaderValue, MessageBuilder, MessageParser, Method, RequestLine, SipMessage, SipRequest,
        SipResponse, SipUri, Tag, ViaHeader,
    },
    siprs_registration::{DigestAuthHandler, auth::build_auth_header},
};
use thiserror::Error;
use tokio::{
    net::{UdpSocket, lookup_host},
    sync::{Mutex, mpsc, oneshot},
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;

use crate::{
    SimulatedDevice,
    configuration::{SipServiceConfiguration, SipTransport},
    domain::derive_channels_for_device,
    sip::transaction::{TransactionContext, TransactionKey, TransactionManager, TransactionState},
};

const SIP_MESSAGE_LIMIT: usize = 65_536;
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(8);
const FIRST_RETRANSMIT_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRANSMISSIONS: u8 = 4;

#[derive(Clone, Copy, Debug)]
pub enum SipLogDirection {
    Send,
    Receive,
}

#[derive(Debug)]
pub struct SipTransportEvent {
    pub(crate) timestamp_millis: u64,
    pub(crate) device_id: String,
    pub(crate) direction: SipLogDirection,
    pub(crate) message: String,
    pub(crate) channel_id: Option<String>,
    pub(crate) is_request: bool,
    pub(crate) method: Option<String>,
    pub(crate) command_type: Option<String>,
    pub(crate) event: Option<String>,
    pub(crate) local_tag: Option<String>,
    pub(crate) from_tag: Option<String>,
    pub(crate) request_uri: Option<String>,
    pub(crate) call_id: Option<String>,
    pub(crate) expires: Option<u32>,
}

#[derive(Debug, Error)]
pub enum SipRegistrationError {
    #[error("当前注册运行时仅支持 UDP 传输")]
    UnsupportedTransport,
    #[error("SIP 地址无效: {0}")]
    InvalidUri(String),
    #[error("本地监听地址无效: {0}")]
    InvalidBindAddress(String),
    #[error("无法解析 SIP 平台地址: {0}")]
    Resolve(String),
    #[error("无法绑定或连接 SIP socket: {0}")]
    Socket(String),
    #[error("SIP 报文构建失败: {0}")]
    Build(String),
    #[error("SIP Digest 认证失败: {0}")]
    Authentication(String),
    #[error("SIP 事务等待响应超时")]
    Timeout,
    #[error("SIP 事务已取消")]
    Cancelled,
    #[error("SIP 响应缺少必要认证挑战")]
    MissingChallenge,
    #[error("SIP 平台返回 {code} {reason}")]
    Rejected { code: u16, reason: String },
    #[error("SIP 平台返回 423，但没有有效的 Min-Expires")]
    MissingMinExpires,
    #[error("SIP 运行时事件队列已关闭")]
    EventChannelClosed,
}

pub struct SipRegistrationClient {
    socket: Arc<UdpSocket>,
    advertised_ip: IpAddr,
    local_port: u16,
    registrar: SipUri,
    domain: String,
    platform_id: String,
    subscription_tags: Mutex<HashMap<String, String>>,
    transactions: TransactionManager,
    event_tx: mpsc::Sender<SipTransportEvent>,
    dialogs: Mutex<crate::runtime::DialogManager>,
    invite_transactions: Mutex<HashSet<TransactionKey>>,
}

impl SipRegistrationClient {
    pub(crate) async fn connect(
        configuration: &SipServiceConfiguration,
        event_tx: mpsc::Sender<SipTransportEvent>,
    ) -> Result<Arc<Self>, SipRegistrationError> {
        if configuration.transport != SipTransport::Udp {
            return Err(SipRegistrationError::UnsupportedTransport);
        }

        let registrar_endpoint = SipUri::parse(&configuration.uri)
            .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let host = registrar_endpoint.host.as_str();
        let port = registrar_endpoint.port.unwrap_or(5_060);
        let registrar = SipUri::parse(&format!(
            "sip:{}@{}:{}",
            configuration.platform_id, host, port
        ))
        .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let remote = lookup_host((host.as_ref(), port))
            .await
            .map_err(|error| SipRegistrationError::Resolve(error.to_string()))?
            .next()
            .ok_or_else(|| SipRegistrationError::Resolve(configuration.uri.clone()))?;
        let bind_ip = configuration
            .local_bind_address
            .parse::<IpAddr>()
            .map_err(|_| {
                SipRegistrationError::InvalidBindAddress(configuration.local_bind_address.clone())
            })?;
        let socket = UdpSocket::bind(SocketAddr::new(bind_ip, configuration.local_port))
            .await
            .map_err(|error| SipRegistrationError::Socket(error.to_string()))?;
        socket
            .connect(remote)
            .await
            .map_err(|error| SipRegistrationError::Socket(error.to_string()))?;
        let local_address = socket
            .local_addr()
            .map_err(|error| SipRegistrationError::Socket(error.to_string()))?;
        let advertised_ip = if configuration.advertised_address.is_empty() {
            local_address.ip()
        } else {
            configuration
                .advertised_address
                .parse::<IpAddr>()
                .map_err(|_| {
                    SipRegistrationError::InvalidBindAddress(
                        configuration.advertised_address.clone(),
                    )
                })?
        };

        Ok(Arc::new(Self {
            socket: Arc::new(socket),
            advertised_ip,
            local_port: local_address.port(),
            registrar,
            domain: configuration.domain.clone(),
            platform_id: configuration.platform_id.clone(),
            subscription_tags: Mutex::new(HashMap::new()),
            transactions: TransactionManager::default(),
            event_tx,
            dialogs: Mutex::new(crate::runtime::DialogManager::default()),
            invite_transactions: Mutex::new(HashSet::new()),
        }))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "共享接收循环必须完成解析、响应和事件投影"
    )]
    pub(crate) async fn receive_loop(
        self: Arc<Self>,
        cancellation: CancellationToken,
        catalog_devices: Arc<HashMap<String, SimulatedDevice>>,
    ) {
        let parser = MessageParser::new(SIP_MESSAGE_LIMIT);
        let mut buffer = vec![0_u8; SIP_MESSAGE_LIMIT];
        loop {
            let received = tokio::select! {
                () = cancellation.cancelled() => break,
                received = self.socket.recv(&mut buffer) => received,
            };
            let Ok(size) = received else {
                break;
            };
            let raw = &buffer[..size];
            let Ok(message) = parser.parse(raw) else {
                continue;
            };
            let SipMessage::Response(response) = message else {
                let SipMessage::Request(request) = message else {
                    continue;
                };
                let text = String::from_utf8_lossy(raw).into_owned();
                let body = request_body_text(&request);
                let (requested_id, parsed_command_type) = xml_metadata(&body);
                let requested_id = requested_id.unwrap_or_default();
                let (device_id, channel_id) =
                    resolve_device_and_channel(&requested_id, &catalog_devices);
                let method = Some(request.request_line.method.to_string());
                let is_subscribe = method.as_deref() == Some("SUBSCRIBE");
                let command_type = parsed_command_type;
                let event =
                    structured_header_value(&request, &HeaderName::Extension("Event".to_owned()));
                let from_tag = request
                    .headers
                    .get(&HeaderName::From)
                    .and_then(HeaderValue::as_from_to)
                    .and_then(|header| header.tag.as_ref())
                    .map(ToString::to_string);
                let request_uri = Some(request.request_line.request_uri.to_string());
                let call_id = request
                    .headers
                    .get(&HeaderName::CallId)
                    .and_then(HeaderValue::as_call_id)
                    .map(|value| value.0.clone());
                let expires = request
                    .headers
                    .get(&HeaderName::Expires)
                    .and_then(header_u32_value);
                let local_tag = if is_subscribe {
                    if let Some(call_id) = call_id.as_ref() {
                        let mut tags = self.subscription_tags.lock().await;
                        Some(
                            tags.entry(call_id.clone())
                                .or_insert_with(|| Tag::new().to_string())
                                .clone(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };
                let _ = self
                    .event_tx
                    .send(SipTransportEvent {
                        timestamp_millis: now_millis(),
                        device_id: device_id.clone(),
                        direction: SipLogDirection::Receive,
                        message: text.clone(),
                        channel_id,
                        is_request: true,
                        method: method.clone(),
                        command_type,
                        event,
                        local_tag: local_tag.clone(),
                        from_tag,
                        request_uri: request_uri.clone(),
                        call_id: call_id.clone(),
                        expires,
                    })
                    .await;
                let disposition = self
                    .dispatch_inbound_request(&request, &text, method.as_deref(), &catalog_devices)
                    .await;
                let response = disposition.response().map(|(status, reason)| {
                    build_request_response(
                        &text,
                        status,
                        reason,
                        local_tag.as_deref(),
                        &device_id,
                        self.advertised_ip,
                        self.local_port,
                    )
                });
                if let Some(response) = response {
                    let response_call_id = extract_header(&response, "Call-ID");
                    let _ = self.socket.send(response.as_bytes()).await;
                    let _ = self
                        .event_tx
                        .send(SipTransportEvent {
                            timestamp_millis: now_millis(),
                            device_id: device_id.clone(),
                            direction: SipLogDirection::Send,
                            message: response,
                            channel_id: None,
                            is_request: false,
                            method: None,
                            command_type: None,
                            event: None,
                            local_tag: None,
                            from_tag: None,
                            request_uri: None,
                            call_id: response_call_id,
                            expires: None,
                        })
                        .await;
                    if is_subscribe
                        && expires == Some(0)
                        && let Some(call_id) = call_id.as_ref()
                    {
                        self.subscription_tags.lock().await.remove(call_id);
                    }
                }
                if let InboundRequestDisposition::RespondAndQuery { body } = disposition {
                    let response_message = self.build_query_response(&text, &body);
                    let response_call_id = extract_header(&response_message, "Call-ID");
                    let _ = self.socket.send(response_message.as_bytes()).await;
                    let _ = self
                        .event_tx
                        .send(SipTransportEvent {
                            timestamp_millis: now_millis(),
                            device_id,
                            direction: SipLogDirection::Send,
                            message: response_message,
                            channel_id: None,
                            is_request: false,
                            method: Some("MESSAGE".to_owned()),
                            command_type: xml_command_type(&body),
                            event: None,
                            local_tag: None,
                            from_tag: None,
                            request_uri: None,
                            call_id: response_call_id,
                            expires: None,
                        })
                        .await;
                }
                continue;
            };
            let Some(call_id) = response
                .headers
                .get(&HeaderName::CallId)
                .and_then(HeaderValue::as_call_id)
                .map(|value| value.0.clone())
            else {
                continue;
            };
            let transaction_key = transaction_key_from_response(&response);
            let is_final = response_class(response.status_line.status_code.0).is_final();
            let context = match transaction_key.as_ref() {
                Some(key) => self.transactions.context(key).await,
                None => None,
            };
            if let Some(context) = context {
                let _ = self
                    .event_tx
                    .send(SipTransportEvent {
                        timestamp_millis: now_millis(),
                        device_id: context.device_id,
                        direction: SipLogDirection::Receive,
                        message: String::from_utf8_lossy(raw).into_owned(),
                        channel_id: context.channel_id,
                        is_request: false,
                        method: context.method,
                        command_type: context.command_type,
                        event: None,
                        local_tag: None,
                        from_tag: None,
                        request_uri: None,
                        call_id: Some(call_id.clone()),
                        expires: None,
                    })
                    .await;
            } else {
                let response_text = String::from_utf8_lossy(raw).into_owned();
                let requested_id = response
                    .headers
                    .get(&HeaderName::To)
                    .or_else(|| response.headers.get(&HeaderName::From))
                    .and_then(HeaderValue::as_from_to)
                    .and_then(|header| header.uri.user_info.as_ref())
                    .map(|user| user.user.clone());
                let (device_id, channel_id) = requested_id
                    .as_deref()
                    .map(|value| resolve_device_and_channel(value, &catalog_devices))
                    .unwrap_or_default();
                let method = response
                    .headers
                    .get(&HeaderName::CSeq)
                    .and_then(HeaderValue::as_cseq)
                    .map(|cseq| cseq.method.to_string());
                let _ = self
                    .event_tx
                    .send(SipTransportEvent {
                        timestamp_millis: now_millis(),
                        device_id,
                        direction: SipLogDirection::Receive,
                        message: response_text,
                        channel_id,
                        is_request: false,
                        method,
                        command_type: None,
                        event: None,
                        local_tag: None,
                        from_tag: None,
                        request_uri: None,
                        call_id: Some(call_id.clone()),
                        expires: None,
                    })
                    .await;
            }
            if is_final
                && let Some(sender) = self.transactions.take_final(transaction_key.as_ref()).await
            {
                let _ = sender.send(response);
            }
        }
    }

    /// 执行带通道上下文的业务交换，并只接受 SIP 2xx。
    async fn exchange_with_channel(
        &self,
        device_id: &str,
        request: SipRequest,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
    ) -> Result<SipResponse, SipRegistrationError> {
        let response = self
            .exchange_raw(device_id, request, cancellation, channel_id)
            .await?;
        accept_sip_success(response)
    }

    async fn exchange_raw(
        &self,
        device_id: &str,
        request: SipRequest,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
    ) -> Result<SipResponse, SipRegistrationError> {
        let transaction_key = transaction_key_from_request(&request)
            .ok_or_else(|| SipRegistrationError::Build("SIP 请求缺少事务头部".to_owned()))?;
        let request_method_name = request.request_line.method.to_string();
        let call_id = request
            .headers
            .get(&HeaderName::CallId)
            .and_then(HeaderValue::as_call_id)
            .map(|value| value.0.clone())
            .ok_or_else(|| SipRegistrationError::Build("SIP 请求缺少 Call-ID".to_owned()))?;
        let payload = MessageBuilder::with_validation(true)
            .build(&SipMessage::Request(request))
            .map_err(|error| SipRegistrationError::Build(error.to_string()))?;
        let (sender, mut receiver) = oneshot::channel();
        self.transactions
            .register(
                transaction_key.clone(),
                TransactionContext {
                    device_id: device_id.to_owned(),
                    channel_id: channel_id.clone(),
                    method: Some(request_method_name),
                    command_type: payload_command_type(&payload),
                },
                sender,
            )
            .await;

        let deadline = Instant::now() + TRANSACTION_TIMEOUT;
        let mut delay = FIRST_RETRANSMIT_DELAY;
        let mut transmissions = 0_u8;
        let result = loop {
            if let Err(error) = self.socket.send(&payload).await {
                break Err(SipRegistrationError::Socket(error.to_string()));
            }
            transmissions = transmissions.saturating_add(1);
            self.event_tx
                .send(SipTransportEvent {
                    timestamp_millis: now_millis(),
                    device_id: device_id.to_owned(),
                    direction: SipLogDirection::Send,
                    message: String::from_utf8_lossy(&payload).into_owned(),
                    channel_id: channel_id.clone(),
                    is_request: true,
                    method: request_method(&payload),
                    command_type: payload_command_type(&payload),
                    event: None,
                    local_tag: None,
                    from_tag: None,
                    request_uri: None,
                    call_id: Some(call_id.clone()),
                    expires: None,
                })
                .await
                .map_err(|_| SipRegistrationError::EventChannelClosed)?;

            let wake_at = if transmissions < MAX_RETRANSMISSIONS {
                (Instant::now() + delay).min(deadline)
            } else {
                deadline
            };
            tokio::select! {
                () = cancellation.cancelled() => break Err(SipRegistrationError::Cancelled),
                response = &mut receiver => {
                    break response.map_err(|_| SipRegistrationError::Timeout);
                }
                () = sleep_until(wake_at) => {
                    if Instant::now() >= deadline {
                        let _ = self
                            .transactions
                            .finish_without_response(&transaction_key, TransactionState::TimedOut)
                            .await;
                        break Err(SipRegistrationError::Timeout);
                    }
                    let _ = self.transactions.record_retransmission(&transaction_key).await;
                    delay = delay.saturating_mul(2);
                }
            }
        };

        if matches!(result, Err(SipRegistrationError::Cancelled)) {
            let _ = self
                .transactions
                .finish_without_response(&transaction_key, TransactionState::Cancelled)
                .await;
        } else {
            self.transactions.remove(&transaction_key).await;
        }
        result
    }

    async fn dispatch_inbound_request(
        &self,
        request: &SipRequest,
        raw: &str,
        method: Option<&str>,
        devices: &HashMap<String, SimulatedDevice>,
    ) -> InboundRequestDisposition {
        let method_name = method.unwrap_or_default().to_ascii_uppercase();
        match method_name.as_str() {
            "CANCEL" => {
                let Some(call_id) = request_call_id(request) else {
                    return InboundRequestDisposition::Respond {
                        status: 481,
                        reason: "Call/Transaction Does Not Exist",
                    };
                };
                let mut invites = self.invite_transactions.lock().await;
                let key = invites.iter().find(|key| key.call_id == call_id).cloned();
                let disposition = key.map_or(
                    InboundRequestDisposition::Respond {
                        status: 481,
                        reason: "Call/Transaction Does Not Exist",
                    },
                    |key| {
                        invites.remove(&key);
                        InboundRequestDisposition::Respond {
                            status: 200,
                            reason: "OK",
                        }
                    },
                );
                drop(invites);
                disposition
            }
            "BYE" => {
                let Some(dialog_id) = request_dialog_id(request) else {
                    return InboundRequestDisposition::Respond {
                        status: 481,
                        reason: "Call/Transaction Does Not Exist",
                    };
                };
                if self.dialogs.lock().await.terminate(&dialog_id) {
                    InboundRequestDisposition::Respond {
                        status: 200,
                        reason: "OK",
                    }
                } else {
                    InboundRequestDisposition::Respond {
                        status: 481,
                        reason: "Call/Transaction Does Not Exist",
                    }
                }
            }
            "ACK" => {
                if let Some(dialog_id) = request_dialog_id(request)
                    && let Some(cseq) = request_cseq(request)
                {
                    let _ = self.dialogs.lock().await.confirm(&dialog_id, cseq);
                }
                InboundRequestDisposition::NoResponse
            }
            "INVITE" => {
                if let Some(key) = transaction_key_from_request(request) {
                    self.invite_transactions.lock().await.insert(key);
                }
                // 媒体能力尚未启用，明确拒绝而不是伪造 200/SDP。
                InboundRequestDisposition::Respond {
                    status: 488,
                    reason: "Not Acceptable Here",
                }
            }
            _ => dispatch_inbound_request(raw, method, devices),
        }
    }

    const fn endpoint_host(&self) -> Host {
        match self.advertised_ip {
            IpAddr::V4(address) => Host::IPv4(address),
            IpAddr::V6(address) => Host::IPv6(address),
        }
    }

    fn build_query_response(&self, request: &str, body: &str) -> String {
        let device_id = sip_body(request)
            .map(xml_metadata)
            .and_then(|(device_id, _)| device_id)
            .unwrap_or_default();
        let call_id = CallId::with_host(&self.endpoint_host().to_string());
        let branch = format!("z9hG4bK{}", now_millis());
        let from = format!("<sip:{device_id}@{}>", self.domain);
        let to = format!("<sip:{}@{}>", self.platform_id, self.registrar.host);
        format!(
            "MESSAGE {} SIP/2.0\r\nVia: SIP/2.0/UDP {}:{};branch={branch}\r\nFrom: {from}\r\nTo: {to}\r\nCall-ID: {call_id}\r\nCSeq: 1 MESSAGE\r\nContact: <sip:{device_id}@{}:{}>\r\nMax-Forwards: 70\r\nContent-Type: Application/MANSCDP+xml\r\nContent-Length: {}\r\n\r\n{body}",
            self.registrar,
            self.advertised_ip,
            self.local_port,
            self.advertised_ip,
            self.local_port,
            body.len()
        )
    }
}

pub struct DeviceSipSession {
    device_id: String,
    aor: SipUri,
    registrar: SipUri,
    contact: SipUri,
    call_id: CallId,
    from_tag: Tag,
    cseq: AtomicU32,
    nonce_count: AtomicU32,
}

impl DeviceSipSession {
    pub(crate) fn new(
        device_id: String,
        configuration: &SipServiceConfiguration,
        client: &SipRegistrationClient,
    ) -> Result<Self, SipRegistrationError> {
        let aor = SipUri::parse(&format!("sip:{device_id}@{}", configuration.domain))
            .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let registrar_endpoint = SipUri::parse(&configuration.uri)
            .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let registrar = SipUri::parse(&format!(
            "sip:{}@{}:{}",
            configuration.platform_id,
            registrar_endpoint.host,
            registrar_endpoint.port.unwrap_or(5_060)
        ))
        .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let contact = SipUri::parse(&format!(
            "sip:{device_id}@{}:{}",
            client.endpoint_host(),
            client.local_port
        ))
        .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;

        Ok(Self {
            device_id,
            aor,
            registrar,
            contact,
            call_id: CallId::with_host(&client.endpoint_host().to_string()),
            from_tag: Tag::new(),
            cseq: AtomicU32::new(0),
            nonce_count: AtomicU32::new(0),
        })
    }

    pub(crate) async fn register(
        &self,
        client: &SipRegistrationClient,
        configuration: &SipServiceConfiguration,
        cancellation: &CancellationToken,
    ) -> Result<u32, SipRegistrationError> {
        self.perform_register(
            client,
            configuration,
            configuration.register_expires,
            cancellation,
        )
        .await
    }

    pub(crate) async fn unregister(
        &self,
        client: &SipRegistrationClient,
        configuration: &SipServiceConfiguration,
        cancellation: &CancellationToken,
    ) -> Result<(), SipRegistrationError> {
        self.perform_register(client, configuration, 0, cancellation)
            .await
            .map(|_| ())
    }

    pub(crate) async fn send_message(
        &self,
        client: &SipRegistrationClient,
        body: String,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
    ) -> Result<(), SipRegistrationError> {
        let cseq = self.next_cseq();
        client
            .exchange_with_channel(
                &self.device_id,
                self.build_message_request(Method::Message, cseq, body),
                cancellation,
                channel_id,
            )
            .await
            .map(|_| ())
    }

    pub(crate) async fn send_notify(
        &self,
        client: &SipRegistrationClient,
        body: String,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
        subscription: &crate::runtime::SubscriptionSnapshot,
    ) -> Result<(), SipRegistrationError> {
        client
            .exchange_with_channel(
                &self.device_id,
                self.build_notify_request(body, subscription),
                cancellation,
                channel_id,
            )
            .await
            .map(|_| ())
    }

    async fn perform_register(
        &self,
        client: &SipRegistrationClient,
        configuration: &SipServiceConfiguration,
        mut expires: u32,
        cancellation: &CancellationToken,
    ) -> Result<u32, SipRegistrationError> {
        let mut authorization: Option<(HeaderName, AuthHeader)> = None;
        for _ in 0..3 {
            let cseq = self.next_cseq();
            let request = self.build_request(cseq, expires, authorization.as_ref());
            let response = client
                // REGISTER 的 401/407/423 是认证与有效期协商的一部分，不能经过
                // 仅接受 2xx 的业务交换，否则 Digest 重试永远不会执行。
                .exchange_raw(&self.device_id, request, cancellation, None)
                .await?;
            let status = response.status_line.status_code.0;
            match status {
                200..=299 => return Ok(effective_expires(&response, expires)),
                401 | 407 => {
                    let (challenge_name, authorization_name) = if status == 401 {
                        (HeaderName::WwwAuthenticate, HeaderName::Authorization)
                    } else {
                        (
                            HeaderName::ProxyAuthenticate,
                            HeaderName::ProxyAuthorization,
                        )
                    };
                    let mut challenge = response
                        .headers
                        .get(&challenge_name)
                        .and_then(HeaderValue::as_auth)
                        .cloned()
                        .ok_or(SipRegistrationError::MissingChallenge)?;
                    normalize_qop(&mut challenge);
                    let nonce_count = self.nonce_count.fetch_add(1, Ordering::Relaxed) + 1;
                    let auth = build_auth_header(
                        &challenge,
                        &self.device_id,
                        &configuration.password,
                        &self.registrar.to_string(),
                        "REGISTER",
                        nonce_count,
                        &DigestAuthHandler,
                    )
                    .map_err(|error| SipRegistrationError::Authentication(error.to_string()))?;
                    authorization = Some((authorization_name, auth));
                }
                423 if expires > 0 => {
                    expires = header_u32(&response, &HeaderName::MinExpires)
                        .ok_or(SipRegistrationError::MissingMinExpires)?;
                    authorization = None;
                }
                code => {
                    return Err(SipRegistrationError::Rejected {
                        code,
                        reason: response.status_line.reason_phrase,
                    });
                }
            }
        }
        Err(SipRegistrationError::Authentication(
            "平台连续返回认证挑战".to_owned(),
        ))
    }

    fn build_request(
        &self,
        cseq: u32,
        expires: u32,
        authorization: Option<&(HeaderName, AuthHeader)>,
    ) -> SipRequest {
        let mut headers = HeaderCollection::new();
        headers.insert(
            HeaderName::Via,
            HeaderValue::Via(ViaHeader::new(
                TransportProtocol::Udp,
                self.contact.host.clone(),
                self.contact.port,
            )),
        );
        headers.insert(
            HeaderName::From,
            HeaderValue::FromTo(
                FromToHeader::new(self.aor.clone()).with_tag(self.from_tag.clone()),
            ),
        );
        headers.insert(
            HeaderName::To,
            HeaderValue::FromTo(FromToHeader::new(self.aor.clone())),
        );
        headers.insert(
            HeaderName::CallId,
            HeaderValue::CallId(self.call_id.clone()),
        );
        headers.insert(
            HeaderName::CSeq,
            HeaderValue::CSeq(CSeqHeader::new(cseq, Method::Register)),
        );
        headers.insert(
            HeaderName::Contact,
            HeaderValue::Contact(ContactHeader::new(self.contact.clone()).with_expires(expires)),
        );
        headers.insert(HeaderName::Expires, HeaderValue::Expires(expires));
        headers.insert(HeaderName::MaxForwards, HeaderValue::MaxForwards(70));
        headers.insert(
            HeaderName::UserAgent,
            HeaderValue::Raw(format!("GBLab/{}", env!("CARGO_PKG_VERSION"))),
        );
        if let Some((name, value)) = authorization {
            headers.insert(name.clone(), HeaderValue::Auth(value.clone()));
        }
        SipRequest {
            request_line: RequestLine {
                method: Method::Register,
                request_uri: self.registrar.clone(),
                version: SipVersion,
            },
            headers,
            body: None,
        }
    }

    fn build_message_request(&self, method: Method, cseq: u32, body: String) -> SipRequest {
        let mut headers = HeaderCollection::new();
        headers.insert(
            HeaderName::Via,
            HeaderValue::Via(ViaHeader::new(
                TransportProtocol::Udp,
                self.contact.host.clone(),
                self.contact.port,
            )),
        );
        headers.insert(
            HeaderName::From,
            HeaderValue::FromTo(
                FromToHeader::new(self.aor.clone()).with_tag(self.from_tag.clone()),
            ),
        );
        headers.insert(
            HeaderName::To,
            HeaderValue::FromTo(FromToHeader::new(self.registrar.clone())),
        );
        headers.insert(
            HeaderName::CallId,
            HeaderValue::CallId(self.call_id.clone()),
        );
        headers.insert(
            HeaderName::CSeq,
            HeaderValue::CSeq(CSeqHeader::new(cseq, method.clone())),
        );
        headers.insert(HeaderName::MaxForwards, HeaderValue::MaxForwards(70));
        headers.insert(
            HeaderName::ContentType,
            HeaderValue::ContentType("Application/MANSCDP+xml".to_owned()),
        );
        if method == Method::Notify {
            headers.insert(
                HeaderName::Extension("Event".to_owned()),
                HeaderValue::Raw("presence".to_owned()),
            );
            headers.insert(
                HeaderName::Extension("Subscription-State".to_owned()),
                HeaderValue::Raw("active".to_owned()),
            );
        }
        SipRequest {
            request_line: RequestLine {
                method,
                request_uri: self.registrar.clone(),
                version: SipVersion,
            },
            headers,
            body: Some(siprs::siprs_message::Body::new(
                "Application/MANSCDP+xml",
                body.into_bytes(),
            )),
        }
    }

    fn build_notify_request(
        &self,
        body: String,
        subscription: &crate::runtime::SubscriptionSnapshot,
    ) -> SipRequest {
        let mut request =
            self.build_message_request(Method::Notify, subscription.notify_cseq, body);
        if let Some(call_id) = subscription.call_id.as_ref() {
            request.headers.insert(
                HeaderName::CallId,
                HeaderValue::CallId(CallId(call_id.clone())),
            );
        }
        request.headers.insert(
            HeaderName::CSeq,
            HeaderValue::CSeq(CSeqHeader::new(subscription.notify_cseq, Method::Notify)),
        );
        if let Some(tag) = subscription.remote_tag.as_ref() {
            request.headers.insert(
                HeaderName::To,
                HeaderValue::FromTo(
                    FromToHeader::new(self.registrar.clone()).with_tag(Tag(tag.clone())),
                ),
            );
        }
        if let Some(tag) = subscription.local_tag.as_ref() {
            request.headers.insert(
                HeaderName::From,
                HeaderValue::FromTo(FromToHeader::new(self.aor.clone()).with_tag(Tag(tag.clone()))),
            );
        }
        if let Some(event) = subscription.event.as_ref() {
            request.headers.insert(
                HeaderName::Extension("Event".to_owned()),
                HeaderValue::Raw(event.clone()),
            );
        }
        request
    }

    fn next_cseq(&self) -> u32 {
        self.cseq.fetch_add(1, Ordering::Relaxed).saturating_add(1)
    }
}

fn payload_command_type(payload: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(payload);
    sip_body(&text).and_then(xml_command_type)
}

fn request_body_text(request: &SipRequest) -> String {
    request
        .body
        .as_ref()
        .map(|body| String::from_utf8_lossy(&body.content).into_owned())
        .unwrap_or_default()
}

fn structured_header_value(request: &SipRequest, name: &HeaderName) -> Option<String> {
    request.headers.get(name).and_then(|value| match value {
        HeaderValue::Raw(value) | HeaderValue::ContentType(value) => Some(value.clone()),
        HeaderValue::Expires(value) => Some(value.to_string()),
        _ => None,
    })
}

fn header_u32_value(value: &HeaderValue) -> Option<u32> {
    match value {
        HeaderValue::Expires(value) => Some(*value),
        HeaderValue::Raw(value) => value.trim().parse().ok(),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SipResponseClass {
    Provisional,
    Success,
    Redirection,
    ClientFailure,
    ServerFailure,
    GlobalFailure,
}

impl SipResponseClass {
    const fn is_final(self) -> bool {
        !matches!(self, Self::Provisional)
    }
}

const fn response_class(status_code: u16) -> SipResponseClass {
    match status_code {
        100..=199 => SipResponseClass::Provisional,
        200..=299 => SipResponseClass::Success,
        300..=399 => SipResponseClass::Redirection,
        400..=499 => SipResponseClass::ClientFailure,
        500..=599 => SipResponseClass::ServerFailure,
        _ => SipResponseClass::GlobalFailure,
    }
}

fn accept_sip_success(response: SipResponse) -> Result<SipResponse, SipRegistrationError> {
    match response_class(response.status_line.status_code.0) {
        SipResponseClass::Success => Ok(response),
        _ => Err(SipRegistrationError::Rejected {
            code: response.status_line.status_code.0,
            reason: response.status_line.reason_phrase,
        }),
    }
}

fn transaction_key_from_request(request: &SipRequest) -> Option<TransactionKey> {
    let call_id = request
        .headers
        .get(&HeaderName::CallId)
        .and_then(HeaderValue::as_call_id)?
        .0
        .clone();
    let cseq = request
        .headers
        .get(&HeaderName::CSeq)
        .and_then(HeaderValue::as_cseq)?;
    let branch = request
        .headers
        .get(&HeaderName::Via)
        .and_then(HeaderValue::as_via)?
        .branch
        .0
        .clone();
    Some(TransactionKey {
        call_id,
        cseq: cseq.sequence.0,
        method: cseq.method.clone(),
        branch,
    })
}

fn transaction_key_from_response(response: &SipResponse) -> Option<TransactionKey> {
    let call_id = response
        .headers
        .get(&HeaderName::CallId)
        .and_then(HeaderValue::as_call_id)?
        .0
        .clone();
    let cseq = response
        .headers
        .get(&HeaderName::CSeq)
        .and_then(HeaderValue::as_cseq)?;
    let branch = response
        .headers
        .get(&HeaderName::Via)
        .and_then(HeaderValue::as_via)?
        .branch
        .0
        .clone();
    Some(TransactionKey {
        call_id,
        cseq: cseq.sequence.0,
        method: cseq.method.clone(),
        branch,
    })
}

fn request_call_id(request: &SipRequest) -> Option<String> {
    request
        .headers
        .get(&HeaderName::CallId)
        .and_then(HeaderValue::as_call_id)
        .map(|value| value.0.clone())
}

fn request_cseq(request: &SipRequest) -> Option<u32> {
    request
        .headers
        .get(&HeaderName::CSeq)
        .and_then(HeaderValue::as_cseq)
        .map(|value| value.sequence.0)
}

fn request_dialog_id(request: &SipRequest) -> Option<crate::runtime::DialogId> {
    let call_id = request_call_id(request)?;
    let local_tag = request
        .headers
        .get(&HeaderName::To)
        .and_then(HeaderValue::as_from_to)
        .and_then(|header| header.tag.as_ref())
        .map(ToString::to_string)?;
    let remote_tag = request
        .headers
        .get(&HeaderName::From)
        .and_then(HeaderValue::as_from_to)
        .and_then(|header| header.tag.as_ref())
        .map(ToString::to_string)?;
    Some(crate::runtime::DialogId::new(
        call_id, local_tag, remote_tag,
    ))
}

fn extract_header(message: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name.to_ascii_lowercase());
    message
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim().to_owned())
}

fn request_method(payload: &[u8]) -> Option<String> {
    String::from_utf8_lossy(payload)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
}

fn resolve_device_and_channel(
    requested_id: &str,
    devices: &HashMap<String, SimulatedDevice>,
) -> (String, Option<String>) {
    if devices.contains_key(requested_id) {
        return (requested_id.to_owned(), None);
    }
    for (device_id, device) in devices {
        if let Ok(channels) = derive_channels_for_device(device)
            && channels
                .iter()
                .any(|channel| channel.id.to_string() == requested_id)
        {
            return (device_id.clone(), Some(requested_id.to_owned()));
        }
    }
    (requested_id.to_owned(), None)
}

enum InboundRequestDisposition {
    NoResponse,
    Respond { status: u16, reason: &'static str },
    RespondAndQuery { body: String },
}

impl InboundRequestDisposition {
    const fn response(&self) -> Option<(u16, &'static str)> {
        match self {
            Self::NoResponse => None,
            Self::Respond { status, reason } => Some((*status, reason)),
            Self::RespondAndQuery { .. } => Some((200, "OK")),
        }
    }
}

fn dispatch_inbound_request(
    request: &str,
    method: Option<&str>,
    devices: &HashMap<String, SimulatedDevice>,
) -> InboundRequestDisposition {
    match method.unwrap_or_default().to_ascii_uppercase().as_str() {
        "MESSAGE" => dispatch_platform_request(request, devices).map_or(
            InboundRequestDisposition::Respond {
                status: 489,
                reason: "Bad Event",
            },
            |body| InboundRequestDisposition::RespondAndQuery { body },
        ),
        "SUBSCRIBE" | "OPTIONS" => InboundRequestDisposition::Respond {
            status: 200,
            reason: "OK",
        },
        "ACK" => InboundRequestDisposition::NoResponse,
        "BYE" | "CANCEL" => InboundRequestDisposition::Respond {
            status: 481,
            reason: "Call/Transaction Does Not Exist",
        },
        "INVITE" => InboundRequestDisposition::Respond {
            status: 488,
            reason: "Not Acceptable Here",
        },
        "NOTIFY" => InboundRequestDisposition::Respond {
            status: 489,
            reason: "Bad Event",
        },
        _ => InboundRequestDisposition::Respond {
            status: 405,
            reason: "Method Not Allowed",
        },
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "无状态 SIP 响应需要复用请求头和本地传输上下文"
)]
fn build_request_response(
    request: &str,
    status: u16,
    reason: &str,
    local_tag: Option<&str>,
    device_id: &str,
    advertised_ip: IpAddr,
    local_port: u16,
) -> String {
    let mut response = format!("SIP/2.0 {status} {reason}\r\n");
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or_default();
    for name in ["Via", "From", "To", "Call-ID", "CSeq"] {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        if let Some(line) = request
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        {
            let line = if name == "To"
                && status / 100 == 2
                && local_tag.is_some()
                && !line.to_ascii_lowercase().contains(";tag=")
            {
                format!("{};tag={}", line.trim_end(), local_tag.unwrap_or_default())
            } else {
                line.trim_end().to_owned()
            };
            response.push_str(&line);
            response.push_str("\r\n");
        }
    }
    if method == "SUBSCRIBE" {
        if let Some(expires) = extract_header(request, "Expires") {
            let _ = write!(response, "Expires: {expires}\r\n");
        }
        let _ = write!(
            response,
            "Contact: <sip:{device_id}@{advertised_ip}:{local_port}>\r\n"
        );
    }
    if status == 405 {
        response.push_str("Allow: MESSAGE, SUBSCRIBE, OPTIONS, INVITE, ACK, BYE, CANCEL\r\n");
    }
    if method == "OPTIONS" && status / 100 == 2 {
        response.push_str("Allow: MESSAGE, SUBSCRIBE, OPTIONS, INVITE, ACK, BYE, CANCEL\r\n");
        response.push_str("Supported: 100rel, timer\r\n");
    }
    response.push_str("Content-Length: 0\r\n\r\n");
    response
}

fn dispatch_platform_request(
    request: &str,
    devices: &HashMap<String, SimulatedDevice>,
) -> Option<String> {
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())?;
    if method != "MESSAGE" {
        return None;
    }
    let body = sip_body(request)?;
    match parse_xml(body).ok()? {
        GbMessage::Query(query) => match query.cmd_type {
            CmdType::Catalog => Some(build_catalog_body(&query, devices)),
            CmdType::DeviceInfo => Some(build_device_info_body(
                query.device_id.as_str(),
                &query.sn.to_string(),
                devices,
            )),
            CmdType::DeviceStatus => {
                Some(build_device_status_body(query.device_id.as_str(), query.sn))
            }
            CmdType::RecordQuery => Some(build_record_info_body(
                query.device_id.as_str(),
                &query.sn.to_string(),
            )),
            CmdType::Other(value) if value == "RecordInfo" => Some(build_record_info_body(
                query.device_id.as_str(),
                &query.sn.to_string(),
            )),
            _ => None,
        },
        GbMessage::Control(control) if control.cmd_type == CmdType::DeviceControl => Some(
            build_device_control_body(control.device_id.as_str(), &control.sn.to_string()),
        ),
        _ => None,
    }
}

fn build_device_info_body(
    device_id: &str,
    sn: &str,
    devices: &HashMap<String, SimulatedDevice>,
) -> String {
    let Some(device) = devices.get(device_id) else {
        return format!(
            "<Response><CmdType>DeviceInfo</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Result>ERROR</Result></Response>"
        );
    };
    format!(
        "<Response><CmdType>DeviceInfo</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><DeviceName>{}</DeviceName><Manufacturer>{}</Manufacturer><Model>{}</Model><Firmware>{}</Firmware><Result>OK</Result></Response>",
        xml_escape(&device.name),
        xml_escape(&device.manufacturer),
        xml_escape(&device.model),
        xml_escape(&device.firmware_version)
    )
}

fn build_device_status_body(device_id: &str, sn: u32) -> String {
    let Ok(device_id) = siprs::siprs_gb28181_codec::DeviceId::parse(device_id) else {
        return format!(
            "<Response><CmdType>DeviceStatus</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Result>ERROR</Result></Response>"
        );
    };
    Response::device_status(sn, device_id, DeviceStatusInfo::new(true, "OK")).to_xml()
}

fn build_device_control_body(device_id: &str, sn: &str) -> String {
    format!(
        "<Response><CmdType>DeviceControl</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Result>OK</Result></Response>"
    )
}

fn build_record_info_body(device_id: &str, sn: &str) -> String {
    format!(
        "<Response><CmdType>RecordInfo</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>0</SumNum><RecordList Num=\"0\"></RecordList></Response>"
    )
}

fn build_catalog_body(query: &Query, devices: &HashMap<String, SimulatedDevice>) -> String {
    let device_id = query.device_id.as_str();
    let Some(device) = devices.get(device_id) else {
        return Response::catalog(query.sn, query.device_id.clone(), 0, Vec::new()).to_xml();
    };
    let mut items = Vec::with_capacity(device.channel_count as usize + 1);
    let mut root = DeviceItem::new(query.device_id.clone());
    root.name = Some(device.name.clone());
    root.manufacturer = Some(device.manufacturer.clone());
    root.model = Some(device.model.clone());
    root.parent_id = Some(device.id.to_string());
    root.status = Some("ON".to_owned());
    items.push(root);
    if let Ok(channels) = derive_channels_for_device(device) {
        for channel in channels {
            let Ok(channel_id) = siprs::siprs_gb28181_codec::DeviceId::parse(channel.id.as_str())
            else {
                continue;
            };
            let mut item = DeviceItem::new(channel_id);
            item.name = Some(channel.name);
            item.manufacturer = Some(device.manufacturer.clone());
            item.model = Some(device.model.clone());
            item.parent_id = Some(device.id.to_string());
            item.status = Some("ON".to_owned());
            items.push(item);
        }
    }
    Response::catalog(
        query.sn,
        query.device_id.clone(),
        u32::try_from(items.len()).unwrap_or(u32::MAX),
        items,
    )
    .to_xml()
}

fn sip_body(request: &str) -> Option<&str> {
    request
        .split_once("\r\n\r\n")
        .or_else(|| request.split_once("\n\n"))
        .map(|(_, body)| body.trim())
        .filter(|body| !body.is_empty())
}

fn xml_metadata(xml: &str) -> (Option<String>, Option<String>) {
    let Ok(message) = parse_xml(xml) else {
        return (None, None);
    };
    match message {
        GbMessage::Query(query) => (
            Some(query.device_id.to_string()),
            Some(query.cmd_type.to_string()),
        ),
        GbMessage::Response(response) => (
            Some(response.device_id.to_string()),
            Some(response.cmd_type.to_string()),
        ),
        GbMessage::Control(control) => (
            Some(control.device_id.to_string()),
            Some(control.cmd_type.to_string()),
        ),
        GbMessage::Notify(notify) => (
            Some(notify.device_id.to_string()),
            Some(notify.cmd_type.to_string()),
        ),
        GbMessage::CascadingRegister(_) => (None, None),
    }
}

fn xml_command_type(xml: &str) -> Option<String> {
    xml_metadata(xml).1
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn effective_expires(response: &SipResponse, fallback: u32) -> u32 {
    response
        .headers
        .get(&HeaderName::Contact)
        .and_then(HeaderValue::as_contact)
        .and_then(|contact| contact.expires)
        .or_else(|| header_u32(response, &HeaderName::Expires))
        .unwrap_or(fallback)
}

fn header_u32(response: &SipResponse, name: &HeaderName) -> Option<u32> {
    match response.headers.get(name) {
        Some(HeaderValue::Expires(value)) => Some(*value),
        Some(HeaderValue::Raw(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn normalize_qop(challenge: &mut AuthHeader) {
    let Some(qop) = challenge.qop.as_deref() else {
        return;
    };
    if qop
        .split(',')
        .any(|value| value.trim().eq_ignore_ascii_case("auth"))
    {
        challenge.qop = Some("auth".to_owned());
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use siprs::{
        siprs_core::{SipVersion, StatusCode},
        siprs_message::{HeaderCollection, MessageParser, SipMessage, SipResponse, StatusLine},
    };

    use super::{
        InboundRequestDisposition, SipResponseClass, accept_sip_success, dispatch_inbound_request,
        response_class, transaction_key_from_request,
    };

    #[test]
    fn response_class_should_distinguish_provisional_success_and_failures() {
        assert_eq!(response_class(180), SipResponseClass::Provisional);
        assert_eq!(response_class(200), SipResponseClass::Success);
        assert_eq!(response_class(302), SipResponseClass::Redirection);
        assert_eq!(response_class(401), SipResponseClass::ClientFailure);
        assert_eq!(response_class(503), SipResponseClass::ServerFailure);
        assert_eq!(response_class(699), SipResponseClass::GlobalFailure);
    }

    #[test]
    fn business_exchange_should_accept_only_2xx_responses() {
        let response = |code: StatusCode, reason: &str| SipResponse {
            status_line: StatusLine {
                version: SipVersion,
                status_code: code,
                reason_phrase: reason.to_owned(),
            },
            headers: HeaderCollection::new(),
            body: None,
        };

        assert!(accept_sip_success(response(StatusCode::OK, "OK")).is_ok());
        assert!(accept_sip_success(response(StatusCode::UNAUTHORIZED, "Unauthorized")).is_err());
        assert!(accept_sip_success(response(StatusCode(500), "Error")).is_err());
    }

    #[test]
    fn dispatcher_should_route_supported_and_unsupported_methods_explicitly() {
        let devices = HashMap::new();
        let options =
            dispatch_inbound_request("OPTIONS sip:p SIP/2.0\r\n", Some("OPTIONS"), &devices);
        assert!(matches!(
            options,
            InboundRequestDisposition::Respond { status: 200, .. }
        ));

        let ack = dispatch_inbound_request("ACK sip:p SIP/2.0\r\n", Some("ACK"), &devices);
        assert!(matches!(ack, InboundRequestDisposition::NoResponse));

        let invite = dispatch_inbound_request("INVITE sip:p SIP/2.0\r\n", Some("INVITE"), &devices);
        assert!(matches!(
            invite,
            InboundRequestDisposition::Respond { status: 488, .. }
        ));

        let bye = dispatch_inbound_request("BYE sip:p SIP/2.0\r\n", Some("BYE"), &devices);
        assert!(matches!(
            bye,
            InboundRequestDisposition::Respond { status: 481, .. }
        ));

        let unknown =
            dispatch_inbound_request("PUBLISH sip:p SIP/2.0\r\n", Some("PUBLISH"), &devices);
        assert!(matches!(
            unknown,
            InboundRequestDisposition::Respond { status: 405, .. }
        ));
    }

    #[test]
    fn dispatcher_should_reject_unknown_message_command_without_pseudo_success() {
        let devices = HashMap::new();
        let request =
            "MESSAGE sip:p SIP/2.0\r\n\r\n<Notify><CmdType>Unsupported</CmdType></Notify>";
        let result = dispatch_inbound_request(request, Some("MESSAGE"), &devices);
        assert!(matches!(
            result,
            InboundRequestDisposition::Respond { status: 489, .. }
        ));
    }

    #[test]
    fn transaction_key_should_include_call_id_cseq_method_and_branch() {
        let raw = b"MESSAGE sip:p SIP/2.0\r\n\
                    Via: SIP/2.0/UDP 127.0.0.1:5060;branch=z9hG4bK-test\r\n\
                    Call-ID: call-1\r\n\
                    CSeq: 42 MESSAGE\r\n\
                    Content-Length: 0\r\n\r\n";
        let parser = MessageParser::new(65_536);
        let Some(SipMessage::Request(request)) = parser.parse(raw).ok() else {
            return;
        };
        let Some(key) = transaction_key_from_request(&request) else {
            return;
        };
        assert_eq!(key.call_id, "call-1");
        assert_eq!(key.cseq, 42);
        assert_eq!(key.method.to_string(), "MESSAGE");
        assert_eq!(key.branch, "z9hG4bK-test");
    }
}
