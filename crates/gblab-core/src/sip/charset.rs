//! GB28181 XML 信令字符集编解码与 SIP 入站报文预处理。
//!
//! `siprs-message` 当前要求整条 SIP 报文是 UTF-8。本模块只在适配层将
//! GB2312/GBK XML 正文转换成 UTF-8 解析副本，原始字节仍由 UDP 传输层接收。

use std::{borrow::Cow, str};

use encoding_rs::GBK;
use thiserror::Error;

use crate::SignalCharset;

const XML_CONTENT_TYPE: &str = "application/manscdp+xml";

/// 编码后的 XML 正文及其 Content-Type。
pub(super) struct EncodedXml {
    pub(super) bytes: Vec<u8>,
    pub(super) content_type: String,
}

/// 供 `siprs-message` 解析和交互日志展示的入站报文视图。
pub(super) struct PreparedSipMessage {
    pub(super) parser_bytes: Vec<u8>,
    pub(super) display_text: String,
}

#[derive(Debug, Error)]
pub(super) enum CharsetError {
    #[error("SIP 报文头不是有效 UTF-8")]
    InvalidSipHeaders,
    #[error("SIP 报文缺少完整的头部结束标记")]
    MissingHeaderBoundary,
    #[error("SIP Content-Length 无效或正文不完整")]
    InvalidContentLength,
    #[error("不支持的信令字符集: {0}")]
    UnsupportedCharset(String),
    #[error("XML 正文不是有效的 {0} 编码")]
    Decode(&'static str),
    #[error("XML 正文包含 {0} 无法表示的字符")]
    Encode(&'static str),
}

/// 按设备全局字符集编码 XML，并使声明、Content-Type 与实际字节保持一致。
pub(super) fn encode_xml(xml: &str, charset: SignalCharset) -> Result<EncodedXml, CharsetError> {
    let xml = xml_with_declaration(xml, charset);
    let bytes = match charset {
        SignalCharset::Utf8 => xml.into_bytes(),
        // 国标平台通常将 GB2312 作为 GBK 的兼容子集处理。两者使用同一成熟
        // 编解码器，但保留各自的协议标签，避免系统 iconv 带来的跨平台差异。
        SignalCharset::Gb2312 | SignalCharset::Gbk => {
            let (encoded, _, had_errors) = GBK.encode(&xml);
            if had_errors {
                return Err(CharsetError::Encode(charset.label()));
            }
            encoded.into_owned()
        }
    };
    Ok(EncodedXml {
        bytes,
        content_type: format!("Application/MANSCDP+xml;charset={}", charset.label()),
    })
}

/// 将平台 XML 正文严格解码为 UTF-8。
pub(super) fn decode_xml(
    body: &[u8],
    content_type: Option<&str>,
    fallback: SignalCharset,
) -> Result<String, CharsetError> {
    let charset = match content_type {
        Some(content_type) => charset_from_content_type(content_type)?,
        None => None,
    }
    .or(charset_from_xml_declaration(body)?)
    .unwrap_or(fallback);

    match charset {
        SignalCharset::Utf8 => str::from_utf8(body)
            .map(str::to_owned)
            .map_err(|_| CharsetError::Decode(charset.label())),
        SignalCharset::Gb2312 | SignalCharset::Gbk => GBK
            .decode_without_bom_handling_and_without_replacement(body)
            .map(Cow::into_owned)
            .ok_or_else(|| CharsetError::Decode(charset.label())),
    }
}

/// 为 `siprs-message` 构造 UTF-8 解析副本，同时保留可读的原始头部日志视图。
pub(super) fn prepare_inbound_sip_message(
    raw: &[u8],
    fallback: SignalCharset,
) -> Result<PreparedSipMessage, CharsetError> {
    let Some((header_end, separator_len)) = find_header_boundary(raw) else {
        return Err(CharsetError::MissingHeaderBoundary);
    };
    let headers =
        str::from_utf8(&raw[..header_end]).map_err(|_| CharsetError::InvalidSipHeaders)?;
    let available_body = &raw[header_end + separator_len..];
    let body = match header_value(headers, "content-length") {
        Some(value) => {
            let length = value
                .parse::<usize>()
                .map_err(|_| CharsetError::InvalidContentLength)?;
            available_body
                .get(..length)
                .ok_or(CharsetError::InvalidContentLength)?
        }
        None => available_body,
    };
    if body.is_empty() {
        let text = str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| CharsetError::InvalidSipHeaders)?;
        return Ok(PreparedSipMessage {
            parser_bytes: raw.to_vec(),
            display_text: text,
        });
    }

    let content_type = header_value(headers, "content-type");
    if !content_type.is_some_and(|value| value.to_ascii_lowercase().contains(XML_CONTENT_TYPE)) {
        let text = str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| CharsetError::Decode("UTF-8"))?;
        return Ok(PreparedSipMessage {
            parser_bytes: raw.to_vec(),
            display_text: text,
        });
    }

    let decoded_body = decode_xml(body, content_type, fallback)?;
    let parser_headers = headers_with_content_length(headers, decoded_body.len());
    let mut parser_bytes = Vec::with_capacity(parser_headers.len() + 4 + decoded_body.len());
    parser_bytes.extend_from_slice(parser_headers.as_bytes());
    parser_bytes.extend_from_slice(b"\r\n\r\n");
    parser_bytes.extend_from_slice(decoded_body.as_bytes());

    let separator = if separator_len == 4 {
        "\r\n\r\n"
    } else {
        "\n\n"
    };
    let display_text = format!("{headers}{separator}{decoded_body}");
    Ok(PreparedSipMessage {
        parser_bytes,
        display_text,
    })
}

