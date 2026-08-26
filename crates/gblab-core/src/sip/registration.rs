use std::{
    collections::HashMap,
    fmt::Write,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime},
};

use siprs::{
    siprs_core::{Host, SipVersion, TransportProtocol},
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

struct PendingResponse {
    device_id: String,
    channel_id: Option<String>,
    method: Option<String>,
    command_type: Option<String>,
    sender: oneshot::Sender<SipResponse>,
}

pub struct SipRegistrationClient {
    socket: Arc<UdpSocket>,
    advertised_ip: IpAddr,
    local_port: u16,
    registrar: SipUri,
    domain: String,
    platform_id: String,
    subscription_tags: Mutex<HashMap<String, String>>,
    pending: Mutex<HashMap<String, PendingResponse>>,
    event_tx: mpsc::Sender<SipTransportEvent>,
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
            pending: Mutex::new(HashMap::new()),
            event_tx,
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
                let text = String::from_utf8_lossy(raw).into_owned();
                let requested_id = extract_xml_value(&text, "DeviceID").unwrap_or_default();
                let (device_id, channel_id) =
                    resolve_device_and_channel(&requested_id, &catalog_devices);
                let method = text
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().next())
                    .map(str::to_owned);
                let is_subscribe = method.as_deref() == Some("SUBSCRIBE");
                let command_type = extract_xml_value(&text, "CmdType");
                let event = extract_header(&text, "Event");
                let from_tag = extract_header_param(&text, "From", "tag");
                let request_uri = extract_request_uri(&text);
                let call_id = extract_header(&text, "Call-ID");
                let expires = extract_header(&text, "Expires").and_then(|value| value.parse().ok());
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
                        method,
                        command_type,
                        event,
                        local_tag: local_tag.clone(),
                        from_tag,
                        request_uri: request_uri.clone(),
                        call_id: call_id.clone(),
                        expires,
                    })
                    .await;
                let response = build_stateless_ok_response(
                    &text,
                    local_tag.as_deref(),
                    &device_id,
                    self.advertised_ip,
                    self.local_port,
                );
                if !response.is_empty() {
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
                if let Some(body) = dispatch_platform_request(&text, &catalog_devices) {
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
                            command_type: extract_xml_value(&body, "CmdType"),
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
            let is_final = response.status_line.status_code.0 >= 200;
            let pending_context = {
                let mut pending = self.pending.lock().await;
                let context = pending.get(&call_id).map(|entry| {
                    (
                        entry.device_id.clone(),
                        entry.channel_id.clone(),
                        entry.method.clone(),
                        entry.command_type.clone(),
                    )
                });
                let sender = is_final.then(|| pending.remove(&call_id)).flatten();
                drop(pending);
                (context, sender)
            };
            if let Some((device_id, channel_id, method, command_type)) = pending_context.0 {
                let _ = self
                    .event_tx
                    .send(SipTransportEvent {
                        timestamp_millis: now_millis(),
                        device_id,
                        direction: SipLogDirection::Receive,
                        message: String::from_utf8_lossy(raw).into_owned(),
                        channel_id,
                        is_request: false,
                        method,
                        command_type,
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
                let requested_id = extract_header(&response_text, "To")
                    .or_else(|| extract_header(&response_text, "From"))
                    .and_then(|value| extract_sip_uri_user(&value));
                let (device_id, channel_id) = requested_id
                    .as_deref()
                    .map(|value| resolve_device_and_channel(value, &catalog_devices))
                    .unwrap_or_default();
                let method = extract_header(&response_text, "CSeq")
                    .and_then(|value| value.split_whitespace().nth(1).map(str::to_owned));
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
            if let Some(pending) = pending_context.1 {
                let _ = pending.sender.send(response);
            }
        }
    }

    async fn exchange(
        &self,
        device_id: &str,
        request: SipRequest,
        cancellation: &CancellationToken,
    ) -> Result<SipResponse, SipRegistrationError> {
        self.exchange_with_channel(device_id, request, cancellation, None)
            .await
    }

    async fn exchange_with_channel(
        &self,
        device_id: &str,
        request: SipRequest,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
    ) -> Result<SipResponse, SipRegistrationError> {
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
        {
            let mut pending = self.pending.lock().await;
            pending.insert(
                call_id.clone(),
                PendingResponse {
                    device_id: device_id.to_owned(),
                    channel_id: channel_id.clone(),
                    method: request_method(&payload),
                    command_type: extract_xml_value(&String::from_utf8_lossy(&payload), "CmdType"),
                    sender,
                },
            );
        }

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
                    is_request: false,
                    method: request_method(&payload),
                    command_type: extract_xml_value(&String::from_utf8_lossy(&payload), "CmdType"),
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
                        break Err(SipRegistrationError::Timeout);
                    }
                    delay = delay.saturating_mul(2);
                }
            }
        };

        self.pending.lock().await.remove(&call_id);
        result
    }

    const fn endpoint_host(&self) -> Host {
        match self.advertised_ip {
            IpAddr::V4(address) => Host::IPv4(address),
            IpAddr::V6(address) => Host::IPv6(address),
        }
    }

    fn build_query_response(&self, request: &str, body: &str) -> String {
        let device_id = extract_xml_value(request, "DeviceID").unwrap_or_default();
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
    cseq: u32,
    nonce_count: u32,
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
            cseq: 0,
            nonce_count: 0,
        })
    }

    pub(crate) async fn register(
        &mut self,
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
        &mut self,
        client: &SipRegistrationClient,
        configuration: &SipServiceConfiguration,
        cancellation: &CancellationToken,
    ) -> Result<(), SipRegistrationError> {
        self.perform_register(client, configuration, 0, cancellation)
            .await
            .map(|_| ())
    }

    pub(crate) async fn send_message(
        &mut self,
        client: &SipRegistrationClient,
        body: String,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
    ) -> Result<(), SipRegistrationError> {
        self.cseq = self.cseq.saturating_add(1);
        client
            .exchange_with_channel(
                &self.device_id,
                self.build_message_request(Method::Message, body),
                cancellation,
                channel_id,
            )
            .await
            .map(|_| ())
    }

    pub(crate) async fn send_notify(
        &mut self,
        client: &SipRegistrationClient,
        body: String,
        cancellation: &CancellationToken,
        channel_id: Option<String>,
        subscription: &crate::runtime::SubscriptionSnapshot,
    ) -> Result<(), SipRegistrationError> {
        self.cseq = self.cseq.saturating_add(1);
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
        &mut self,
        client: &SipRegistrationClient,
        configuration: &SipServiceConfiguration,
        mut expires: u32,
        cancellation: &CancellationToken,
    ) -> Result<u32, SipRegistrationError> {
        let mut authorization: Option<(HeaderName, AuthHeader)> = None;
        for _ in 0..3 {
            self.cseq = self.cseq.saturating_add(1);
            let request = self.build_request(expires, authorization.as_ref());
            let response = client
                .exchange(&self.device_id, request, cancellation)
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
                    self.nonce_count = self.nonce_count.saturating_add(1);
                    let auth = build_auth_header(
                        &challenge,
                        &self.device_id,
                        &configuration.password,
                        &self.registrar.to_string(),
                        "REGISTER",
                        self.nonce_count,
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
            HeaderValue::CSeq(CSeqHeader::new(self.cseq, Method::Register)),
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

    fn build_message_request(&self, method: Method, body: String) -> SipRequest {
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
            HeaderValue::CSeq(CSeqHeader::new(self.cseq, method.clone())),
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
        let mut request = self.build_message_request(Method::Notify, body);
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
}

fn extract_xml_value(message: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = message.find(&open)? + open.len();
    let end = message[start..].find(&close)? + start;
    Some(message[start..end].trim().to_owned())
}

fn extract_header(message: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name.to_ascii_lowercase());
    message
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim().to_owned())
}

fn extract_header_param(message: &str, name: &str, parameter: &str) -> Option<String> {
    let value = extract_header(message, name)?;
    let marker = format!("{parameter}=");
    let start = value.to_ascii_lowercase().find(&marker)? + marker.len();
    let remaining = &value[start..];
    let end = remaining
        .find(';')
        .or_else(|| remaining.find('>'))
        .unwrap_or(remaining.len());
    Some(remaining[..end].trim_matches('"').to_owned())
}

fn extract_request_uri(message: &str) -> Option<String> {
    message
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_owned)
}

