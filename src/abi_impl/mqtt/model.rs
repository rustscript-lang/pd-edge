use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use uuid::Uuid;
use vm::{Value, VmError};

const DEFAULT_KEEP_ALIVE_SECS: u16 = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MqttScheme {
    #[default]
    Mqtt,
    #[cfg(feature = "tls")]
    Mqtts,
}

impl MqttScheme {
    pub(crate) fn parse(value: &str) -> Result<Self, VmError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mqtt" => Ok(Self::Mqtt),
            #[cfg(feature = "tls")]
            "mqtts" => Ok(Self::Mqtts),
            "ws" | "wss" => Err(VmError::HostError(
                "mqtt over websocket is planned separately and is not available in this milestone"
                    .to_string(),
            )),
            _ => Err(VmError::HostError(format!(
                "invalid mqtt scheme '{value}'; expected mqtt{}",
                if cfg!(feature = "tls") {
                    " or mqtts"
                } else {
                    ""
                },
            ))),
        }
    }

    pub(crate) fn default_port(self) -> u16 {
        match self {
            Self::Mqtt => 1883,
            #[cfg(feature = "tls")]
            Self::Mqtts => 8883,
        }
    }

    pub(crate) fn uses_tls(self) -> bool {
        match self {
            Self::Mqtt => false,
            #[cfg(feature = "tls")]
            Self::Mqtts => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MqttPhase {
    #[default]
    Inactive,
    Configured,
    CarrierAttached,
    ConnectSent,
    Open,
    Closed,
    Failed,
}

impl MqttPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Configured => "configured",
            Self::CarrierAttached => "carrier-attached",
            Self::ConnectSent => "connect-sent",
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MqttCarrierAttachment {
    Tcp {
        stream: i64,
    },
    #[cfg(feature = "tls")]
    Tls {
        session: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MqttEvent {
    Publish {
        topic: String,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
        dup: bool,
    },
    Closed {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl MqttEvent {
    /// Encodes the event as the closed `MqttEvent` record.
    ///
    /// Every published member is always present. A member the active variant
    /// does not carry is the canonical guest `Option::None` carrier, so a
    /// guest distinguishes "this variant has no topic" from "the topic is the
    /// empty string" without relying on a missing map entry.
    pub(crate) fn into_value(self) -> Value {
        fn some(value: Value) -> Value {
            vm::option::some_carrier(value)
        }
        fn none() -> Value {
            vm::option::none_carrier()
        }
        match self {
            Self::Publish {
                topic,
                payload,
                qos,
                retain,
                dup,
            } => Value::map(vec![
                (Value::string("kind"), Value::string("publish")),
                (Value::string("topic"), some(Value::string(topic))),
                (
                    Value::string("payload_text"),
                    some(Value::string(
                        String::from_utf8_lossy(&payload).into_owned(),
                    )),
                ),
                (
                    Value::string("payload_base64"),
                    some(Value::string(STANDARD.encode(payload))),
                ),
                (Value::string("qos"), some(Value::Int(i64::from(qos)))),
                (Value::string("retain"), some(Value::Bool(retain))),
                (Value::string("dup"), some(Value::Bool(dup))),
                (Value::string("reason"), none()),
            ]),
            Self::Closed { reason } => closed_event_value("closed", reason),
            Self::Failed { reason } => closed_event_value("failed", reason),
        }
    }
}

/// Encodes one reason-only event (`closed` | `failed`) as the closed record.
fn closed_event_value(kind: &str, reason: String) -> Value {
    Value::map(vec![
        (Value::string("kind"), Value::string(kind)),
        (Value::string("topic"), vm::option::none_carrier()),
        (Value::string("payload_text"), vm::option::none_carrier()),
        (Value::string("payload_base64"), vm::option::none_carrier()),
        (Value::string("qos"), vm::option::none_carrier()),
        (Value::string("retain"), vm::option::none_carrier()),
        (Value::string("dup"), vm::option::none_carrier()),
        (
            Value::string("reason"),
            vm::option::some_carrier(Value::string(reason)),
        ),
    ])
}

#[derive(Clone, Debug)]
pub(crate) struct MqttConnectConfig {
    pub(crate) scheme: MqttScheme,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) client_id: String,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) keep_alive_secs: u16,
    pub(crate) clean_start: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MqttConnectionState {
    phase: MqttPhase,
    present: bool,
    scheme: MqttScheme,
    host: Option<String>,
    port: Option<u16>,
    client_id: String,
    username: Option<String>,
    password: Option<String>,
    keep_alive_secs: u16,
    clean_start: bool,
    next_packet_id: u16,
    carrier: Option<MqttCarrierAttachment>,
    failure_message: Option<String>,
    last_activity_at: Option<Instant>,
    ping_response_pending: bool,
    pub(crate) pending_read_buffer: Vec<u8>,
    pub(crate) pending_events: VecDeque<MqttEvent>,
}

impl Default for MqttConnectionState {
    fn default() -> Self {
        Self {
            phase: MqttPhase::Inactive,
            present: false,
            scheme: MqttScheme::Mqtt,
            host: None,
            port: None,
            client_id: String::new(),
            username: None,
            password: None,
            keep_alive_secs: DEFAULT_KEEP_ALIVE_SECS,
            clean_start: true,
            next_packet_id: 1,
            carrier: None,
            failure_message: None,
            last_activity_at: None,
            ping_response_pending: false,
            pending_read_buffer: Vec::new(),
            pending_events: VecDeque::new(),
        }
    }
}

impl MqttConnectionState {
    pub(crate) fn configure(&mut self) {
        self.present = true;
        self.failure_message = None;
        if matches!(
            self.phase,
            MqttPhase::Inactive | MqttPhase::Closed | MqttPhase::Failed
        ) {
            self.phase = MqttPhase::Configured;
        }
    }

    pub(crate) fn set_scheme(&mut self, scheme: MqttScheme) {
        self.configure();
        self.scheme = scheme;
        if self.port.is_none() {
            self.port = Some(scheme.default_port());
        }
    }

    pub(crate) fn set_target(&mut self, host: String, port: u16) {
        self.configure();
        self.host = Some(host);
        self.port = Some(port);
    }

    pub(crate) fn set_client_id(&mut self, client_id: String) {
        self.configure();
        self.client_id = client_id;
    }

    pub(crate) fn set_username(&mut self, username: String) {
        self.configure();
        self.username = if username.is_empty() {
            None
        } else {
            Some(username)
        };
    }

    pub(crate) fn set_password(&mut self, password: String) {
        self.configure();
        self.password = if password.is_empty() {
            None
        } else {
            Some(password)
        };
    }

    pub(crate) fn set_keep_alive_secs(&mut self, keep_alive_secs: u16) {
        self.configure();
        self.keep_alive_secs = keep_alive_secs;
    }

    pub(crate) fn set_clean_start(&mut self, enabled: bool) {
        self.configure();
        self.clean_start = enabled;
    }

    pub(crate) fn attach_carrier(&mut self, carrier: MqttCarrierAttachment) {
        self.configure();
        self.carrier = Some(carrier);
        self.last_activity_at = None;
        self.ping_response_pending = false;
        self.pending_read_buffer.clear();
        self.pending_events.clear();
        self.phase = MqttPhase::CarrierAttached;
    }

    pub(crate) fn note_connect_sent(&mut self) {
        self.configure();
        self.phase = MqttPhase::ConnectSent;
    }

    pub(crate) fn mark_open(&mut self) {
        self.configure();
        self.failure_message = None;
        self.last_activity_at = Some(Instant::now());
        self.ping_response_pending = false;
        self.phase = MqttPhase::Open;
    }

    pub(crate) fn mark_closed(&mut self) {
        self.ping_response_pending = false;
        self.phase = MqttPhase::Closed;
    }

    pub(crate) fn mark_failed(&mut self, message: impl Into<String>) {
        self.present = true;
        self.phase = MqttPhase::Failed;
        self.failure_message = Some(message.into());
        self.ping_response_pending = false;
    }

    pub(crate) fn carrier(&self) -> Option<MqttCarrierAttachment> {
        self.carrier.clone()
    }

    pub(crate) fn take_carrier(&mut self) -> Option<MqttCarrierAttachment> {
        self.carrier.take()
    }

    pub(crate) fn phase(&self) -> MqttPhase {
        self.phase
    }

    pub(crate) fn is_present(&self) -> bool {
        self.present
    }

    pub(crate) fn failure_message(&self) -> Option<String> {
        self.failure_message.clone()
    }

    pub(crate) fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    pub(crate) fn port(&self) -> Option<u16> {
        self.port
    }

    pub(crate) fn scheme(&self) -> MqttScheme {
        self.scheme
    }

    pub(crate) fn keep_alive_secs(&self) -> u16 {
        self.keep_alive_secs
    }

    pub(crate) fn clean_start(&self) -> bool {
        self.clean_start
    }

    pub(crate) fn next_keep_alive_wait(&self) -> Option<Duration> {
        if self.phase != MqttPhase::Open || self.keep_alive_secs == 0 {
            return None;
        }
        let interval = Duration::from_secs(u64::from(self.keep_alive_secs));
        Some(match self.last_activity_at {
            Some(last_activity_at) => {
                interval.saturating_sub(Instant::now().saturating_duration_since(last_activity_at))
            }
            None => interval,
        })
    }

    pub(crate) fn ping_response_pending(&self) -> bool {
        self.ping_response_pending
    }

    pub(crate) fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub(crate) fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub(crate) fn ensure_client_id(&mut self) -> String {
        if self.client_id.is_empty() {
            self.client_id = format!("pd-edge-{}", Uuid::new_v4());
        }
        self.client_id.clone()
    }

    pub(crate) fn next_packet_id(&mut self) -> u16 {
        let next = if self.next_packet_id == 0 {
            1
        } else {
            self.next_packet_id
        };
        self.next_packet_id = if next == u16::MAX { 1 } else { next + 1 };
        next
    }

    pub(crate) fn note_packet_activity(&mut self) {
        self.last_activity_at = Some(Instant::now());
        self.ping_response_pending = false;
    }

    pub(crate) fn note_ping_request_sent(&mut self) {
        self.last_activity_at = Some(Instant::now());
        self.ping_response_pending = true;
    }

    pub(crate) fn note_ping_response_received(&mut self) {
        self.last_activity_at = Some(Instant::now());
        self.ping_response_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
        let Value::Map(entries) = value else {
            panic!("MqttEvent must be a map, got {value:?}");
        };
        entries
            .get(&Value::string(name))
            .unwrap_or_else(|| panic!("MqttEvent must always carry '{name}'"))
    }

    fn optional_text(value: &Value) -> Option<String> {
        let (some, payload) = vm::option::payload(value)
            .unwrap_or_else(|error| panic!("'{value:?}' must be a canonical Option: {error}"));
        if !some {
            return None;
        }
        match payload {
            Some(Value::String(text)) => Some(text.to_string()),
            other => panic!("Option::Some payload must be text, got {other:?}"),
        }
    }

    #[test]
    fn publish_event_carries_publish_members_and_no_reason() {
        let value = MqttEvent::Publish {
            topic: "pd-edge/topic".to_string(),
            payload: b"hello".to_vec(),
            qos: 1,
            retain: true,
            dup: false,
        }
        .into_value();

        assert_eq!(
            field(&value, "kind"),
            &Value::string("publish"),
            "the discriminator is always present"
        );
        assert_eq!(
            optional_text(field(&value, "topic")),
            Some("pd-edge/topic".to_string())
        );
        assert_eq!(
            optional_text(field(&value, "payload_text")),
            Some("hello".to_string())
        );
        assert!(optional_text(field(&value, "payload_base64")).is_some());
        assert_eq!(
            field(&value, "qos"),
            &vm::option::some_carrier(Value::Int(1))
        );
        assert_eq!(
            field(&value, "retain"),
            &vm::option::some_carrier(Value::Bool(true))
        );
        assert_eq!(
            field(&value, "dup"),
            &vm::option::some_carrier(Value::Bool(false))
        );
        assert_eq!(
            field(&value, "reason"),
            &vm::option::none_carrier(),
            "a publish event carries no reason member"
        );
    }

    #[test]
    fn closed_event_carries_only_its_reason_member() {
        let value = MqttEvent::Closed {
            reason: "broker closed".to_string(),
        }
        .into_value();

        assert_eq!(field(&value, "kind"), &Value::string("closed"));
        for absent in [
            "topic",
            "payload_text",
            "payload_base64",
            "qos",
            "retain",
            "dup",
        ] {
            assert_eq!(
                field(&value, absent),
                &vm::option::none_carrier(),
                "'{absent}' is absent on a closed event"
            );
        }
        assert_eq!(
            optional_text(field(&value, "reason")),
            Some("broker closed".to_string())
        );
    }
}
