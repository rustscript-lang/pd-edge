use pd_edge_host_function::pd_edge_host_function;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use vm::{CallOutcome, Value, Vm, VmError};

use super::http::state::{
    append_outbound_exchange_body, append_response_output_body_bytes, consume_request_body_all,
    default_upstream_exchange_handle, read_outbound_exchange_response_all,
    read_outbound_exchange_response_next_line, read_request_body_next_line,
    read_upstream_response_all, read_upstream_response_next_line,
};
use super::{
    EDGE_IO_HANDLE_DYNAMIC_BASE, EdgeVirtualIoHandle, ProxyVmContext, SharedProxyVmContext,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeProtocolIoHandle {
    Downstream,
    OutboundExchange(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeIoReadSource {
    Protocol(EdgeProtocolIoHandle),
    VirtualHandle(i64),
}

#[derive(Clone, Debug)]
enum EdgeIoWriteTarget {
    Protocol(EdgeProtocolIoHandle),
    FilePath { path: String, append: bool },
}

fn decode_protocol_io_handle(
    context: &ProxyVmContext,
    handle: i64,
) -> Option<EdgeProtocolIoHandle> {
    if handle == 0 {
        return Some(EdgeProtocolIoHandle::Downstream);
    }
    let exchanges = context.lock_exchanges();
    if handle == default_upstream_exchange_handle() || exchanges.exchanges.contains_key(&handle) {
        return Some(EdgeProtocolIoHandle::OutboundExchange(handle));
    }
    None
}

fn invalid_io_handle_error(handle: i64) -> VmError {
    VmError::HostError(format!(
        "edge io handle {handle} is invalid; use io::open for file handles or pass a handle returned by http/tcp/tls",
    ))
}

fn requires_io_handle_error(function_name: &str) -> VmError {
    VmError::HostError(format!(
        "edge {function_name} requires a handle; use io::open for files or pass a handle returned by http/tcp/tls",
    ))
}

fn resolve_edge_io_read_source(
    context: &ProxyVmContext,
    value: &Value,
    function_name: &str,
) -> Result<EdgeIoReadSource, VmError> {
    match value {
        Value::Int(handle) => {
            if let Some(target) = decode_protocol_io_handle(context, *handle) {
                return Ok(EdgeIoReadSource::Protocol(target));
            }
            if context.lock_edge_io().handles.contains_key(handle) {
                return Ok(EdgeIoReadSource::VirtualHandle(*handle));
            }
            Err(invalid_io_handle_error(*handle))
        }
        Value::String(_) => Err(requires_io_handle_error(function_name)),
        _ => Err(VmError::TypeMismatch("int")),
    }
}

fn allocate_edge_virtual_io_handle(context: &ProxyVmContext, handle: EdgeVirtualIoHandle) -> i64 {
    let mut edge_io = context.lock_edge_io();
    let handle_id = edge_io.next_handle.max(EDGE_IO_HANDLE_DYNAMIC_BASE);
    edge_io.next_handle = handle_id.saturating_add(1);
    edge_io.handles.insert(handle_id, handle);
    handle_id
}

fn read_edge_virtual_handle_all(context: &ProxyVmContext, handle: i64) -> Result<String, VmError> {
    match context.lock_edge_io().handles.get_mut(&handle) {
        Some(EdgeVirtualIoHandle::BufferedRead { text, offset }) => {
            *offset = text.len();
            Ok(text.clone())
        }
        Some(EdgeVirtualIoHandle::FileWrite { .. }) => Err(VmError::HostError(format!(
            "edge io handle {handle} is write-only",
        ))),
        None => Err(VmError::HostError(format!(
            "edge io handle {handle} is invalid",
        ))),
    }
}

fn read_edge_virtual_handle_line(
    context: &ProxyVmContext,
    handle: i64,
) -> Result<Option<String>, VmError> {
    match context.lock_edge_io().handles.get_mut(&handle) {
        Some(EdgeVirtualIoHandle::BufferedRead { text, offset }) => {
            let start = (*offset).min(text.len());
            if start >= text.len() {
                return Ok(None);
            }
            let bytes = text.as_bytes();
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'\n' {
                end += 1;
            }
            let line = text[start..end].to_string();
            if end < bytes.len() && bytes[end] == b'\n' {
                end += 1;
            }
            *offset = end;
            Ok(Some(line))
        }
        Some(EdgeVirtualIoHandle::FileWrite { .. }) => Err(VmError::HostError(format!(
            "edge io handle {handle} is write-only",
        ))),
        None => Err(VmError::HostError(format!(
            "edge io handle {handle} is invalid",
        ))),
    }
}

fn resolve_edge_io_write_target(
    context: &ProxyVmContext,
    value: &Value,
) -> Result<EdgeIoWriteTarget, VmError> {
    match value {
        Value::Int(handle) => {
            if let Some(target) = decode_protocol_io_handle(context, *handle) {
                return Ok(EdgeIoWriteTarget::Protocol(target));
            }
            let edge_io = context.lock_edge_io();
            match edge_io.handles.get(handle) {
                Some(EdgeVirtualIoHandle::FileWrite { path, append }) => {
                    Ok(EdgeIoWriteTarget::FilePath {
                        path: path.clone(),
                        append: *append,
                    })
                }
                Some(EdgeVirtualIoHandle::BufferedRead { .. }) => Err(VmError::HostError(format!(
                    "edge io handle {handle} is read-only",
                ))),
                None => Err(VmError::HostError(format!(
                    "edge io handle {handle} is invalid",
                ))),
            }
        }
        Value::String(_) => Err(requires_io_handle_error("io::write")),
        _ => Err(VmError::TypeMismatch("int")),
    }
}

async fn write_edge_file_path(path: &str, append: bool, text: &str) -> Result<(), VmError> {
    if append {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(|err| VmError::HostError(format!("edge io::write open failed: {err}")))?;
        file.write_all(text.as_bytes())
            .await
            .map_err(|err| VmError::HostError(format!("edge io::write append failed: {err}")))?;
        file.flush()
            .await
            .map_err(|err| VmError::HostError(format!("edge io::write flush failed: {err}")))?;
        return Ok(());
    }

    tokio::fs::write(path, text.as_bytes())
        .await
        .map_err(|err| VmError::HostError(format!("edge io::write failed: {err}")))
}

async fn read_io_target_all(
    context: &SharedProxyVmContext,
    target: EdgeProtocolIoHandle,
) -> Result<String, VmError> {
    match target {
        EdgeProtocolIoHandle::Downstream => {
            let body = consume_request_body_all(context).await?;
            Ok(String::from_utf8_lossy(&body).into_owned())
        }
        EdgeProtocolIoHandle::OutboundExchange(handle)
            if handle == default_upstream_exchange_handle() =>
        {
            Ok(String::from_utf8_lossy(&read_upstream_response_all(context).await?).into_owned())
        }
        EdgeProtocolIoHandle::OutboundExchange(handle) => Ok(String::from_utf8_lossy(
            &read_outbound_exchange_response_all(context, handle).await?,
        )
        .into_owned()),
    }
}

/// Reads one line from a protocol-backed handle.
///
/// `Ok(None)` is end of stream, `Ok(Some(""))` is a present blank line, and
/// the trailing `\n` is stripped by the underlying line reader.
async fn read_io_target_line(
    context: &SharedProxyVmContext,
    target: EdgeProtocolIoHandle,
) -> Result<Option<String>, VmError> {
    let line = match target {
        EdgeProtocolIoHandle::Downstream => read_request_body_next_line(context).await?,
        EdgeProtocolIoHandle::OutboundExchange(handle)
            if handle == default_upstream_exchange_handle() =>
        {
            read_upstream_response_next_line(context).await?
        }
        EdgeProtocolIoHandle::OutboundExchange(handle) => {
            read_outbound_exchange_response_next_line(context, handle).await?
        }
    };
    Ok(line.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

fn write_io_target(
    context: &SharedProxyVmContext,
    target: EdgeProtocolIoHandle,
    text: &str,
) -> Result<(), VmError> {
    match target {
        EdgeProtocolIoHandle::Downstream => {
            append_response_output_body_bytes(context, text.as_bytes())
        }
        EdgeProtocolIoHandle::OutboundExchange(handle) => {
            append_outbound_exchange_body(context, handle, text)
        }
    }
}

/// Opens a file handle for runtime I/O.
#[pd_edge_host_function(name = "io::open", scope = io)]
async fn io_open(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    path: String,
    mode: String,
) -> Result<CallOutcome, VmError> {
    let mode = mode.trim().to_ascii_lowercase();
    let handle = match mode.as_str() {
        "r" => {
            let buffered = tokio::fs::read_to_string(&path)
                .await
                .map_err(|err| VmError::HostError(format!("edge io::open read failed: {err}")))?;
            allocate_edge_virtual_io_handle(
                &context,
                EdgeVirtualIoHandle::BufferedRead {
                    text: buffered,
                    offset: 0,
                },
            )
        }
        "w" | "a" => allocate_edge_virtual_io_handle(
            &context,
            EdgeVirtualIoHandle::FileWrite {
                path: path.clone(),
                append: mode == "a",
            },
        ),
        _ => {
            return Err(VmError::HostError(format!(
                "edge io::open only supports modes 'r', 'w', or 'a', got '{mode}'",
            )));
        }
    };
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::Int(handle))))
}

/// Starts a child process and returns a process-backed handle.
#[pd_edge_host_function(name = "io::popen", scope = io)]
async fn io_popen(_vm: &mut Vm, _command: String, _mode: String) -> Result<CallOutcome, VmError> {
    Err(VmError::HostError(
        "io::popen is disabled in edge runtime; use protocol-specific async host APIs".to_string(),
    ))
}

/// Reads all remaining text from an I/O handle.
///
/// Publishes the core `io::read_all` contract: the text is the guest success
/// value, and a handle or transport failure stays a fatal host error.
#[pd_edge_host_function(name = "io::read_all", scope = io)]
async fn io_read_all(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    source: Value,
) -> Result<CallOutcome, VmError> {
    let source = resolve_edge_io_read_source(&context, &source, "io::read_all")?;
    let text = match source {
        EdgeIoReadSource::Protocol(target) => read_io_target_all(&context, target).await?,
        EdgeIoReadSource::VirtualHandle(handle) => read_edge_virtual_handle_all(&context, handle)?,
    };
    Ok(super::http::helpers::text_result_outcome(Ok(text)))
}

/// Reads a single line of text from an I/O handle.
///
/// Publishes the core `io::read_line` contract: `Ok(None)` is end of stream,
/// `Ok(Some(""))` is a present blank line, and `Ok(Some(text))` is a line with
/// the trailing `\n` stripped (a preceding `\r` is kept, exactly as the core
/// reader keeps it). A handle or transport failure is not a line value: it
/// stays a fatal host error, so the guest never sees an `Err` here.
/// `io::read_line` overrides the core builtin, so it must encode exactly the
/// contract the guest compiler resolves for that name.
#[pd_edge_host_function(name = "io::read_line", scope = io)]
async fn io_read_line(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    source: Value,
) -> Result<CallOutcome, VmError> {
    let source = resolve_edge_io_read_source(&context, &source, "io::read_line")?;
    let text = match source {
        EdgeIoReadSource::Protocol(target) => read_io_target_line(&context, target).await?,
        EdgeIoReadSource::VirtualHandle(handle) => read_edge_virtual_handle_line(&context, handle)?,
    };
    // The read itself succeeded, so the guest sees the optional line value;
    // a handle or transport failure above stays a fatal host error.
    Ok(super::http::helpers::optional_text_outcome(Ok(text)))
}

/// Writes text to an I/O handle.
#[pd_edge_host_function(name = "io::write", scope = io)]
async fn io_write(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    target_arg: Value,
    text: String,
) -> Result<CallOutcome, VmError> {
    let target = resolve_edge_io_write_target(&context, &target_arg)?;
    match target {
        EdgeIoWriteTarget::Protocol(kind) => write_io_target(&context, kind, &text)?,
        EdgeIoWriteTarget::FilePath { path, append } => {
            write_edge_file_path(&path, append, &text).await?;
        }
    }
    Ok(super::http::helpers::int_result_outcome(text.len() as i64))
}

/// Flushes buffered output for an I/O handle.
///
/// Publishes the core `io::flush` contract: a unit success value, and an
/// invalid handle stays a fatal host error.
#[pd_edge_host_function(name = "io::flush", scope = io)]
async fn io_flush(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    target: Value,
) -> Result<CallOutcome, VmError> {
    match target {
        Value::Int(handle) => {
            let edge_io = context.lock_edge_io();
            if decode_protocol_io_handle(&context, handle).is_none()
                && !edge_io.handles.contains_key(&handle)
            {
                return Err(invalid_io_handle_error(handle));
            }
        }
        Value::String(_) => return Err(requires_io_handle_error("io::flush")),
        _ => return Err(VmError::TypeMismatch("int")),
    }
    Ok(super::http::helpers::unit_result_outcome())
}

/// Closes an I/O handle.
///
/// Publishes the core `io::close` contract: a unit success value, and an
/// invalid handle stays a fatal host error.
#[pd_edge_host_function(name = "io::close", scope = io)]
async fn io_close(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    target: Value,
) -> Result<CallOutcome, VmError> {
    match target {
        Value::Int(handle) => {
            if decode_protocol_io_handle(&context, handle).is_some() {
                return Ok(super::http::helpers::unit_result_outcome());
            }
            if context.lock_edge_io().handles.remove(&handle).is_none() {
                return Err(invalid_io_handle_error(handle));
            }
        }
        Value::String(_) => return Err(requires_io_handle_error("io::close")),
        _ => return Err(VmError::TypeMismatch("int")),
    }
    Ok(super::http::helpers::unit_result_outcome())
}

/// Returns whether a file system path exists.
#[pd_edge_host_function(name = "io::exists", scope = io)]
async fn io_exists(
    _vm: &mut Vm,
    _context: SharedProxyVmContext,
    path: String,
) -> Result<CallOutcome, VmError> {
    let exists = tokio::fs::metadata(path.as_str()).await.is_ok();
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::Bool(
        exists,
    ))))
}
