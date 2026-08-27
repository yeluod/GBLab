//! 入站 SIP 方法分发、GB28181 XML 路由与响应构造。
//!
//! 此模块只处理协议层的解析与分派，不持有运行时设备状态或 Tokio 生命周期。

use std::{collections::HashMap, fmt::Write, net::IpAddr};

use siprs::{
    siprs_core::{SipVersion, StatusCode},
    siprs_gb28181_xml::{
        CmdType, DeviceItem, DeviceStatusInfo, Message as GbMessage, Query, Response, parse_xml,
    },
    siprs_message::{
        ContactHeader, HeaderCollection, HeaderName, HeaderValue, MessageBuilder, MessageParser,
        Method, SipMessage, SipRequest, SipResponse, SipUri, StatusLine, Tag,
    },
};

use crate::{SignalCharset, SimulatedDevice, domain::derive_channels_for_device};

use super::{
    charset::{sip_message_for_display, xml_without_declaration},
    dialog::DialogId,
    registration::SipRegistrationError,
    transaction::TransactionKey,
};

pub(super) const SIP_MESSAGE_LIMIT: usize = 65_536;

pub(super) fn payload_command_type(payload: &[u8], fallback: SignalCharset) -> Option<String> {
    let text = sip_message_for_display(payload, fallback);
    sip_body(&text).and_then(xml_command_type)
}

pub(super) fn request_body_text(request: &SipRequest) -> String {
    request
        .body
        .as_ref()
        .map(|body| String::from_utf8_lossy(&body.content).into_owned())
        .unwrap_or_default()
}

pub(super) fn structured_header_value(request: &SipRequest, name: &HeaderName) -> Option<String> {
    request.headers.get(name).and_then(|value| match value {
        HeaderValue::Raw(value) | HeaderValue::ContentType(value) => Some(value.clone()),
        HeaderValue::Expires(value) => Some(value.to_string()),
        _ => None,
    })
}

pub(super) fn header_u32_value(value: &HeaderValue) -> Option<u32> {
    match value {
        HeaderValue::Expires(value) => Some(*value),
        HeaderValue::Raw(value) => value.trim().parse().ok(),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SipResponseClass {
    Provisional,
    Success,
    Redirection,
    ClientFailure,
    ServerFailure,
    GlobalFailure,
}

impl SipResponseClass {
    pub(super) const fn is_final(self) -> bool {
        !matches!(self, Self::Provisional)
    }
}

pub(super) const fn response_class(status_code: u16) -> SipResponseClass {
    match status_code {
        100..=199 => SipResponseClass::Provisional,
        200..=299 => SipResponseClass::Success,
        300..=399 => SipResponseClass::Redirection,
        400..=499 => SipResponseClass::ClientFailure,
        500..=599 => SipResponseClass::ServerFailure,
        _ => SipResponseClass::GlobalFailure,
    }
}

pub(super) fn accept_sip_success(
    response: SipResponse,
) -> Result<SipResponse, SipRegistrationError> {
    match response_class(response.status_line.status_code.0) {
        SipResponseClass::Success => Ok(response),
        _ => Err(SipRegistrationError::Rejected {
            code: response.status_line.status_code.0,
            reason: response.status_line.reason_phrase,
        }),
    }
}

pub(super) fn transaction_key_from_request(request: &SipRequest) -> Option<TransactionKey> {
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

pub(super) fn transaction_key_from_response(response: &SipResponse) -> Option<TransactionKey> {
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

pub(super) fn request_call_id(request: &SipRequest) -> Option<String> {
    request
        .headers
        .get(&HeaderName::CallId)
        .and_then(HeaderValue::as_call_id)
        .map(|value| value.0.clone())
}

pub(super) fn request_cseq(request: &SipRequest) -> Option<u32> {
    request
        .headers
        .get(&HeaderName::CSeq)
        .and_then(HeaderValue::as_cseq)
        .map(|value| value.sequence.0)
}

pub(super) fn invite_key_from_cancel(request: &SipRequest) -> Option<TransactionKey> {
    let key = transaction_key_from_request(request)?;
    Some(TransactionKey {
        call_id: key.call_id,
        cseq: key.cseq,
        method: Method::Invite,
        branch: key.branch,
    })
}

pub(super) fn request_dialog_id(request: &SipRequest) -> Option<DialogId> {
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
    Some(DialogId::new(call_id, local_tag, remote_tag))
}

pub(super) fn extract_header(message: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name.to_ascii_lowercase());
    message
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim().to_owned())
}

pub(super) fn request_method(payload: &[u8]) -> Option<String> {
    String::from_utf8_lossy(payload)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned)
}

pub(super) fn resolve_device_and_channel(
    requested_id: &str,
    devices: &HashMap<String, SimulatedDevice>,
    channel_index: &HashMap<String, String>,
) -> (String, Option<String>) {
    if devices.contains_key(requested_id) {
        return (requested_id.to_owned(), None);
    }
    if let Some(device_id) = channel_index.get(requested_id) {
        return (device_id.clone(), Some(requested_id.to_owned()));
    }
    (requested_id.to_owned(), None)
}

pub(super) fn build_channel_index(
    devices: &HashMap<String, SimulatedDevice>,
) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for (device_id, device) in devices {
        if let Ok(channels) = derive_channels_for_device(device) {
            for channel in channels {
                index.insert(channel.id.to_string(), device_id.clone());
            }
        }
    }
    index
}

