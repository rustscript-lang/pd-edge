//! Function-local raw-handle metadata for edge host descriptors.
//!
//! Edge handles travel as `int` wire tokens owned by runtime scope state, not
//! as catalog resources. Guest `HostFunctionSchema` therefore cannot carry
//! resource effects for them without changing the published ABI. This module
//! is the bounded edge descriptor-extension layer keyed to the same canonical
//! function names as the ABI catalog: every raw-handle argument and return is
//! declared with an explicit family and mode (borrow / borrow-mut / take /
//! create / runtime-owned pending).
//!
//! The table is the all-features surface. Feature-disabled families stay
//! present here so completeness tests are never vacuously empty.

use vm::host_extension::{HostAdapterDescriptor, HostBindingKind, HostFunctionDescriptor};

#[cfg(test)]
use edge_abi::{AbiFunction, AbiParamType, AbiValueType};

/// Access mode of one raw handle token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawHandleMode {
    /// Read-only use of an existing handle.
    Borrow,
    /// Mutating use of an existing handle.
    BorrowMut,
    /// Consuming close / disconnect of an existing handle.
    Take,
    /// Creation of a new runtime-owned handle token.
    Create,
    /// The function captures a runtime-owned pending operation on the handle.
    RuntimeOwnedPending,
}

/// One handle-bearing slot on a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawHandleSlot {
    /// Guest argument at `index`.
    Arg(usize),
    /// Guest return value.
    Return,
}

/// One explicit raw-handle effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RawHandleEffect {
    pub function: &'static str,
    pub slot: RawHandleSlot,
    pub family: &'static str,
    pub mode: RawHandleMode,
}

/// Handle-owning families. Feature-disabled families remain listed.
#[cfg(test)]
pub(crate) const RAW_HANDLE_FAMILIES: &[&str] = &[
    "tcp::",
    "udp::",
    "tls::",
    "websocket::",
    "mqtt::",
    "webrtc::",
    "proxy::",
    "http::exchange::",
    "http::response::",
];

