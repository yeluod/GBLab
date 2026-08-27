//! 单设备 SIP 会话与 REGISTER/Digest 协商。
//!
//! 会话只维护设备级标识和递增序号；实际 UDP 事务等待由 client/outbound 层负责。

use std::sync::atomic::{AtomicU32, Ordering};

use siprs::{
    siprs_core::{SipVersion, TransportProtocol},
    siprs_message::{
        AuthHeader, CSeqHeader, CallId, ContactHeader, FromToHeader, HeaderCollection, HeaderName,
        HeaderValue, Method, RequestLine, SipRequest, SipResponse, SipUri, Tag, ViaHeader,
    },
    siprs_registration::{DigestAuthHandler, auth::build_auth_header},
};
use tokio_util::sync::CancellationToken;

use crate::configuration::SipServiceConfiguration;

use super::{
    notify::NotifyDialogContext,
    registration::{SipRegistrationClient, SipRegistrationError},
};

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

pub struct DeviceSipSession {
    device_id: String,
    aor: SipUri,
    registrar: SipUri,
    contact: SipUri,
    call_id: CallId,
    from_tag: Tag,
    cseq: AtomicU32,
    nonce_count: AtomicU32,
    sn: AtomicU32,
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
            client.local_port()
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
            sn: AtomicU32::new(0),
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
        subscription: &NotifyDialogContext,
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
        let mut last_nonce: Option<String> = None;
        for _ in 0..8 {
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
                    if challenge.nonce != last_nonce {
                        self.nonce_count.store(0, Ordering::Relaxed);
                        last_nonce.clone_from(&challenge.nonce);
                    }
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

    fn build_notify_request(&self, body: String, subscription: &NotifyDialogContext) -> SipRequest {
        let mut request = self.build_message_request(Method::Notify, subscription.cseq, body);
        if let Some(call_id) = subscription.call_id.as_ref() {
            request.headers.insert(
                HeaderName::CallId,
                HeaderValue::CallId(CallId(call_id.clone())),
            );
        }
        request.headers.insert(
            HeaderName::CSeq,
            HeaderValue::CSeq(CSeqHeader::new(subscription.cseq, Method::Notify)),
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

    pub(crate) fn next_sn(&self) -> u32 {
        let next = self.sn.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        if next == 0 {
            self.sn.store(1, Ordering::Relaxed);
            1
        } else {
            next
        }
    }
}
