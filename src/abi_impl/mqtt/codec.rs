use vm::VmError;

use super::model::MqttConnectConfig;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MqttIncomingPacket {
    ConnAck {
        reason_code: u8,
    },
    Publish {
        topic: String,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
        dup: bool,
        packet_id: Option<u16>,
    },
    PubAck {
        packet_id: u16,
        reason_code: u8,
    },
    SubAck {
        packet_id: u16,
        reason_codes: Vec<u8>,
    },
    UnsubAck {
        packet_id: u16,
        reason_codes: Vec<u8>,
    },
    PingResp,
    Disconnect {
        reason_code: u8,
    },
}

pub(crate) fn encode_variable_int(mut value: usize) -> Result<Vec<u8>, VmError> {
    if value > 268_435_455 {
        return Err(VmError::HostError(format!(
            "mqtt remaining length {value} exceeds protocol maximum",
        )));
    }
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value % 128) as u8;
        value /= 128;
        if value > 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return Ok(encoded);
        }
    }
}

pub(crate) fn decode_variable_int(bytes: &[u8]) -> Result<Option<(usize, usize)>, VmError> {
    let mut multiplier = 1usize;
    let mut value = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate().take(4) {
        value += usize::from(byte & 0x7f) * multiplier;
        if byte & 0x80 == 0 {
            return Ok(Some((value, index + 1)));
        }
        multiplier *= 128;
    }
    if bytes.len() >= 4 {
        return Err(VmError::HostError(
            "mqtt variable length integer exceeds 4 bytes".to_string(),
        ));
    }
    Ok(None)
}

fn encode_utf8(value: &str) -> Result<Vec<u8>, VmError> {
    let bytes = value.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| {
        VmError::HostError(format!("mqtt string exceeds 65535 bytes: {}", value.len(),))
    })?;
    let mut encoded = Vec::with_capacity(2 + bytes.len());
    encoded.extend_from_slice(&len.to_be_bytes());
    encoded.extend_from_slice(bytes);
    Ok(encoded)
}

pub(crate) fn decode_u16(bytes: &[u8], offset: &mut usize, field: &str) -> Result<u16, VmError> {
    if *offset + 2 > bytes.len() {
        return Err(VmError::HostError(format!("mqtt {field} is truncated",)));
    }
    let value = u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]);
    *offset += 2;
    Ok(value)
}

fn decode_utf8(bytes: &[u8], offset: &mut usize, field: &str) -> Result<String, VmError> {
    let len = decode_u16(bytes, offset, field)? as usize;
    if *offset + len > bytes.len() {
        return Err(VmError::HostError(format!("mqtt {field} is truncated",)));
    }
    let value = std::str::from_utf8(&bytes[*offset..*offset + len])
        .map_err(|err| VmError::HostError(format!("mqtt {field} is not valid utf-8: {err}")))?;
    *offset += len;
    Ok(value.to_string())
}

fn decode_properties_length(bytes: &[u8], offset: &mut usize) -> Result<usize, VmError> {
    let (length, encoded_len) = decode_variable_int(&bytes[*offset..])?
        .ok_or_else(|| VmError::HostError("mqtt properties length is truncated".to_string()))?;
    *offset += encoded_len;
    if *offset + length > bytes.len() {
        return Err(VmError::HostError(
            "mqtt properties are truncated".to_string(),
        ));
    }
    Ok(length)
}

fn encode_fixed_packet(header: u8, body: Vec<u8>) -> Result<Vec<u8>, VmError> {
    let mut packet = vec![header];
    packet.extend_from_slice(&encode_variable_int(body.len())?);
    packet.extend_from_slice(&body);
    Ok(packet)
}

pub(crate) fn encode_connect_packet(config: &MqttConnectConfig) -> Result<Vec<u8>, VmError> {
    let mut body = Vec::new();
    body.extend_from_slice(&encode_utf8("MQTT")?);
    body.push(0x05);

    let mut flags = 0u8;
    if config.clean_start {
        flags |= 0b0000_0010;
    }
    if config.username.is_some() {
        flags |= 0b1000_0000;
    }
    if config.password.is_some() {
        flags |= 0b0100_0000;
    }
    body.push(flags);
    body.extend_from_slice(&config.keep_alive_secs.to_be_bytes());
    body.push(0);
    body.extend_from_slice(&encode_utf8(&config.client_id)?);
    if let Some(username) = &config.username {
        body.extend_from_slice(&encode_utf8(username)?);
    }
    if let Some(password) = &config.password {
        body.extend_from_slice(&encode_utf8(password)?);
    }
    encode_fixed_packet(0x10, body)
}

pub(crate) fn encode_publish_packet(
    topic: &str,
    payload: &[u8],
    qos: u8,
    retain: bool,
    packet_id: Option<u16>,
) -> Result<Vec<u8>, VmError> {
    let mut body = Vec::new();
    body.extend_from_slice(&encode_utf8(topic)?);
    if let Some(packet_id) = packet_id {
        body.extend_from_slice(&packet_id.to_be_bytes());
    }
    body.push(0);
    body.extend_from_slice(payload);
    let mut header = 0b0011_0000;
    header |= (qos & 0x03) << 1;
    if retain {
        header |= 0b0000_0001;
    }
    encode_fixed_packet(header, body)
}

pub(crate) fn encode_subscribe_packet(
    filter: &str,
    qos: u8,
    packet_id: u16,
) -> Result<Vec<u8>, VmError> {
    let mut body = Vec::new();
    body.extend_from_slice(&packet_id.to_be_bytes());
    body.push(0);
    body.extend_from_slice(&encode_utf8(filter)?);
    body.push(qos & 0x03);
    encode_fixed_packet(0x82, body)
}