/// All-features raw-handle metadata, keyed by canonical ABI function name.
pub(crate) const RAW_HANDLE_EFFECTS: &[RawHandleEffect] = &[
    RawHandleEffect {
        function: "http::response::apply_exchange",
        slot: RawHandleSlot::Arg(0),
        family: "http::response::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::response::apply_exchange_with_headers",
        slot: RawHandleSlot::Arg(0),
        family: "http::response::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::new",
        slot: RawHandleSlot::Return,
        family: "http::exchange::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "http::exchange::default_upstream",
        slot: RawHandleSlot::Return,
        family: "http::exchange::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "http::exchange::prepare_default_upstream",
        slot: RawHandleSlot::Return,
        family: "http::exchange::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "http::exchange::send",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_header",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_method",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_path",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_query",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_version",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::get_version",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::set_target",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_scheme",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::attach_tcp",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::attach_tcp",
        slot: RawHandleSlot::Arg(1),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::attach_tls_plaintext",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::attach_tls_plaintext",
        slot: RawHandleSlot::Arg(1),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::set_body",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::add_header",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::clear_header",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::set_query_arg",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "http::exchange::get_status",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_header",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_headers",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_body",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_trailer",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_trailers",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::get_http_version",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::body::next_chunk",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "http::exchange::body::eof",
        slot: RawHandleSlot::Arg(0),
        family: "http::exchange::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::downstream",
        slot: RawHandleSlot::Return,
        family: "tcp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "tcp::stream::default_upstream",
        slot: RawHandleSlot::Return,
        family: "tcp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "tcp::stream::new",
        slot: RawHandleSlot::Return,
        family: "tcp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "tcp::stream::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::bind",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::set_target",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::connect",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::get_local_addr",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::get_peer_addr",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::read",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::read_binary",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::read_exact_binary",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::peek",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::peek_binary",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::write",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::write_binary",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tcp::stream::eof",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tcp::stream::close",
        slot: RawHandleSlot::Arg(0),
        family: "tcp::",
        mode: RawHandleMode::Take,
    },
    RawHandleEffect {
        function: "udp::socket::new",
        slot: RawHandleSlot::Return,
        family: "udp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "udp::socket::downstream",
        slot: RawHandleSlot::Return,
        family: "udp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "udp::socket::default_upstream",
        slot: RawHandleSlot::Return,
        family: "udp::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "udp::socket::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::bind",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::set_target",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::connect",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::get_local_addr",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::get_peer_addr",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::send_text",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::recv_text",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::send_binary",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::recv_binary",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::send_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "udp::socket::recv_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "udp::socket::close",
        slot: RawHandleSlot::Arg(0),
        family: "udp::",
        mode: RawHandleMode::Take,
    },
    RawHandleEffect {
        function: "tls::session::from_socket",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::from_socket",
        slot: RawHandleSlot::Return,
        family: "tls::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "tls::session::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::needs_configuration",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::handshake",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_alpn",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_verify",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_verify_hostname",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_trusted_certificate",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_client_certificate",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_client_private_key",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_server_certificate",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_server_private_key",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_sni",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_min_version",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::set_max_version",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "tls::session::get_peer_name",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::get_server_name",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::get_alpn",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::get_peer_certificate",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "tls::session::is_session_reused",
        slot: RawHandleSlot::Arg(0),
        family: "tls::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "mqtt::connection::new",
        slot: RawHandleSlot::Return,
        family: "mqtt::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "mqtt::connection::default_upstream",
        slot: RawHandleSlot::Return,
        family: "mqtt::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "mqtt::connection::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_scheme",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_target",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_client_id",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_username",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_password",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_keep_alive_secs",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::set_clean_start",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::connect",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "mqtt::connection::disconnect",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::Take,
    },
    RawHandleEffect {
        function: "mqtt::connection::publish_text",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::publish_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::publish_binary",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::subscribe",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::unsubscribe",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "mqtt::connection::read_event",
        slot: RawHandleSlot::Arg(0),
        family: "mqtt::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::new",
        slot: RawHandleSlot::Return,
        family: "websocket::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "websocket::connection::downstream",
        slot: RawHandleSlot::Return,
        family: "websocket::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "websocket::connection::default_upstream",
        slot: RawHandleSlot::Return,
        family: "websocket::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "websocket::connection::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::set_target",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::set_scheme",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::set_path",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::set_query",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::set_header",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::set_subprotocols",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::connect",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::get_subprotocol",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::send_text",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::read_text",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::send_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::send_binary",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "websocket::connection::read_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::read_binary",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::eof",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "websocket::connection::close",
        slot: RawHandleSlot::Arg(0),
        family: "websocket::",
        mode: RawHandleMode::Take,
    },
    RawHandleEffect {
        function: "webrtc::connection::new",
        slot: RawHandleSlot::Return,
        family: "webrtc::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "webrtc::connection::downstream",
        slot: RawHandleSlot::Return,
        family: "webrtc::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "webrtc::connection::default_upstream",
        slot: RawHandleSlot::Return,
        family: "webrtc::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "webrtc::connection::is_present",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::set_ice_servers",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::set_data_channel_label",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::set_remote_description",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::create_offer",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::create_answer",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::connect",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::get_phase",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::send_text",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::read_text",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::send_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::send_binary",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "webrtc::connection::read_binary_base64",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::read_binary",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::eof",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "webrtc::connection::close",
        slot: RawHandleSlot::Arg(0),
        family: "webrtc::",
        mode: RawHandleMode::Take,
    },
    RawHandleEffect {
        function: "proxy::stream::downstream",
        slot: RawHandleSlot::Return,
        family: "proxy::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "proxy::stream::exchange",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "proxy::stream::exchange",
        slot: RawHandleSlot::Return,
        family: "proxy::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "proxy::stream::from_tcp",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "proxy::stream::from_tcp",
        slot: RawHandleSlot::Return,
        family: "proxy::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "proxy::stream::from_tls_plaintext",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "proxy::stream::from_tls_plaintext",
        slot: RawHandleSlot::Return,
        family: "proxy::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "proxy::stream::from_websocket_binary",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::Borrow,
    },
    RawHandleEffect {
        function: "proxy::stream::from_websocket_binary",
        slot: RawHandleSlot::Return,
        family: "proxy::",
        mode: RawHandleMode::Create,
    },
    RawHandleEffect {
        function: "proxy::pipe",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "proxy::pipe",
        slot: RawHandleSlot::Arg(1),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "proxy::forward",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "proxy::forward",
        slot: RawHandleSlot::Arg(1),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "proxy::forward_native",
        slot: RawHandleSlot::Arg(0),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
    RawHandleEffect {
        function: "proxy::forward_native",
        slot: RawHandleSlot::Arg(1),
        family: "proxy::",
        mode: RawHandleMode::BorrowMut,
    },
];

/// Effects declared for `function`.
pub(crate) fn raw_handle_effects_for(
    function: &str,
) -> impl Iterator<Item = &'static RawHandleEffect> {
    RAW_HANDLE_EFFECTS
        .iter()
        .filter(move |effect| effect.function == function)
}

/// Effects declared for one family prefix.
#[cfg(test)]
pub(crate) fn raw_handle_effects_for_family(
    family: &str,
) -> impl Iterator<Item = &'static RawHandleEffect> {
    RAW_HANDLE_EFFECTS
        .iter()
        .filter(move |effect| effect.family == family)
}

/// Whether `function` is a raw-handle ABI entry (has at least one declared effect).
#[cfg(test)]
pub(crate) fn is_raw_handle_function(name: &str) -> bool {
    RAW_HANDLE_EFFECTS
        .iter()
        .any(|effect| effect.function == name)
}