fn extract_sip_uri_user(value: &str) -> Option<String> {
    let start = value.to_ascii_lowercase().find("sip:")? + 4;
    let remaining = &value[start..];
    let end = remaining
        .find('@')
        .or_else(|| remaining.find('>'))
        .or_else(|| remaining.find(';'))
        .unwrap_or(remaining.len());
    let user = remaining[..end].trim();
    (!user.is_empty()).then(|| user.to_owned())
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

fn build_stateless_ok_response(
    request: &str,
    local_tag: Option<&str>,
    device_id: &str,
    advertised_ip: IpAddr,
    local_port: u16,
) -> String {
    let mut response = String::from("SIP/2.0 200 OK\r\n");
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
    response.push_str("Content-Length: 0\r\n\r\n");
    response
}

fn build_catalog_body(
    device_id: &str,
    sn: &str,
    devices: &HashMap<String, SimulatedDevice>,
) -> String {
    let Some(device) = devices.get(device_id) else {
        return format!(
            "<Response><CmdType>Catalog</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>0</SumNum><DeviceList Num=\"0\"></DeviceList></Response>"
        );
    };
    let channels = derive_channels_for_device(device).unwrap_or_default();
    let mut items = format!(
        "<Device><DeviceID>{}</DeviceID><Name>{}</Name><Manufacturer>{}</Manufacturer><Model>{}</Model><Status>ON</Status><ParentID>{}</ParentID></Device>",
        xml_escape(&device.id.to_string()),
        xml_escape(&device.name),
        xml_escape(&device.manufacturer),
        xml_escape(&device.model),
        xml_escape(&device.id.to_string())
    );
    for channel in &channels {
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
        "<Response><CmdType>Catalog</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>{}</SumNum><DeviceList Num=\"{}\">{items}</DeviceList></Response>",
        channels.len() + 1,
        channels.len() + 1
    )
}

fn dispatch_platform_request(
    request: &str,
    devices: &HashMap<String, SimulatedDevice>,
) -> Option<String> {
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())?;
    let command_type = extract_xml_value(request, "CmdType")?;
    if method != "MESSAGE" {
        return None;
    }
    let device_id = extract_xml_value(request, "DeviceID").unwrap_or_default();
    let sn = extract_xml_value(request, "SN").unwrap_or_else(|| "1".to_owned());
    match command_type.as_str() {
        "Catalog" => Some(build_catalog_body(&device_id, &sn, devices)),
        "DeviceInfo" => Some(build_device_info_body(&device_id, &sn, devices)),
        "DeviceStatus" => Some(build_device_status_body(&device_id, &sn)),
        "DeviceControl" => Some(build_device_control_body(&device_id, &sn)),
        "RecordInfo" => Some(build_record_info_body(&device_id, &sn)),
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

fn build_device_status_body(device_id: &str, sn: &str) -> String {
    format!(
        "<Response><CmdType>DeviceStatus</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Online>ON</Online><Status>OK</Status><Result>OK</Result></Response>"
    )
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
