//! 共享 UDP 传输、入站 Request 处理与服务端事务缓存。

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use siprs::{
    siprs_core::{Host, SipVersion, TransportProtocol},
    siprs_message::{
        CSeqHeader, CallId, ContactHeader, FromToHeader, HeaderCollection, HeaderName, HeaderValue,
        MessageBuilder, MessageParser, Method, RequestLine, SipMessage, SipRequest, SipResponse,
        SipUri, Tag, ViaHeader,
    },
};
use tokio::{
    sync::oneshot,
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;

use super::sdp::{SdpAnswer, SdpOffer};
use crate::SimulatedDevice;
use crate::media::MediaSessionCoordinator;

use super::{
    charset::{encode_xml, prepare_inbound_sip_message, sip_message_for_display},
    dispatcher::{
        InboundRequestDisposition, SIP_MESSAGE_LIMIT, accept_sip_success, build_channel_index,
        build_request_response, dispatch_inbound_request, extract_header, header_u32_value,
        invite_key_from_cancel, payload_command_type, request_body_text, request_call_id,
        request_method, resolve_device_and_channel, response_class, structured_header_value,
        transaction_key_from_request, transaction_key_from_response, xml_metadata,
    },
    registration::{
        CachedServerResponse, InviteServerTransaction, InviteServerTransactionState,
        SipLogDirection, SipRegistrationClient, SipRegistrationError, SipTransportEvent,
        UasDialogTag,
    },
    time::now_millis,
    transaction::{TransactionContext, TransactionKey, TransactionState},
};

const SERVER_TRANSACTION_TTL: Duration = Duration::from_secs(32);
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(8);
const FIRST_RETRANSMIT_DELAY: Duration = Duration::from_millis(500);
const MAX_RETRANSMISSIONS: u8 = 4;

impl SipRegistrationClient {
    pub(crate) async fn clear_server_transactions(&self) {
        self.server_transactions.lock().await.clear();
        self.invite_transactions.lock().await.clear();
        self.invite_dialog_tags.lock().await.clear();
    }

    #[expect(
        clippy::too_many_lines,
        reason = "共享接收循环必须完成解析、响应和事件投影"
    )]
    pub(crate) async fn receive_loop(
        self: Arc<Self>,
        cancellation: CancellationToken,
        catalog_devices: Arc<HashMap<String, SimulatedDevice>>,
        media: Option<MediaSessionCoordinator>,
    ) {
        let parser = MessageParser::new(SIP_MESSAGE_LIMIT);
        let mut buffer = vec![0_u8; SIP_MESSAGE_LIMIT];
        let channel_index = build_channel_index(&catalog_devices);
        loop {
            let received = tokio::select! {
                () = cancellation.cancelled() => break,
                received = self.socket.recv(&mut buffer) => received,
            };
            let Ok(size) = received else {
                break;
            };
            let raw = &buffer[..size];
            let Ok(prepared) = prepare_inbound_sip_message(raw, self.signal_charset) else {
                continue;
            };
            let Ok(message) = parser.parse(&prepared.parser_bytes) else {
                continue;
            };
            let Ok(parse_text) = std::str::from_utf8(&prepared.parser_bytes) else {
                continue;
            };
            let display_text = prepared.display_text;
            let SipMessage::Response(response) = message else {
                let SipMessage::Request(request) = message else {
                    continue;
                };
                let server_key = transaction_key_from_request(&request);
                let now = Instant::now();
                self.invite_transactions
                    .lock()
                    .await
                    .retain(|_, transaction| transaction.expires_at > now);
                {
                    let mut transactions = self.server_transactions.lock().await;
                    transactions.retain(|_, cached| cached.expires_at > now);
                    if let Some(key) = server_key.as_ref()
                        && let Some(cached) = transactions.get(key)
                    {
                        let payload = cached.response.clone();
                        drop(transactions);
                        let _ = self.socket.send(&payload).await;
                        self.emit_event(SipTransportEvent {
                            timestamp_millis: now_millis(),
                            device_id: String::new(),
                            direction: SipLogDirection::Send,
                            message: sip_message_for_display(&payload, self.signal_charset),
                            channel_id: None,
                            is_request: false,
                            method: None,
                            command_type: None,
                            event: None,
                            local_tag: None,
                            from_tag: None,
                            request_uri: None,
                            call_id: request_call_id(&request),
                            expires: None,
                        });
                        continue;
                    }
                }
                let body = request_body_text(&request);
                let (requested_id, parsed_command_type) = xml_metadata(&body);
                let requested_id = requested_id.unwrap_or_default();
                let (device_id, channel_id) =
                    resolve_device_and_channel(&requested_id, &catalog_devices, &channel_index);
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
                let request_to_tag = request
                    .headers
                    .get(&HeaderName::To)
                    .and_then(HeaderValue::as_from_to)
                    .and_then(|header| header.tag.as_ref())
                    .map(ToString::to_string);
                let expires = request
                    .headers
                    .get(&HeaderName::Expires)
                    .and_then(header_u32_value);
                {
                    let mut tags = self.uas_tags.lock().await;
                    tags.retain(|_, dialog| dialog.is_active(now));
                }
                let local_tag = if method.as_deref() == Some("ACK") {
                    None
                } else if let Some(call_id) = call_id.as_ref() {
                    if is_subscribe {
                        let mut tags = self.uas_tags.lock().await;
                        let lifetime = Duration::from_secs(u64::from(expires.unwrap_or(3_600)));
                        // 刷新订阅属于同一对话，并携带上次 200 响应中的 UAS To-tag。
                        // 优先复用该 Tag，确保后续 NOTIFY 仍属于平台已有对话。
                        let value = select_uas_dialog_tag(
                            request_to_tag,
                            tags.get(call_id.as_str())
                                .map(|dialog| dialog.value.as_str()),
                        );
                        let dialog = tags.entry(call_id.clone()).or_insert_with(|| UasDialogTag {
                            value: value.clone(),
                            expires_at: now + lifetime,
                        });
                        dialog.value = value;
                        dialog.expires_at = now + lifetime;
                        let value = dialog.value.clone();
                        drop(tags);
                        Some(value)
                    } else {
                        // Non-dialog requests only need a tag for this response;
                        // retaining every transient Call-ID would create an
                        // unbounded map under MESSAGE/OPTIONS traffic.
                        Some(Tag::new().to_string())
                    }
                } else {
                    None
                };
                self.emit_event(SipTransportEvent {
                    timestamp_millis: now_millis(),
                    device_id: device_id.clone(),
                    direction: SipLogDirection::Receive,
                    message: display_text,
                    channel_id: channel_id.clone(),
                    is_request: true,
                    method: method.clone(),
                    command_type,
                    event,
                    local_tag: local_tag.clone(),
                    from_tag,
                    request_uri: request_uri.clone(),
                    call_id: call_id.clone(),
                    expires,
                });
                let special_response = match method.as_deref() {
                    Some("INVITE") => {
                        self.handle_invite(
                            &request,
                            parse_text,
                            &device_id,
                            channel_id.as_deref(),
                            &catalog_devices,
                            media.as_ref(),
                        )
                        .await
                    }
                    Some("BYE") => self.handle_bye(&request, parse_text, media.as_ref()).await,
                    Some("ACK") => self.handle_ack(&request, media.as_ref()).await,
                    _ => None,
                };
                let disposition = if special_response.is_some() {
                    InboundRequestDisposition::NoResponse
                } else {
                    self.dispatch_inbound_request(
                        &request,
                        parse_text,
                        method.as_deref(),
                        &catalog_devices,
                    )
                    .await
                };
                let query_permit = if matches!(
                    disposition,
                    InboundRequestDisposition::RespondAndQuery { .. }
                ) {
                    reserve_query_permit(&self.query_executor)
                } else {
                    None
                };
                let disposition = if matches!(
                    disposition,
                    InboundRequestDisposition::RespondAndQuery { .. }
                ) && query_permit.is_none()
                {
                    InboundRequestDisposition::Respond {
                        status: 503,
                        reason: "Service Unavailable",
                    }
                } else {
                    disposition
                };
                let response = special_response.or_else(|| {
                    disposition.response().map(|(status, reason)| {
                        build_request_response(
                            parse_text,
                            status,
                            reason,
                            local_tag.as_deref(),
                            &device_id,
                            self.advertised_ip,
                            self.local_port,
                        )
                    })
                });
                if let Some(response) = response {
                    let response_call_id = extract_header(&response, "Call-ID");
                    let payload = response.as_bytes().to_vec();
                    let _ = self.socket.send(&payload).await;
                    if let Some(key) = server_key.clone() {
                        self.server_transactions.lock().await.insert(
                            key,
                            CachedServerResponse {
                                response: payload,
                                expires_at: Instant::now() + SERVER_TRANSACTION_TTL,
                            },
                        );
                    }
                    self.emit_event(SipTransportEvent {
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
                    });
                    if is_subscribe
                        && expires == Some(0)
                        && let Some(call_id) = call_id.as_ref()
                    {
                        self.uas_tags.lock().await.remove(call_id);
                    }
                }
                if let InboundRequestDisposition::RespondAndQuery { body } = disposition {
                    let client = Arc::clone(&self);
                    let Some(permit) = query_permit else {
                        drop(query_permit);
                        continue;
                    };
                    let request = request.clone();
                    let device_id = device_id.clone();
                    let cancellation = cancellation.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let _ = client
                            .send_query_response(&device_id, &request, body, &cancellation)
                            .await;
                    });
                } else {
                    drop(query_permit);
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
                self.emit_event(SipTransportEvent {
                    timestamp_millis: now_millis(),
                    device_id: context.device_id,
                    direction: SipLogDirection::Receive,
                    message: display_text.clone(),
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
                });
            } else {
                let response_text = display_text;
                let requested_id = response
                    .headers
                    .get(&HeaderName::To)
                    .or_else(|| response.headers.get(&HeaderName::From))
                    .and_then(HeaderValue::as_from_to)
                    .and_then(|header| header.uri.user_info.as_ref())
                    .map(|user| user.user.clone());
                let (device_id, channel_id) = requested_id
                    .as_deref()
                    .map(|value| {
                        resolve_device_and_channel(value, &catalog_devices, &channel_index)
                    })
                    .unwrap_or_default();
                let method = response
                    .headers
                    .get(&HeaderName::CSeq)
                    .and_then(HeaderValue::as_cseq)
                    .map(|cseq| cseq.method.to_string());
                self.emit_event(SipTransportEvent {
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
                });
            }
            if is_final
                && let Some(sender) = self.transactions.take_final(transaction_key.as_ref()).await
            {
                let _ = sender.send(response);
            }
        }
    }

    /// 执行带通道上下文的业务交换，并只接受 SIP 2xx。
    pub(super) async fn exchange_with_channel(
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

    pub(super) async fn exchange_raw(
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
                    command_type: payload_command_type(&payload, self.signal_charset),
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
            self.emit_event(SipTransportEvent {
                timestamp_millis: now_millis(),
                device_id: device_id.to_owned(),
                direction: SipLogDirection::Send,
                message: sip_message_for_display(&payload, self.signal_charset),
                channel_id: channel_id.clone(),
                is_request: true,
                method: request_method(&payload),
                command_type: payload_command_type(&payload, self.signal_charset),
                event: None,
                local_tag: None,
                from_tag: None,
                request_uri: None,
                call_id: Some(call_id.clone()),
                expires: None,
            });

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
                let mut invites = self.invite_transactions.lock().await;
                invites.retain(|_, transaction| {
                    transaction.expires_at > Instant::now()
                        && transaction.state != InviteServerTransactionState::Terminated
                });
                let key = invite_key_from_cancel(request);
                let disposition = cancel_invite_transaction(&mut invites, key.as_ref());
                drop(invites);
                disposition
            }
            "BYE" => {
                // This phase never establishes an INVITE dialog, so every BYE is out of dialog.
                InboundRequestDisposition::Respond {
                    status: 481,
                    reason: "Call/Transaction Does Not Exist",
                }
            }
            "ACK" => InboundRequestDisposition::NoResponse,
            "INVITE" => {
                if let Some(key) = transaction_key_from_request(request) {
                    let mut transaction = InviteServerTransaction {
                        state: InviteServerTransactionState::Proceeding,
                        expires_at: Instant::now() + SERVER_TRANSACTION_TTL,
                    };
                    transaction.state = InviteServerTransactionState::Completed;
                    self.invite_transactions
                        .lock()
                        .await
                        .insert(key, transaction);
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

    #[expect(
        clippy::too_many_arguments,
        reason = "INVITE 需要完整的协议和目录上下文"
    )]
    async fn handle_invite(
        &self,
        request: &SipRequest,
        raw: &str,
        device_id: &str,
        channel_id: Option<&str>,
        devices: &HashMap<String, SimulatedDevice>,
        media: Option<&MediaSessionCoordinator>,
    ) -> Option<String> {
        let media = media?;
        if !devices.contains_key(device_id) || channel_id.is_none() {
            return Some(build_request_response(
                raw,
                404,
                "Not Found",
                None,
                device_id,
                self.advertised_ip,
                self.local_port,
            ));
        }
        if !media.source_available() {
            return Some(build_request_response(
                raw,
                500,
                "Server Internal Error",
                None,
                device_id,
                self.advertised_ip,
                self.local_port,
            ));
        }
        let Ok(offer) = SdpOffer::parse(&request_body_text(request)) else {
            return Some(build_request_response(
                raw,
                488,
                "Not Acceptable Here",
                None,
                device_id,
                self.advertised_ip,
                self.local_port,
            ));
        };
        let remote = std::net::SocketAddr::new(offer.remote_address, offer.remote_port);
        let call_id = request_call_id(request)?;
        let ssrc = offer.ssrc.unwrap_or_else(|| {
            call_id.bytes().fold(0_u32, |hash, byte| {
                hash.wrapping_mul(33).wrapping_add(u32::from(byte))
            })
        });
        let local_port = match media.start(call_id.clone(), remote, ssrc).await {
            Ok(local) => local.port(),
            Err(_) => {
                return Some(build_request_response(
                    raw,
                    486,
                    "Busy Here",
                    None,
                    device_id,
                    self.advertised_ip,
                    self.local_port,
                ));
            }
        };
        let local_tag = Tag::new().to_string();
        self.invite_dialog_tags
            .lock()
            .await
            .insert(call_id.clone(), local_tag.clone());
        let answer = SdpAnswer {
            address: self.advertised_ip,
            port: local_port,
            payload_type: offer.payload_type,
            ssrc,
        }
        .to_string();
        let mut response = build_request_response(
            raw,
            200,
            "OK",
            Some(&local_tag),
            device_id,
            self.advertised_ip,
            self.local_port,
        );
        if let Some(prefix) = response.strip_suffix("Content-Length: 0\r\n\r\n") {
            response = format!(
                "{prefix}Content-Type: application/sdp\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
        }
        Some(response)
    }

    async fn handle_bye(
        &self,
        request: &SipRequest,
        raw: &str,
        media: Option<&MediaSessionCoordinator>,
    ) -> Option<String> {
        let media = media?;
        let call_id = request_call_id(request)?;
        if !media.stop(&call_id).await {
            return None;
        }
        self.invite_dialog_tags.lock().await.remove(&call_id);
        Some(build_request_response(
            raw,
            200,
            "OK",
            None,
            "",
            self.advertised_ip,
            self.local_port,
        ))
    }

    async fn handle_ack(
        &self,
        request: &SipRequest,
        media: Option<&MediaSessionCoordinator>,
    ) -> Option<String> {
        let media = media?;
        let call_id = request_call_id(request)?;
        let expected_tag = self.invite_dialog_tags.lock().await.get(&call_id).cloned();
        let received_tag = request
            .headers
            .get(&HeaderName::To)
            .and_then(HeaderValue::as_from_to)
            .and_then(|header| header.tag.as_ref())
            .map(ToString::to_string);
        if expected_tag.as_deref() != received_tag.as_deref() {
            return None;
        }
        let _ = media.activate(&call_id).await;
        None
    }

    pub(super) const fn endpoint_host(&self) -> Host {
        match self.advertised_ip {
            IpAddr::V4(address) => Host::IPv4(address),
            IpAddr::V6(address) => Host::IPv6(address),
        }
    }

    pub(super) const fn local_port(&self) -> u16 {
        self.local_port
    }

    async fn send_query_response(
        &self,
        device_id: &str,
        request: &SipRequest,
        body: String,
        cancellation: &CancellationToken,
    ) -> Result<(), SipRegistrationError> {
        let aor = SipUri::parse(&format!("sip:{device_id}@{}", self.domain))
            .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let contact = SipUri::parse(&format!(
            "sip:{device_id}@{}:{}",
            self.advertised_ip, self.local_port
        ))
        .map_err(|error| SipRegistrationError::InvalidUri(error.to_string()))?;
        let cseq = self
            .query_cseq
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let mut headers = HeaderCollection::new();
        headers.insert(
            HeaderName::Via,
            HeaderValue::Via(ViaHeader::new(
                TransportProtocol::Udp,
                contact.host.clone(),
                contact.port,
            )),
        );
        headers.insert(
            HeaderName::From,
            HeaderValue::FromTo(FromToHeader::new(aor.clone()).with_tag(Tag::new())),
        );
        headers.insert(
            HeaderName::To,
            request
                .headers
                .get(&HeaderName::From)
                .cloned()
                .unwrap_or_else(|| HeaderValue::FromTo(FromToHeader::new(self.registrar.clone()))),
        );
        headers.insert(
            HeaderName::CallId,
            HeaderValue::CallId(CallId::with_host(&self.endpoint_host().to_string())),
        );
        headers.insert(
            HeaderName::CSeq,
            HeaderValue::CSeq(CSeqHeader::new(cseq, Method::Message)),
        );
        headers.insert(
            HeaderName::Contact,
            HeaderValue::Contact(ContactHeader::new(contact)),
        );
        headers.insert(HeaderName::MaxForwards, HeaderValue::MaxForwards(70));
        let encoded = encode_xml(&body, self.signal_charset)
            .map_err(|error| SipRegistrationError::Charset(error.to_string()))?;
        headers.insert(
            HeaderName::ContentType,
            HeaderValue::ContentType(encoded.content_type.clone()),
        );
        let outbound = SipRequest {
            request_line: RequestLine {
                method: Method::Message,
                request_uri: self.registrar.clone(),
                version: SipVersion,
            },
            headers,
            body: Some(siprs::siprs_message::Body::new(
                encoded.content_type,
                encoded.bytes,
            )),
        };
        self.exchange_with_channel(device_id, outbound, cancellation, None)
            .await
            .map(|_| ())
    }
}

/// Reserves a bounded query worker slot before acknowledging the SIP request.
///
/// Keeping this decision synchronous and non-blocking means saturation is exposed as a
/// protocol-level 503 instead of accepting a request and silently dropping its business
/// response later.
fn reserve_query_permit(
    executor: &Arc<tokio::sync::Semaphore>,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    Arc::clone(executor).try_acquire_owned().ok()
}

fn select_uas_dialog_tag(request_tag: Option<String>, cached_tag: Option<&str>) -> String {
    request_tag
        .or_else(|| cached_tag.map(str::to_owned))
        .unwrap_or_else(|| Tag::new().to_string())
}

fn cancel_invite_transaction(
    invites: &mut HashMap<TransactionKey, InviteServerTransaction>,
    key: Option<&TransactionKey>,
) -> InboundRequestDisposition {
    let Some(key) = key else {
        return InboundRequestDisposition::Respond {
            status: 481,
            reason: "Call/Transaction Does Not Exist",
        };
    };
    let Some(transaction) = invites.get_mut(key) else {
        return InboundRequestDisposition::Respond {
            status: 481,
            reason: "Call/Transaction Does Not Exist",
        };
    };
    if transaction.state != InviteServerTransactionState::Proceeding {
        return InboundRequestDisposition::Respond {
            status: 481,
            reason: "Call/Transaction Does Not Exist",
        };
    }
    transaction.state = InviteServerTransactionState::Terminated;
    InboundRequestDisposition::Respond {
        status: 200,
        reason: "OK",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use siprs::siprs_message::Method;
    use tokio::{sync::Semaphore, time::Instant};

    use super::{
        InboundRequestDisposition, cancel_invite_transaction, reserve_query_permit,
        select_uas_dialog_tag,
    };
    use crate::sip::{
        registration::{InviteServerTransaction, InviteServerTransactionState},
        transaction::TransactionKey,
    };

    fn invite_key() -> TransactionKey {
        TransactionKey {
            call_id: "call-1".to_owned(),
            cseq: 42,
            method: Method::Invite,
            branch: "z9hG4bK-invite".to_owned(),
        }
    }

    #[test]
    fn cancel_after_invite_final_response_should_return_481() {
        let key = invite_key();
        let mut invites = HashMap::from([(
            key.clone(),
            InviteServerTransaction {
                state: InviteServerTransactionState::Completed,
                expires_at: Instant::now() + Duration::from_secs(32),
            },
        )]);

        let disposition = cancel_invite_transaction(&mut invites, Some(&key));

        assert!(matches!(
            disposition,
            InboundRequestDisposition::Respond { status: 481, .. }
        ));
        assert_eq!(
            invites.get(&key).map(|transaction| transaction.state),
            Some(InviteServerTransactionState::Completed)
        );
    }

    #[test]
    fn cancel_while_invite_is_proceeding_should_terminate_transaction() {
        let key = invite_key();
        let mut invites = HashMap::from([(
            key.clone(),
            InviteServerTransaction {
                state: InviteServerTransactionState::Proceeding,
                expires_at: Instant::now() + Duration::from_secs(32),
            },
        )]);

        let disposition = cancel_invite_transaction(&mut invites, Some(&key));

        assert!(matches!(
            disposition,
            InboundRequestDisposition::Respond { status: 200, .. }
        ));
        assert_eq!(
            invites.get(&key).map(|transaction| transaction.state),
            Some(InviteServerTransactionState::Terminated)
        );
    }

    #[test]
    fn query_capacity_should_be_rejected_before_ack_when_saturated() {
        let executor = std::sync::Arc::new(Semaphore::new(1));
        let permit = reserve_query_permit(&executor);

        assert!(permit.is_some());
        assert!(reserve_query_permit(&executor).is_none());

        drop(permit);
        assert!(reserve_query_permit(&executor).is_some());
    }

    #[test]
    fn subscription_tag_should_prefer_request_tag_then_cache() {
        assert_eq!(
            select_uas_dialog_tag(Some("request-tag".to_owned()), Some("cached-tag")),
            "request-tag"
        );
        assert_eq!(
            select_uas_dialog_tag(None, Some("cached-tag")),
            "cached-tag"
        );
    }
}