/// Rejects a descriptor set whose raw-handle metadata is incoherent.
///
/// Missing, extra, wrong-slot, or pending-capture-without-declaration entries
/// fail before any registry mutation. Functions not in this descriptor set are
/// ignored so a scope install can be a subset of the all-features table.
pub(crate) fn validate_raw_handle_metadata(
    descriptors: &[HostFunctionDescriptor],
) -> Result<(), String> {
    for descriptor in descriptors {
        let name = descriptor.schema.name.as_str();
        if matches!(
            (&descriptor.binding.kind, &descriptor.adapter),
            (
                HostBindingKind::StaticStackRuntimeOwned,
                HostAdapterDescriptor::StaticStackRuntimeOwned(_)
            )
        ) && !raw_handle_effects_for(name)
            .any(|effect| effect.mode == RawHandleMode::RuntimeOwnedPending)
        {
            return Err(format!(
                "edge host function '{name}' captures runtime-owned pending without raw-handle metadata"
            ));
        }
        if raw_handle_effects_for(name)
            .any(|effect| effect.mode == RawHandleMode::RuntimeOwnedPending)
            && !matches!(
                descriptor.binding.kind,
                HostBindingKind::StaticStackRuntimeOwned
            )
        {
            return Err(format!(
                "edge host function '{name}' declares runtime-owned pending metadata without a matching adapter"
            ));
        }
    }
    Ok(())
}

/// Whether an ABI function sits on a raw runtime-owned handle boundary.
#[cfg(test)]
pub(crate) fn is_raw_handle_boundary(function: &AbiFunction) -> bool {
    is_raw_handle_function(function.name)
}

#[cfg(test)]
fn slot_type_is_int(function: &AbiFunction, slot: RawHandleSlot) -> bool {
    match slot {
        RawHandleSlot::Arg(index) => function.param_types.get(index) == Some(&AbiParamType::Int),
        RawHandleSlot::Return => function.return_type == AbiValueType::Int,
    }
}

/// Completeness of the static table against a compiled ABI function list.
#[cfg(test)]
pub(crate) fn raw_handle_metadata_mismatches(functions: &[AbiFunction]) -> Vec<String> {
    let mut mismatches = Vec::new();
    let compiled: std::collections::BTreeSet<&str> =
        functions.iter().map(|function| function.name).collect();
    let mut seen = std::collections::BTreeSet::new();
    for function in functions {
        let declared: Vec<_> = raw_handle_effects_for(function.name).collect();
        if declared.is_empty() {
            if is_candidate_handle_function(function) {
                mismatches.push(format!(
                    "missing raw-handle metadata for '{}'",
                    function.name
                ));
            }
            continue;
        }
        seen.insert(function.name);
        for effect in declared {
            if !RAW_HANDLE_FAMILIES.contains(&effect.family) {
                mismatches.push(format!(
                    "unknown raw-handle family '{}' on '{}'",
                    effect.family, function.name
                ));
            }
            if !function.name.starts_with(effect.family) {
                mismatches.push(format!(
                    "raw-handle family '{}' does not prefix '{}'",
                    effect.family, function.name
                ));
            }
            if !slot_type_is_int(function, effect.slot) {
                mismatches.push(format!(
                    "raw-handle slot {:?} on '{}' is not an int token",
                    effect.slot, function.name
                ));
            }
            if matches!(effect.mode, RawHandleMode::Take)
                && !matches!(effect.slot, RawHandleSlot::Arg(_))
            {
                mismatches.push(format!(
                    "take/close mode on '{}' must name an argument, not a return",
                    function.name
                ));
            }
            if matches!(effect.mode, RawHandleMode::Create)
                && !matches!(effect.slot, RawHandleSlot::Return)
            {
                mismatches.push(format!(
                    "create mode on '{}' must name the return slot",
                    function.name
                ));
            }
        }
    }
    for effect in RAW_HANDLE_EFFECTS {
        if compiled.contains(effect.function) {
            continue;
        }
        // Feature-disabled functions stay in the static table; they are not extra.
        if !RAW_HANDLE_FAMILIES.contains(&effect.family) {
            mismatches.push(format!(
                "unknown raw-handle family '{}' on absent function '{}'",
                effect.family, effect.function
            ));
        }
    }
    let _ = seen;
    mismatches
}

#[cfg(test)]
fn is_candidate_handle_function(function: &AbiFunction) -> bool {
    if function.name.starts_with("http::response::apply_exchange") {
        return true;
    }
    RAW_HANDLE_FAMILIES
        .iter()
        .any(|family| family != &"http::response::" && function.name.starts_with(family))
        && (function.return_type == AbiValueType::Int
            || function.param_types.contains(&AbiParamType::Int))
        && !is_plain_counter(function)
}

#[cfg(test)]
fn is_plain_counter(function: &AbiFunction) -> bool {
    // Status/byte-count returns without a handle argument are not tokens.
    function
        .param_types
        .iter()
        .all(|ty| *ty != AbiParamType::Int)
        && function.return_type == AbiValueType::Int
}