/// 去除任意合法 XML 声明，规避上游解析器只识别固定 UTF-8 声明的问题。
pub(super) fn xml_without_declaration(xml: &str) -> &str {
    let trimmed = xml.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with("<?xml") {
        return trimmed;
    }
    trimmed
        .find("?>")
        .map_or(trimmed, |end| trimmed[end + 2..].trim_start())
}

pub(super) fn sip_message_for_display(payload: &[u8], fallback: SignalCharset) -> String {
    prepare_inbound_sip_message(payload, fallback).map_or_else(
        |_| String::from_utf8_lossy(payload).into_owned(),
        |prepared| prepared.display_text,
    )
}

fn xml_with_declaration(xml: &str, charset: SignalCharset) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"{}\"?>\n{}",
        charset.label(),
        xml_without_declaration(xml)
    )
}

fn charset_from_content_type(content_type: &str) -> Result<Option<SignalCharset>, CharsetError> {
    for parameter in content_type.split(';').skip(1) {
        let Some((name, value)) = parameter.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("charset") {
            let label = value.trim_matches(|character| character == '"' || character == '\'');
            return parse_charset_label_result(label).map(Some);
        }
    }
    Ok(None)
}

fn charset_from_xml_declaration(body: &[u8]) -> Result<Option<SignalCharset>, CharsetError> {
    let prefix_len = body.len().min(512);
    let prefix = &body[..prefix_len];
    let Some(end) = prefix.windows(2).position(|window| window == b"?>") else {
        return Ok(None);
    };
    let declaration = str::from_utf8(&prefix[..end + 2])
        .map_err(|_| CharsetError::UnsupportedCharset("XML 声明不是 ASCII".to_owned()))?;
    let lower = declaration.to_ascii_lowercase();
    let Some(encoding_start) = lower.find("encoding") else {
        return Ok(None);
    };
    let Some((_, value)) = declaration[encoding_start..].split_once('=') else {
        return Ok(None);
    };
    let value = value.trim_start();
    let label = match value.as_bytes().first() {
        Some(b'"') => value[1..].split_once('"').map(|(label, _)| label),
        Some(b'\'') => value[1..].split_once('\'').map(|(label, _)| label),
        _ => value.split_whitespace().next(),
    };
    label.map(parse_charset_label_result).transpose()
}

fn parse_charset_label_result(label: &str) -> Result<SignalCharset, CharsetError> {
    match label.trim().to_ascii_uppercase().replace('_', "-").as_str() {
        "GB2312" | "GB-2312" | "GB-2312-80" | "CSGB2312" => Ok(SignalCharset::Gb2312),
        "GBK" | "CP936" | "MS936" => Ok(SignalCharset::Gbk),
        "UTF-8" | "UTF8" => Ok(SignalCharset::Utf8),
        _ => Err(CharsetError::UnsupportedCharset(label.to_owned())),
    }
}

fn find_header_boundary(raw: &[u8]) -> Option<(usize, usize)> {
    raw.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| (position, 4))
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| (position, 2))
        })
}

fn header_value<'a>(headers: &'a str, expected_name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(expected_name)
            .then_some(value.trim())
    })
}

fn headers_with_content_length(headers: &str, content_length: usize) -> String {
    let mut replaced = false;
    let mut lines = headers
        .lines()
        .map(|line| {
            if line
                .split_once(':')
                .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            {
                replaced = true;
                format!("Content-Length: {content_length}")
            } else {
                line.trim_end_matches('\r').to_owned()
            }
        })
        .collect::<Vec<_>>();
    if !replaced {
        lines.push(format!("Content-Length: {content_length}"));
    }
    lines.join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::{decode_xml, encode_xml, prepare_inbound_sip_message, xml_without_declaration};
    use crate::SignalCharset;

    #[test]
    fn xml_codec_should_round_trip_supported_charsets() -> Result<(), Box<dyn std::error::Error>> {
        let xml = "<Response><DeviceName>模拟摄像机</DeviceName></Response>";
        for charset in [
            SignalCharset::Gb2312,
            SignalCharset::Gbk,
            SignalCharset::Utf8,
        ] {
            let encoded = encode_xml(xml, charset)?;
            let decoded = decode_xml(&encoded.bytes, Some(&encoded.content_type), charset)?;
            assert!(decoded.contains("模拟摄像机"));
            assert!(decoded.contains(charset.label()));
        }
        Ok(())
    }

    #[test]
    fn declaration_normalizer_should_accept_platform_gb2312_declaration() {
        let xml = "<?xml version=\"1.0\" encoding=\"GB2312\" standalone=\"yes\"?>\n<Query></Query>";

        assert_eq!(xml_without_declaration(xml), "<Query></Query>");
    }

    #[test]
    fn inbound_preparation_should_decode_gbk_body_and_recalculate_length()
    -> Result<(), Box<dyn std::error::Error>> {
        let xml = "<Response><DeviceName>扩展字符</DeviceName></Response>";
        let encoded = encode_xml(xml, SignalCharset::Gbk)?;
        let message = format!(
            "MESSAGE sip:device SIP/2.0\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
            encoded.content_type,
            encoded.bytes.len()
        );
        let mut raw = message.into_bytes();
        raw.extend_from_slice(&encoded.bytes);

        let prepared = prepare_inbound_sip_message(&raw, SignalCharset::Gb2312)?;

        assert!(prepared.display_text.contains("扩展字符"));
        assert!(str::from_utf8(&prepared.parser_bytes)?.contains("扩展字符"));
        Ok(())
    }
}
