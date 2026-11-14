use super::{
    helpers::{
        headers_to_value_map, optional_text_outcome, query_to_value_map, request_path_with_query,
    },
    state::{
        SharedProxyVmContext, read_request_body_all, read_request_body_next_chunk,
        request_body_eof, schedule_downstream_http_handoff,
    },
};
use axum::http::HeaderName;
use edge_abi::symbols::http::request as http_request;
use pd_edge_host_function::pd_edge_host_function;
use vm::{CallOutcome, CallReturn, Value, Vm, VmError};

use crate::{
    abi_impl::schedule_current_future_call, runtime::promote_transport_context_into_http_request,
};

#[derive(Clone, Copy)]
enum RequestField {
    Id,
    Method,
    Path,
    Query,
    PathWithQuery,
    HttpVersion,
    Scheme,
    Host,
    ClientIp,
}

fn request_field_outcome(
    context: SharedProxyVmContext,
    field: RequestField,
) -> Result<CallOutcome, VmError> {
    let value = context.with_request_head(|request_head| match field {
        RequestField::Id => request_head.request_id().to_string(),
        RequestField::Method => request_head.method().as_str().to_string(),
        RequestField::Path => request_head.path().to_string(),
        RequestField::Query => request_head.query().to_string(),
        RequestField::PathWithQuery => {
            request_path_with_query(request_head.path(), request_head.query())
        }
        RequestField::HttpVersion => request_head.http_version().to_string(),
        RequestField::Scheme => request_head.scheme().to_string(),
        RequestField::Host => request_head.host().to_string(),
        RequestField::ClientIp => request_head.client_ip().to_string(),
    });
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::string(
        value,
    ))))
}

/// Returns the full body for the downstream HTTP request as text.
#[pd_edge_host_function(name = http_request::GET_BODY.name, scope = http)]
async fn get_request_body(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
) -> Result<CallOutcome, VmError> {
    let body = read_request_body_all(&context).await?;
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::string(
        String::from_utf8_lossy(&body).into_owned(),
    ))))
}

/// Reads the next body chunk from the downstream HTTP request.
#[pd_edge_host_function(name = "http::request::body::next_chunk", scope = http_extension)]
async fn get_request_body_chunk(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
    max_bytes: i64,
) -> Result<CallOutcome, VmError> {
    if max_bytes <= 0 {
        return Err(VmError::HostError(format!(
            "body chunk size must be > 0, got '{max_bytes}'",
        )));
    }
    let chunk = read_request_body_next_chunk(&context, max_bytes as usize).await?;
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::string(
        String::from_utf8_lossy(&chunk).into_owned(),
    ))))
}

/// Returns whether the body stream for the downstream HTTP request is exhausted.
#[pd_edge_host_function(name = "http::request::body::eof", scope = http_extension)]
async fn get_request_body_eof(
    _vm: &mut Vm,
    context: SharedProxyVmContext,
) -> Result<CallOutcome, VmError> {
    let eof = request_body_eof(&context).await?;
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::Bool(eof))))
}

/// Attaches the untouched downstream transport to the HTTP stack and resumes
/// the current VM invocation with HTTP request semantics.
#[pd_edge_host_function(name = "http::downstream::attach_transport", scope = http)]
fn attach_downstream_transport_to_http(
    vm: &mut Vm,
    context: SharedProxyVmContext,
) -> Result<CallOutcome, VmError> {
    schedule_downstream_http_handoff(&context)?;
    schedule_current_future_call(vm, async move {
        promote_transport_context_into_http_request(context).await?;
        Ok(CallReturn::none())
    })
}

/// Returns the current downstream request id.
#[pd_edge_host_function(name = http_request::GET_ID.name, scope = http)]
fn get_request_id(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Id)
}

/// Returns the HTTP method for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_METHOD.name, scope = http)]
fn get_request_method(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Method)
}

/// Returns the request path for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_PATH.name, scope = http)]
fn get_request_path(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Path)
}

/// Returns the decoded query string for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_QUERY.name, scope = http)]
fn get_request_query(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Query)
}

/// Returns the URL scheme for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_SCHEME.name, scope = http)]
fn get_request_scheme(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Scheme)
}

/// Returns the host name for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_HOST.name, scope = http)]
fn get_request_host(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::Host)
}

/// Returns the downstream client IP address.
#[pd_edge_host_function(name = http_request::GET_CLIENT_IP.name, scope = http)]
fn get_request_client_ip(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::ClientIp)
}

/// Returns the request path and query string for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_PATH_WITH_QUERY.name, scope = http)]
fn get_request_path_with_query(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::PathWithQuery)
}

/// Returns the HTTP version for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_HTTP_VERSION.name, scope = http)]
fn get_request_http_version(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    request_field_outcome(context, RequestField::HttpVersion)
}

/// Returns the first value for a header on the downstream HTTP request.
///
/// `Ok(None)` means the request carries no such header, `Ok(Some(""))` means it
/// carries it with an empty value, and `Err` reports a value that is not valid
/// text. An invalid header name is a fatal host error.
#[pd_edge_host_function(name = http_request::GET_HEADER.name, scope = http)]
fn get_request_header_impl(
    context: SharedProxyVmContext,
    name: &str,
) -> Result<CallOutcome, VmError> {
    let header_name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| VmError::HostError(format!("invalid header name '{name}'")))?;
    let value = context.with_request_head(|request_head| {
        request_head.lazy_headers().get_text(header_name.as_str())
    });
    Ok(optional_text_outcome(value))
}

