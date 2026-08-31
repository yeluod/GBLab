use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};

use siprs::siprs_message::SipUri;
use thiserror::Error;
use tokio::{
    net::{UdpSocket, lookup_host},
    sync::{Mutex, mpsc},
    time::Instant,
};

use crate::{
    configuration::{SignalCharset, SipServiceConfiguration, SipTransport},
    sip::transaction::{TransactionKey, TransactionManager},
};

#[cfg(test)]
use super::dispatcher::{
    InboundRequestDisposition, SipResponseClass, accept_sip_success, dispatch_inbound_request,
    invite_key_from_cancel, response_class, transaction_key_from_request,
};

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
    #[error("SIP XML 字符集处理失败: {0}")]
    Charset(String),
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
}

pub struct SipRegistrationClient {
    pub(super) socket: Arc<UdpSocket>,
    pub(super) advertised_ip: IpAddr,
    pub(super) local_port: u16,
    pub(super) registrar: SipUri,
    pub(super) domain: String,
    pub(super) uas_tags: Mutex<HashMap<String, UasDialogTag>>,
    pub(super) transactions: TransactionManager,
    pub(super) event_tx: mpsc::Sender<SipTransportEvent>,
    pub(super) invite_transactions: Mutex<HashMap<TransactionKey, InviteServerTransaction>>,
    pub(super) server_transactions: Mutex<HashMap<TransactionKey, CachedServerResponse>>,
    pub(super) query_cseq: AtomicU32,
    pub(super) signal_charset: SignalCharset,
    dropped_events: AtomicU64,
    pub(super) query_executor: Arc<tokio::sync::Semaphore>,
}

pub(super) struct CachedServerResponse {
    pub(super) response: Vec<u8>,
    pub(super) expires_at: Instant,
}

pub(super) struct UasDialogTag {
    pub(super) value: String,
    pub(super) expires_at: Instant,
}

impl UasDialogTag {
    pub(super) fn is_active(&self, now: Instant) -> bool {
        self.expires_at > now
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InviteServerTransactionState {
    Proceeding,
    Completed,
    Terminated,
}

pub(super) struct InviteServerTransaction {
    pub(super) state: InviteServerTransactionState,
    pub(super) expires_at: Instant,
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
            uas_tags: Mutex::new(HashMap::new()),
            transactions: TransactionManager::default(),
            event_tx,
            invite_transactions: Mutex::new(HashMap::new()),
            server_transactions: Mutex::new(HashMap::new()),
            query_cseq: AtomicU32::new(0),
            signal_charset: configuration.signal_charset,
            dropped_events: AtomicU64::new(0),
            query_executor: Arc::new(tokio::sync::Semaphore::new(64)),
        }))
    }

    pub(super) fn emit_event(&self, event: SipTransportEvent) {
        if self.event_tx.try_send(event).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use siprs::{
        siprs_core::{SipVersion, StatusCode},
        siprs_message::{HeaderCollection, MessageParser, SipMessage, SipResponse, StatusLine},
    };

    use tokio::time::Instant;

    use super::{
        InboundRequestDisposition, SipResponseClass, UasDialogTag, accept_sip_success,
        dispatch_inbound_request, invite_key_from_cancel, response_class,
        transaction_key_from_request,
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
    fn subscribe_dialog_tag_should_expire_naturally() {
        let now = Instant::now();
        let active = UasDialogTag {
            value: "tag".to_owned(),
            expires_at: now + Duration::from_secs(1),
        };
        let expired = UasDialogTag {
            value: "tag".to_owned(),
            expires_at: now,
        };

        assert!(active.is_active(now));
        assert!(!expired.is_active(now));
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
            InboundRequestDisposition::Respond { status: 400, .. }
        ));
    }

    #[test]
    fn dispatcher_should_accept_platform_gb2312_xml_declaration() {
        let devices = HashMap::new();
        let request = "MESSAGE sip:34020000002000000100 SIP/2.0\r\n\
                       Content-Type: Application/MANSCDP+xml;charset=GB2312\r\n\r\n\
                       <?xml version=\"1.0\" encoding=\"GB2312\" standalone=\"yes\"?>\n\
                       <Query><CmdType>DeviceInfo</CmdType><SN>3152</SN>\
                       <DeviceID>34020000002000000100</DeviceID></Query>";

        let result = dispatch_inbound_request(request, Some("MESSAGE"), &devices);

        assert!(matches!(
            result,
            InboundRequestDisposition::RespondAndQuery { .. }
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

    #[test]
    fn cancel_should_map_to_exact_invite_transaction_key() {
        let raw = b"CANCEL sip:p SIP/2.0\r\n\
                    Via: SIP/2.0/UDP 127.0.0.1:5060;branch=z9hG4bK-cancel\r\n\
                    Call-ID: call-1\r\n\
                    CSeq: 42 CANCEL\r\n\
                    Content-Length: 0\r\n\r\n";
        let parser = MessageParser::new(65_536);
        let Some(SipMessage::Request(request)) = parser.parse(raw).ok() else {
            return;
        };
        let Some(key) = invite_key_from_cancel(&request) else {
            return;
        };
        assert_eq!(key.call_id, "call-1");
        assert_eq!(key.cseq, 42);
        assert_eq!(key.method.to_string(), "INVITE");
        assert_eq!(key.branch, "z9hG4bK-cancel");
    }
}
