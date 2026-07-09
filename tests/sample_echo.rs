#[cfg(feature = "http3")]
#[path = "support/http3_support.rs"]
mod http3_support;

#[cfg(any(feature = "http2", feature = "http3", feature = "mqtt"))]
use edge::sample_echo::{SampleEchoServerConfig, spawn_sample_echo_server};
use edge::sample_echo::{spawn_connect_forward_proxy, spawn_tcp_echo_server};
#[cfg(feature = "http2")]
use http_body_util::{BodyExt, Full};
#[cfg(feature = "http2")]
use hyper::Request;
#[cfg(feature = "http2")]
use hyper_util::rt::{TokioExecutor, TokioIo};
#[cfg(all(feature = "tls", feature = "http2"))]
use reqwest::{StatusCode, Version};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::{Duration, timeout},
};

#[tokio::test]
async fn spawn_connect_forward_proxy_tunnels_bytes_after_connect() {
    let (upstream_addr, upstream_handle) =
        spawn_tcp_echo_server("127.0.0.1:0".parse().expect("valid addr"))
            .await
            .expect("tcp echo should start");
    let (proxy_addr, proxy_handle) =
        spawn_connect_forward_proxy("127.0.0.1:0".parse().expect("valid addr"))
            .await
            .expect("forward proxy should start");

    let mut client = timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(proxy_addr),
    )
    .await
    .expect("proxy connect timed out")
    .expect("proxy should accept connections");
    let connect_request =
        format!("CONNECT {upstream_addr} HTTP/1.1\r\nHost: {upstream_addr}\r\n\r\n");
    client
        .write_all(connect_request.as_bytes())
        .await
        .expect("connect request should write");

    let mut connect_response = [0u8; 256];
    let read = timeout(Duration::from_secs(2), client.read(&mut connect_response))
        .await
        .expect("connect response timed out")
        .expect("connect response should read");
    let connect_response = String::from_utf8_lossy(&connect_response[..read]);
    assert!(
        connect_response.starts_with("HTTP/1.1 200 Connection Established"),
        "unexpected CONNECT response: {connect_response}"
    );

    client
        .write_all(b"hello-through-proxy")
        .await
        .expect("payload should write");

    let mut echoed = [0u8; 64];
    let read = timeout(Duration::from_secs(2), client.read(&mut echoed))
        .await
        .expect("echo read timed out")
        .expect("echo should read");
    assert_eq!(&echoed[..read], b"hello-through-proxy");

    proxy_handle.abort();
    upstream_handle.abort();
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn sample_echo_server_http_listener_accepts_cleartext_http2_prior_knowledge() {
    let server = spawn_sample_echo_server(zero_addr_sample_echo_config())
        .await
        .expect("sample echo server should start");
    let addr = server
        .addresses
        .http
        .expect("http listener should be present");

    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("http2 client should connect");
    let io = TokioIo::new(stream);
    let (mut sender, connection) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
        .handshake(io)
        .await
        .expect("http2 client handshake should succeed");
    let connection_handle = tokio::spawn(async move {
        connection
            .await
            .expect("http2 client connection should run");
    });

    let host = addr.to_string();
    let request = Request::builder()
        .method("POST")
        .uri(format!("http://{addr}/echo"))
        .version(hyper::Version::HTTP_2)
        .header("host", &host)
        .body(Full::new(axum::body::Bytes::from_static(b"h2c-body")))
        .expect("http2 request should build");
    let response = sender
        .send_request(request)
        .await
        .expect("http2 request should complete");
    assert_eq!(response.status(), hyper::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-echo-http-version")
            .and_then(|value| value.to_str().ok()),
        Some("2")
    );
    assert_eq!(
        response
            .headers()
            .get("x-echo-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("http")
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("http2 response body should collect")
        .to_bytes();
    assert_eq!(body.as_ref(), b"h2c-body");

    connection_handle.abort();
    drop(server);
}

#[cfg(all(feature = "tls", feature = "http2"))]
#[tokio::test]
async fn sample_echo_server_https_listener_negotiates_http2_over_tls() {
    let server = spawn_sample_echo_server(zero_addr_sample_echo_config())
        .await
        .expect("sample echo server should start");
    let addr = server
        .addresses
        .https
        .expect("https listener should be present");

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("https h2 client should build");
    let response = client
        .post(format!("https://localhost:{}/echo", addr.port()))
        .body("tls-h2-body")
        .send()
        .await
        .expect("https h2 request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.version(), Version::HTTP_2);
    assert_eq!(
        response
            .headers()
            .get("x-echo-http-version")
            .and_then(|value| value.to_str().ok()),
        Some("2")
    );
    assert_eq!(
        response
            .headers()
            .get("x-echo-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("https")
    );
    assert_eq!(
        response.text().await.expect("https h2 body should read"),
        "tls-h2-body"
    );

    drop(server);
}

#[cfg(feature = "http3")]
#[tokio::test]
async fn sample_echo_server_http3_listener_serves_http3_requests() {
    let server = spawn_sample_echo_server(zero_addr_sample_echo_config())
        .await
        .expect("sample echo server should start");
    let addr = server
        .addresses
        .http3
        .expect("http3 listener should be present");

    let response = http3_support::send_http3_request(
        &format!("https://127.0.0.1:{}/echo?mode=http3", addr.port()),
        "POST",
        &[],
        b"quic-body",
    )
    .await;
    assert_eq!(response.status, axum::http::StatusCode::OK);
    assert_eq!(response.version, axum::http::Version::HTTP_3);
    assert_eq!(
        response
            .headers
            .get("x-echo-http-version")
            .and_then(|value| value.to_str().ok()),
        Some("3")
    );
    assert_eq!(
        response
            .headers
            .get("x-echo-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("http3")
    );
    assert_eq!(
        response
            .headers
            .get("x-echo-method")
            .and_then(|value| value.to_str().ok()),
        Some("POST")
    );
    assert_eq!(
        response
            .headers
            .get("x-echo-path")
            .and_then(|value| value.to_str().ok()),
        Some("/echo")
    );
    assert_eq!(response.body.as_ref(), b"quic-body");

    drop(server);
}

#[cfg(feature = "mqtt")]
#[tokio::test]
async fn sample_echo_server_mqtt_listener_echoes_subscribed_publish() {
    let server = spawn_sample_echo_server(zero_addr_sample_echo_config())
        .await
        .expect("sample echo server should start");
    let addr = server
        .addresses
        .mqtt
        .expect("mqtt listener should be present");

    let mut stream = timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr))
        .await
        .expect("mqtt connect timed out")
        .expect("mqtt listener should accept connections");

    stream
        .write_all(&encode_connect_packet("sample-echo-mqtt-test"))
        .await
        .expect("connect should write");
    let connack = read_packet(&mut stream).await.expect("connack should read");
    assert_eq!(connack, vec![0x20, 0x03, 0x00, 0x00, 0x00]);

    stream
        .write_all(&encode_subscribe_packet("sensor/#", 0, 1))
        .await
        .expect("subscribe should write");
    let suback = read_packet(&mut stream).await.expect("suback should read");
    assert_eq!(suback, vec![0x90, 0x04, 0x00, 0x01, 0x00, 0x00]);

    stream
        .write_all(&encode_publish_packet(
            "sensor/temp",
            b"21.5",
            1,
            false,
            Some(7),
        ))
        .await
        .expect("publish should write");
    let puback = read_packet(&mut stream).await.expect("puback should read");
    assert_eq!(puback, vec![0x40, 0x04, 0x00, 0x07, 0x00, 0x00]);
    let publish = read_packet(&mut stream)
        .await
        .expect("echo publish should read");
    let (topic, payload) = decode_publish_packet(&publish);
    assert_eq!(topic, "sensor/temp");
    assert_eq!(payload, b"21.5");

    stream
        .write_all(&[0xE0, 0x00])
        .await
        .expect("disconnect should write");

    drop(server);
}

#[cfg(any(feature = "http2", feature = "http3", feature = "mqtt"))]
fn zero_addr_sample_echo_config() -> SampleEchoServerConfig {
    let any = "127.0.0.1:0".parse().expect("valid wildcard addr");
    SampleEchoServerConfig {
        tcp_addr: any,
        udp_addr: any,
        tls_addr: any,
        http_addr: any,
        https_addr: any,
        http3_addr: any,
        websocket_addr: any,
        websocket_tls_addr: any,
        mqtt_addr: any,
        mqtts_addr: any,
        webrtc_addr: any,
        forward_proxy_addr: any,
    }
}

#[cfg(feature = "mqtt")]
async fn read_packet(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut first = [0u8; 1];
    stream.read_exact(&mut first).await?;
    let mut encoded_len = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        encoded_len.push(byte[0]);
        if byte[0] & 0x80 == 0 {
            break;
        }
    }
    let (remaining_len, _) = decode_variable_int(&encoded_len).expect("remaining length");
    let mut body = vec![0u8; remaining_len];
    stream.read_exact(&mut body).await?;
    let mut packet = vec![first[0]];
    packet.extend_from_slice(&encoded_len);
    packet.extend_from_slice(&body);
    Ok(packet)
}

#[cfg(feature = "mqtt")]
fn decode_variable_int(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut multiplier = 1usize;
    let mut value = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate().take(4) {
        value += usize::from(byte & 0x7f) * multiplier;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        multiplier *= 128;
    }
    None
}

#[cfg(feature = "mqtt")]
fn encode_variable_int(mut value: usize) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value % 128) as u8;
        value /= 128;
        if value > 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

#[cfg(feature = "mqtt")]
fn encode_utf8_field(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let len = u16::try_from(bytes.len()).expect("mqtt field should fit u16");
    let mut encoded = Vec::with_capacity(bytes.len() + 2);
    encoded.extend_from_slice(&len.to_be_bytes());
    encoded.extend_from_slice(bytes);
    encoded
}

#[cfg(feature = "mqtt")]
fn encode_connect_packet(client_id: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&encode_utf8_field("MQTT"));
    body.push(0x05);
    body.push(0x02);
    body.extend_from_slice(&30u16.to_be_bytes());
    body.push(0x00);
    body.extend_from_slice(&encode_utf8_field(client_id));
    let mut packet = vec![0x10];
    packet.extend_from_slice(&encode_variable_int(body.len()));
    packet.extend_from_slice(&body);
    packet
}