/// Returns all headers on the downstream HTTP request as a map.
#[pd_edge_host_function(name = http_request::GET_HEADERS.name, scope = http)]
fn get_request_headers(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    Ok(CallOutcome::Return(vm::CallReturn::one(
        context.with_request_head(|request_head| headers_to_value_map(request_head.headers())),
    )))
}

/// Returns a query parameter from the downstream HTTP request.
///
/// `Ok(None)` means the query string has no such parameter, `Ok(Some(""))`
/// means it has one with an empty value; a repeated key keeps the first value.
#[pd_edge_host_function(name = http_request::GET_QUERY_ARG.name, scope = http)]
fn get_request_query_arg(
    context: SharedProxyVmContext,
    name: &str,
) -> Result<CallOutcome, VmError> {
    let value = context.with_request_head(|request_head| {
        url::form_urlencoded::parse(request_head.query().as_bytes())
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    });
    Ok(optional_text_outcome(Ok(value)))
}

/// Returns all query parameters from the downstream HTTP request as a map.
#[pd_edge_host_function(name = http_request::GET_QUERY_ARGS.name, scope = http)]
fn get_request_query_args(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    Ok(CallOutcome::Return(vm::CallReturn::one(
        context.with_request_head(|request_head| query_to_value_map(request_head.query())),
    )))
}

/// Returns the local destination port for the downstream HTTP request.
#[pd_edge_host_function(name = http_request::GET_PORT.name, scope = http)]
fn get_request_port(context: SharedProxyVmContext) -> Result<CallOutcome, VmError> {
    Ok(CallOutcome::Return(vm::CallReturn::one(Value::Int(
        context.with_request_head(|request_head| request_head.port() as i64),
    ))))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{HeaderMap, HeaderValue, Method};

    use super::*;
    use crate::abi_impl::RateLimiterStore;
    use crate::abi_impl::http::ProxyVmContext;
    use crate::abi_impl::http::helpers::decode_optional_text_result;
    use crate::abi_impl::http::state::{
        HttpRequestContext, LazyRequestId, RequestPortField, RequestStringField,
    };

    fn test_context(query: &str, headers: HeaderMap) -> SharedProxyVmContext {
        Arc::new(ProxyVmContext::from_http_request(
            HttpRequestContext {
                request_id: LazyRequestId::from_string("test-request".to_string()),
                method: Method::GET,
                path: RequestStringField::Static("/getters".to_string()),
                query: RequestStringField::Static(query.to_string()),
                http_version: RequestStringField::Static("1.1".to_string()),
                port: RequestPortField::Static(80),
                scheme: RequestStringField::Static("http".to_string()),
                host: RequestStringField::Static("localhost".to_string()),
                client_ip: RequestStringField::Static("127.0.0.1".to_string()),
                body: Body::empty(),
                headers: headers.into(),
            },
            Arc::new(RateLimiterStore::new()),
        ))
    }

    fn header_value(bytes: &[u8]) -> HeaderValue {
        HeaderValue::from_bytes(bytes).expect("header value bytes should be admissible")
    }

    #[test]
    fn request_get_header_absent_is_ok_none() {
        let context = test_context("", HeaderMap::new());
        let outcome =
            get_request_header_impl(context, "x-absent").expect("absent header is not fatal");
        assert_eq!(
            decode_optional_text_result(&outcome),
            Ok(None),
            "an absent request header must be Ok(None), not an empty string"
        );
    }

    #[test]
    fn request_get_header_present_empty_is_ok_some_empty() {
        let mut headers = HeaderMap::new();
        headers.insert("x-empty", header_value(b""));
        let context = test_context("", headers);
        let outcome =
            get_request_header_impl(context, "x-empty").expect("empty header is not fatal");
        assert_eq!(
            decode_optional_text_result(&outcome),
            Ok(Some(String::new())),
            "a present empty request header must stay distinguishable from an absent one"
        );
    }

    #[test]
    fn request_get_header_invalid_text_is_err() {
        let mut headers = HeaderMap::new();
        headers.insert("x-binary", header_value(&[0x80, 0xff]));
        let context = test_context("", headers);
        let outcome =
            get_request_header_impl(context, "x-binary").expect("decode error is a guest error");
        match decode_optional_text_result(&outcome) {
            Err(message) => assert!(
                message.contains("not valid text"),
                "the guest error must describe the decode failure: {message}"
            ),
            other => panic!("a header value that is not text must be a guest error, got {other:?}"),
        }
    }

    #[test]
    fn request_get_header_invalid_name_is_fatal() {
        let context = test_context("", HeaderMap::new());
        let error = get_request_header_impl(context, "not a header name")
            .expect_err("an invalid header name must stay a fatal host error");
        assert!(
            format!("{error}").contains("invalid header name"),
            "the fatal error must name the invalid header: {error}"
        );
    }

    #[test]
    fn request_get_query_arg_repeated_key_keeps_the_first_value() {
        let context = test_context("a=1&a=2&flag=", HeaderMap::new());

        let repeated =
            get_request_query_arg_impl(context.clone(), "a").expect("query arg is not fatal");
        assert_eq!(
            decode_optional_text_result(&repeated),
            Ok(Some("1".to_string())),
            "a repeated query key must keep the first value"
        );

        let empty =
            get_request_query_arg_impl(context.clone(), "flag").expect("query arg is not fatal");
        assert_eq!(
            decode_optional_text_result(&empty),
            Ok(Some(String::new())),
            "a parameter with an empty value must be Ok(Some(\"\"))"
        );

        let absent =
            get_request_query_arg_impl(context, "missing").expect("query arg is not fatal");
        assert_eq!(
            decode_optional_text_result(&absent),
            Ok(None),
            "an absent query parameter must be Ok(None)"
        );
    }
}
