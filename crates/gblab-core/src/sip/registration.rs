use std::{
    collections::HashMap,
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

use crate::configuration::{SipServiceConfiguration, SipTransport};

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
    sender: oneshot::Sender<SipResponse>,
}

pub struct SipRegistrationClient {
    socket: Arc<UdpSocket>,
    advertised_ip: IpAddr,
    local_port: u16,
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

        let registrar = SipUri::parse(&configuration.uri)
            .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let host = registrar.host.as_str();
        let port = registrar.port.unwrap_or(5_060);
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
            pending: Mutex::new(HashMap::new()),
            event_tx,
        }))
    }

    pub(crate) async fn receive_loop(self: Arc<Self>, cancellation: CancellationToken) {
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
            let Ok(SipMessage::Response(response)) = parser.parse(raw) else {
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
            let (device_id, sender) = {
                let mut pending = self.pending.lock().await;
                let device_id = pending.get(&call_id).map(|entry| entry.device_id.clone());
                let sender = is_final.then(|| pending.remove(&call_id)).flatten();
                drop(pending);
                (device_id, sender)
            };
            if let Some(device_id) = device_id {
                let _ = self
                    .event_tx
                    .send(SipTransportEvent {
                        timestamp_millis: now_millis(),
                        device_id,
                        direction: SipLogDirection::Receive,
                        message: String::from_utf8_lossy(raw).into_owned(),
                    })
                    .await;
            }
            if let Some(pending) = sender {
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
        let call_id = request
            .headers
            .get(&HeaderName::CallId)
            .and_then(HeaderValue::as_call_id)
            .map(|value| value.0.clone())
            .ok_or_else(|| SipRegistrationError::Build("REGISTER 缺少 Call-ID".to_owned()))?;
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