pub(crate) fn encode_unsubscribe_packet(filter: &str, packet_id: u16) -> Result<Vec<u8>, VmError> {
    let mut body = Vec::new();
    body.extend_from_slice(&packet_id.to_be_bytes());
    body.push(0);
    body.extend_from_slice(&encode_utf8(filter)?);
    encode_fixed_packet(0xA2, body)
}

pub(crate) fn encode_puback_packet(packet_id: u16) -> Result<Vec<u8>, VmError> {
    let mut body = Vec::new();
    body.extend_from_slice(&packet_id.to_be_bytes());
    body.push(0x00);
    body.push(0x00);
    encode_fixed_packet(0x40, body)
}

pub(crate) fn encode_pingreq_packet() -> Result<Vec<u8>, VmError> {
    encode_fixed_packet(0xC0, Vec::new())
}

pub(crate) fn encode_disconnect_packet(
    reason_code: u8,
    reason_text: &str,
) -> Result<Vec<u8>, VmError> {
    if reason_code == 0 && reason_text.is_empty() {
        return Ok(vec![0xE0, 0x00]);
    }

    let mut body = vec![reason_code];
    let mut properties = Vec::new();
    if !reason_text.is_empty() {
        properties.push(0x1f);
        properties.extend_from_slice(&encode_utf8(reason_text)?);
    }
    body.extend_from_slice(&encode_variable_int(properties.len())?);
    body.extend_from_slice(&properties);
    encode_fixed_packet(0xE0, body)
}

pub(crate) fn decode_packet(bytes: &[u8]) -> Result<Option<(MqttIncomingPacket, usize)>, VmError> {
    if bytes.len() < 2 {
        return Ok(None);
    }
    let header = bytes[0];
    let packet_type = header >> 4;
    let flags = header & 0x0f;
    let Some((remaining_len, encoded_len)) = decode_variable_int(&bytes[1..])? else {
        return Ok(None);
    };
    let total_len = 1 + encoded_len + remaining_len;
    if bytes.len() < total_len {
        return Ok(None);
    }
    let body = &bytes[1 + encoded_len..total_len];
    let packet = match packet_type {
        2 => {
            if body.len() < 2 {
                return Err(VmError::HostError("mqtt connack is truncated".to_string()));
            }
            let mut offset = 2;
            let properties_len = decode_properties_length(body, &mut offset)?;
            if offset + properties_len != body.len() {
                return Err(VmError::HostError(
                    "mqtt connack properties are malformed".to_string(),
                ));
            }
            MqttIncomingPacket::ConnAck {
                reason_code: body[1],
            }
        }
        3 => {
            let retain = flags & 0x01 != 0;
            let qos = (flags >> 1) & 0x03;
            let dup = flags & 0x08 != 0;
            if qos > 1 {
                return Err(VmError::HostError(
                    "mqtt qos 2 is not supported in this milestone".to_string(),
                ));
            }
            let mut offset = 0usize;
            let topic = decode_utf8(body, &mut offset, "publish topic")?;
            let packet_id = if qos > 0 {
                Some(decode_u16(body, &mut offset, "publish packet id")?)
            } else {
                None
            };
            let properties_len = decode_properties_length(body, &mut offset)?;
            offset += properties_len;
            if offset > body.len() {
                return Err(VmError::HostError(
                    "mqtt publish properties are malformed".to_string(),
                ));
            }
            MqttIncomingPacket::Publish {
                topic,
                payload: body[offset..].to_vec(),
                qos,
                retain,
                dup,
                packet_id,
            }
        }
        4 => {
            let mut offset = 0usize;
            let packet_id = decode_u16(body, &mut offset, "puback packet id")?;
            let reason_code = if offset < body.len() {
                let reason = body[offset];
                offset += 1;
                reason
            } else {
                0
            };
            if offset < body.len() {
                let properties_len = decode_properties_length(body, &mut offset)?;
                offset += properties_len;
                if offset != body.len() {
                    return Err(VmError::HostError(
                        "mqtt puback properties are malformed".to_string(),
                    ));
                }
            }
            MqttIncomingPacket::PubAck {
                packet_id,
                reason_code,
            }
        }
        9 => {
            let mut offset = 0usize;
            let packet_id = decode_u16(body, &mut offset, "suback packet id")?;
            let properties_len = decode_properties_length(body, &mut offset)?;
            offset += properties_len;
            if offset > body.len() {
                return Err(VmError::HostError(
                    "mqtt suback properties are malformed".to_string(),
                ));
            }
            MqttIncomingPacket::SubAck {
                packet_id,
                reason_codes: body[offset..].to_vec(),
            }
        }
        11 => {
            let mut offset = 0usize;
            let packet_id = decode_u16(body, &mut offset, "unsuback packet id")?;
            let properties_len = decode_properties_length(body, &mut offset)?;
            offset += properties_len;
            if offset > body.len() {
                return Err(VmError::HostError(
                    "mqtt unsuback properties are malformed".to_string(),
                ));
            }
            MqttIncomingPacket::UnsubAck {
                packet_id,
                reason_codes: body[offset..].to_vec(),
            }
        }
        13 => MqttIncomingPacket::PingResp,
        14 => {
            let reason_code = body.first().copied().unwrap_or(0);
            MqttIncomingPacket::Disconnect { reason_code }
        }
        _ => {
            return Err(VmError::HostError(format!(
                "unsupported mqtt control packet type {packet_type}",
            )));
        }
    };
    Ok(Some((packet, total_len)))
}