pub(super) enum InboundRequestDisposition {
    NoResponse,
    Respond { status: u16, reason: &'static str },
    RespondAndQuery { body: String },
}

impl InboundRequestDisposition {
    pub(super) const fn response(&self) -> Option<(u16, &'static str)> {
        match self {
            Self::NoResponse => None,
            Self::Respond { status, reason } => Some((*status, reason)),
            Self::RespondAndQuery { .. } => Some((200, "OK")),
        }
    }
}

pub(super) fn dispatch_inbound_request(
    request: &str,
    method: Option<&str>,
    devices: &HashMap<String, SimulatedDevice>,
) -> InboundRequestDisposition {
    match method.unwrap_or_default().to_ascii_uppercase().as_str() {
        "MESSAGE" => dispatch_platform_request(request, devices).map_or(
            InboundRequestDisposition::Respond {
                status: 400,
                reason: "Bad Request",
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
pub(super) fn build_request_response(
    request: &str,
    status: u16,
    reason: &str,
    local_tag: Option<&str>,
    device_id: &str,
    advertised_ip: IpAddr,
    local_port: u16,
) -> String {
    let parser = MessageParser::new(SIP_MESSAGE_LIMIT);
    if let Ok(SipMessage::Request(parsed)) = parser.parse(request.as_bytes()) {
        let method = parsed.request_line.method.to_string();
        let request_headers = parsed.headers;
        let mut headers = HeaderCollection::new();
        for via in request_headers.get_all(&HeaderName::Via) {
            headers.insert(HeaderName::Via, via.clone());
        }
        for name in [HeaderName::From, HeaderName::CallId, HeaderName::CSeq] {
            if let Some(value) = request_headers.get(&name) {
                headers.insert(name, value.clone());
            }
        }
        if let Some(to) = request_headers.get(&HeaderName::To).cloned() {
            let to = match (status / 100 == 2, local_tag, to) {
                (true, Some(local_tag), HeaderValue::FromTo(to)) => {
                    HeaderValue::FromTo(to.with_tag(Tag(local_tag.to_owned())))
                }
                (_, _, value) => value,
            };
            headers.insert(HeaderName::To, to);
        }
        if method == "SUBSCRIBE" {
            if let Some(expires) = request_headers.get(&HeaderName::Expires) {
                headers.insert(HeaderName::Expires, expires.clone());
            }
            if let Ok(contact) =
                SipUri::parse(&format!("sip:{device_id}@{advertised_ip}:{local_port}"))
            {
                headers.insert(
                    HeaderName::Contact,
                    HeaderValue::Contact(ContactHeader::new(contact)),
                );
            }
        }
        if status == 405 {
            headers.insert(
                HeaderName::Extension("Allow".to_owned()),
                HeaderValue::Raw(
                    "MESSAGE, SUBSCRIBE, OPTIONS, INVITE, ACK, BYE, CANCEL".to_owned(),
                ),
            );
        }
        if method == "OPTIONS" && status / 100 == 2 {
            headers.insert(
                HeaderName::Extension("Allow".to_owned()),
                HeaderValue::Raw(
                    "MESSAGE, SUBSCRIBE, OPTIONS, INVITE, ACK, BYE, CANCEL".to_owned(),
                ),
            );
        }
        let response = SipResponse {
            status_line: StatusLine {
                version: SipVersion,
                status_code: StatusCode(status),
                reason_phrase: reason.to_owned(),
            },
            headers,
            body: None,
        };
        if let Ok(payload) =
            MessageBuilder::with_validation(false).build(&SipMessage::Response(response))
        {
            return String::from_utf8_lossy(&payload).into_owned();
        }
    }
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
    }
    response.push_str("Content-Length: 0\r\n\r\n");
    response
}

pub(super) fn dispatch_platform_request(
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
    match parse_gb_xml(body).ok()? {
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

pub(super) fn build_device_info_body(
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

pub(super) fn build_device_status_body(device_id: &str, sn: u32) -> String {
    let Ok(device_id) = siprs::siprs_gb28181_codec::DeviceId::parse(device_id) else {
        return format!(
            "<Response><CmdType>DeviceStatus</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Result>ERROR</Result></Response>"
        );
    };
    Response::device_status(sn, device_id, DeviceStatusInfo::new(true, "OK")).to_xml()
}

pub(super) fn build_device_control_body(device_id: &str, sn: &str) -> String {
    format!(
        "<Response><CmdType>DeviceControl</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><Result>OK</Result></Response>"
    )
}

pub(super) fn build_record_info_body(device_id: &str, sn: &str) -> String {
    format!(
        "<Response><CmdType>RecordInfo</CmdType><SN>{sn}</SN><DeviceID>{device_id}</DeviceID><SumNum>0</SumNum><RecordList Num=\"0\"></RecordList></Response>"
    )
}

pub(super) fn build_catalog_body(
    query: &Query,
    devices: &HashMap<String, SimulatedDevice>,
) -> String {
    let device_id = query.device_id.as_str();
    let Some(device) = devices.get(device_id) else {
        return Response::catalog(query.sn, query.device_id.clone(), 0, Vec::new()).to_xml();
    };
    let mut items = Vec::with_capacity(device.channel_count as usize);
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

pub(super) fn sip_body(request: &str) -> Option<&str> {
    request
        .split_once("\r\n\r\n")
        .or_else(|| request.split_once("\n\n"))
        .map(|(_, body)| body.trim())
        .filter(|body| !body.is_empty())
}

pub(super) fn xml_metadata(xml: &str) -> (Option<String>, Option<String>) {
    let Ok(message) = parse_gb_xml(xml) else {
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

pub(super) fn xml_command_type(xml: &str) -> Option<String> {
    xml_metadata(xml).1
}

/// 解析允许使用任意 XML 声明字符集与 standalone 属性的 GB28181 XML。
pub fn parse_gb_xml(xml: &str) -> Result<GbMessage, siprs::siprs_gb28181_xml::XmlError> {
    parse_xml(xml_without_declaration(xml))
}

pub(super) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::IpAddr};

    use siprs::siprs_gb28181_codec::DeviceId as CodecDeviceId;
    use siprs::{
        siprs_gb28181_xml::{Message as GbMessage, Query, parse_xml},
        siprs_message::{HeaderName, MessageParser, SipMessage},
    };

    use crate::{
        DeviceKind, SimulatedDevice,
        domain::{DeviceId, DeviceIdError},
    };

    use super::{SIP_MESSAGE_LIMIT, build_catalog_body, build_request_response};

    fn simulated_device(channel_count: u16) -> Result<SimulatedDevice, DeviceIdError> {
        Ok(SimulatedDevice {
            id: DeviceId::new("34020000002000000100")?,
            name: "模拟摄像机-001".to_owned(),
            kind: DeviceKind::Camera,
            manufacturer: "GBLab".to_owned(),
            model: "SIM-CAM-100".to_owned(),
            firmware_version: "V1.0.0".to_owned(),
            channel_count,
            created_at: 0,
        })
    }

    #[test]
    fn catalog_response_should_only_contain_real_channels() -> Result<(), Box<dyn std::error::Error>>
    {
        let device = simulated_device(1)?;
        let device_id = CodecDeviceId::parse(device.id.as_str())?;
        let query = Query::catalog(7, device_id);
        let devices = HashMap::from([(device.id.to_string(), device)]);

        let parsed = parse_xml(&build_catalog_body(&query, &devices))?;
        let GbMessage::Response(response) = parsed else {
            return Err("目录响应应解析为 Response".into());
        };

        assert_eq!(response.sum_num, Some(1));
        assert_eq!(response.device_list.len(), 1);
        assert_eq!(
            response.device_list[0].device_id.as_str(),
            "34020000002000100001"
        );
        Ok(())
    }

    #[test]
    fn catalog_response_should_not_advertise_parent_device_as_channel()
    -> Result<(), Box<dyn std::error::Error>> {
        let device = simulated_device(2)?;
        let device_id = CodecDeviceId::parse(device.id.as_str())?;
        let query = Query::catalog(8, device_id);
        let devices = HashMap::from([(device.id.to_string(), device)]);

        let parsed = parse_xml(&build_catalog_body(&query, &devices))?;
        let GbMessage::Response(response) = parsed else {
            return Err("目录响应应解析为 Response".into());
        };

        assert!(
            response
                .device_list
                .iter()
                .all(|item| item.device_id.as_str() != "34020000002000000100")
        );
        Ok(())
    }

    #[test]
    fn subscribe_response_should_only_contain_response_headers_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let request = "SUBSCRIBE sip:34020000002000000100@192.168.10.94:5060 SIP/2.0\r\n\
                       Via: SIP/2.0/UDP 192.168.10.91:5060;branch=z9hG4bK-platform\r\n\
                       From: <sip:34020000002000000001@3402000000>;tag=platform-tag\r\n\
                       To: <sip:34020000002000000100@192.168.10.94:5060>\r\n\
                       Call-ID: alarm-subscription\r\n\
                       CSeq: 9 SUBSCRIBE\r\n\
                       Contact: <sip:34020000002000000001@192.168.10.91:5060>\r\n\
                       Event: Alarm\r\n\
                       Expires: 3599\r\n\
                       Content-Type: Application/MANSCDP+xml;charset=GB2312\r\n\
                       Content-Length: 0\r\n\r\n";

        let payload = build_request_response(
            request,
            200,
            "OK",
            Some("device-tag"),
            "34020000002000000100",
            "192.168.10.94".parse::<IpAddr>()?,
            5060,
        );
        let parsed = MessageParser::new(SIP_MESSAGE_LIMIT).parse(payload.as_bytes())?;
        let SipMessage::Response(response) = parsed else {
            return Err("应构造 SIP Response".into());
        };

        for header in [
            HeaderName::Via,
            HeaderName::From,
            HeaderName::To,
            HeaderName::CallId,
            HeaderName::CSeq,
            HeaderName::Contact,
        ] {
            assert_eq!(
                response.headers.get_all(&header).len(),
                1,
                "{header} 必须唯一"
            );
        }
        assert!(!response.headers.contains(&HeaderName::ContentType));
        Ok(())
    }
}
