/// Host function `http::request::get_id`.
#[pd_host_function(name = "http::request::get_id")]
fn http_request_get_id() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_method`.
#[pd_host_function(name = "http::request::get_method")]
fn http_request_get_method() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_path`.
#[pd_host_function(name = "http::request::get_path")]
fn http_request_get_path() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_query`.
#[pd_host_function(name = "http::request::get_query")]
fn http_request_get_query() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_scheme`.
#[pd_host_function(name = "http::request::get_scheme")]
fn http_request_get_scheme() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_host`.
#[pd_host_function(name = "http::request::get_host")]
fn http_request_get_host() -> String {
    unreachable!("abi declaration only")
}

/// Returns the first value for a header on the downstream HTTP request.
///
/// `Ok(None)` means the request carries no such header, `Ok(Some(""))` means it
/// carries it with an empty value, and `Err` reports a value that is not valid
/// text. An invalid header name is a fatal host error, not an absent header.
#[pd_host_function(name = "http::request::get_header")]
fn http_request_get_header(name: &str) -> GuestResult<GuestOption<String>, String> {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_client_ip`.
#[pd_host_function(name = "http::request::get_client_ip")]
fn http_request_get_client_ip() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_headers`.
#[pd_host_function(name = "http::request::get_headers")]
fn http_request_get_headers() -> Map {
    unreachable!("abi declaration only")
}

/// Returns a query parameter from the downstream HTTP request.
///
/// `Ok(None)` means the query string has no such parameter, `Ok(Some(""))`
/// means it has one with an empty value. A repeated key keeps the first value.
#[pd_host_function(name = "http::request::get_query_arg")]
fn http_request_get_query_arg(name: &str) -> GuestResult<GuestOption<String>, String> {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_query_args`.
#[pd_host_function(name = "http::request::get_query_args")]
fn http_request_get_query_args() -> Map {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_path_with_query`.
#[pd_host_function(name = "http::request::get_path_with_query")]
fn http_request_get_path_with_query() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_body`.
#[pd_host_function(name = "http::request::get_body")]
fn http_request_get_body() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_http_version`.
#[pd_host_function(name = "http::request::get_http_version")]
fn http_request_get_http_version() -> String {
    unreachable!("abi declaration only")
}

/// Host function `http::request::get_port`.
#[pd_host_function(name = "http::request::get_port")]
fn http_request_get_port() -> i64 {
    unreachable!("abi declaration only")
}