#[cfg(feature = "mqtt")]
fn encode_subscribe_packet(filter: &str, qos: u8, packet_id: u16) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&packet_id.to_be_bytes());
    body.push(0x00);
    body.extend_from_slice(&encode_utf8_field(filter));
    body.push(qos & 0x03);
    let mut packet = vec![0x82];
    packet.extend_from_slice(&encode_variable_int(body.len()));
    packet.extend_from_slice(&body);
    packet
}

#[cfg(feature = "mqtt")]
fn encode_publish_packet(
    topic: &str,
    payload: &[u8],
    qos: u8,
    retain: bool,
    packet_id: Option<u16>,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&encode_utf8_field(topic));
    if let Some(packet_id) = packet_id {
        body.extend_from_slice(&packet_id.to_be_bytes());
    }
    body.push(0x00);
    body.extend_from_slice(payload);
    let mut packet = vec![0x30 | ((qos & 0x03) << 1)];
    if retain {
        packet[0] |= 0x01;
    }
    packet.extend_from_slice(&encode_variable_int(body.len()));
    packet.extend_from_slice(&body);
    packet
}

#[cfg(feature = "mqtt")]
fn decode_publish_packet(packet: &[u8]) -> (String, Vec<u8>) {
    let (_, encoded_len) =
        decode_variable_int(&packet[1..]).expect("remaining length should decode");
    let body = &packet[1 + encoded_len..];
    let mut offset = 0usize;
    let topic_len = u16::from_be_bytes([body[offset], body[offset + 1]]) as usize;
    offset += 2;
    let topic = std::str::from_utf8(&body[offset..offset + topic_len])
        .expect("topic should be utf8")
        .to_string();
    offset += topic_len;
    if ((packet[0] >> 1) & 0x03) > 0 {
        offset += 2;
    }
    offset += 1;
    (topic, body[offset..].to_vec())
}
