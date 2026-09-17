use edge::{
    ABI_VERSION, FUNCTIONS, compile_edge_source_file, compile_edge_source_file_with_options,
    edge_compile_options, function_by_name, host_namespace_specs,
};
#[cfg(feature = "mqtt")]
use vm::HostTypeSchema;
use vm::{
    CompileSourceFileOptions, CompiledProgram, SourceFlavor, Value, Vm, VmStatus,
    lint_trailing_function_return_semicolons,
    lint_unknown_inferred_local_types_at_path_with_options, lint_unknown_type_annotations,
};

fn unique_temp_root(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "pd_edge_compile_test_{label}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&root).expect("temp module root should be created");
    root
}

#[test]
fn compile_edge_source_file_supports_runtime_namespace_host_import() {
    let root = unique_temp_root("runtime_namespace");
    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use runtime;
        runtime::sleep(1);
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "runtime::sleep"),
        "runtime namespace should map to runtime host import"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_supports_runtime_exit_host_import() {
    let root = unique_temp_root("runtime_exit_namespace");
    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use runtime;
        runtime::exit();
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "runtime::exit"),
        "runtime namespace should map runtime::exit to a host import"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_supports_rate_limit_namespace_host_import() {
    let root = unique_temp_root("rate_limit_namespace");
    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use rate_limit;
        rate_limit::allow("client-1", 3, 60);
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "rate_limit::allow"),
        "rate_limit namespace should map to host import"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_supports_console_namespace_host_import() {
    let root = unique_temp_root("console_namespace");
    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use console;
        let arg0: string = console::args::get(0);
        console::args::count() + arg0.length + console::stdout::write("ok");
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "console::args::count"),
        "console namespace should map args::count to a host import"
    );
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "console::args::get"),
        "console namespace should map args::get to a host import"
    );
    assert!(
        compiled
            .program
            .imports
            .iter()
            .any(|import| import.name == "console::stdout::write"),
        "console namespace should map stdout::write to a host import"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_supports_console_http3_client_example() {
    let program_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/console/sample_console_http3_client.rss");

    let compiled =
        compile_edge_source_file(program_path.as_path()).expect("example should compile");
    let import_names = compiled
        .program
        .imports
        .iter()
        .map(|import| import.name.as_str())
        .collect::<Vec<_>>();

    assert!(
        import_names.contains(&"console::args::count"),
        "console sample should import argv count"
    );
    assert!(
        import_names.contains(&"console::stdin::read_all"),
        "console sample should import stdin read_all for body streaming"
    );
    assert!(
        import_names.contains(&"runtime::exit"),
        "console sample should import runtime::exit for usage handling"
    );
    assert!(
        import_names.contains(&"http::exchange::send"),
        "console sample should send an outbound exchange"
    );
    assert!(
        import_names.contains(&"tls::session::from_socket"),
        "console sample should create a TLS session from the exchange"
    );
}

#[test]
fn sample_anthropic_messages_to_openai_chat_completions_program_compiles_and_lints_cleanly() {
    let program_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "examples/http/proxy/sample_anthropic_messages_to_openai_chat_completions_program.rss",
    );
    let source = std::fs::read_to_string(&program_path).expect("sample source should read");

    compile_edge_source_file(program_path.as_path()).expect("sample should compile");

    let unknown_inferred_locals = lint_unknown_inferred_local_types_at_path_with_options(
        &program_path,
        &source,
        SourceFlavor::RustScript,
        edge_compile_options(),
    )
    .expect("unknown inferred local lint should succeed");
    assert!(
        unknown_inferred_locals.is_empty(),
        "sample should not produce unknown inferred local warnings: {unknown_inferred_locals:?}"
    );

    let unknown_type_annotations = lint_unknown_type_annotations(&source, SourceFlavor::RustScript)
        .expect("unknown type annotation lint should succeed");
    assert!(
        unknown_type_annotations.is_empty(),
        "sample should not produce unknown type annotation warnings: {unknown_type_annotations:?}"
    );

    let trailing_return_semicolons =
        lint_trailing_function_return_semicolons(&source, SourceFlavor::RustScript)
            .expect("trailing function return semicolon lint should succeed");
    assert!(
        trailing_return_semicolons.is_empty(),
        "sample should not produce trailing function return semicolon warnings: {trailing_return_semicolons:?}"
    );
}

#[test]
fn compile_edge_source_file_prefers_local_module_over_host_namespace_fallback() {
    let root = unique_temp_root("helper_local_module");

    let helper_module = root.join("helper.rss");
    std::fs::write(
        &helper_module,
        r#"
        pub fn sleep(ms) {
            ms + 1;
        }
    "#,
    )
    .expect("helper module should write");

    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use helper;
        helper::sleep(41);
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    assert!(
        compiled.program.imports.is_empty(),
        "a local file module that is not an edge ABI host namespace must still compile as a guest module"
    );

    let mut vm = Vm::new(compiled.program);
    let status = vm.run().expect("vm should run");
    assert_eq!(status, VmStatus::Halted);
    assert_eq!(vm.stack(), &[Value::Int(42)]);

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_file(helper_module);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_with_options_can_override_runtime_module() {
    let root = unique_temp_root("helper_override");

    let override_module = root.join("custom_helper.rss");
    std::fs::write(
        &override_module,
        r#"
        pub fn sleep(ms) {
            ms + 2;
        }
    "#,
    )
    .expect("override module source should write");

    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use helper;
        helper::sleep(40);
    "#,
    )
    .expect("main source should write");

    let options =
        CompileSourceFileOptions::new().with_module_override_path("helper.rss", &override_module);
    let compiled =
        compile_edge_source_file_with_options(&main_path, options).expect("compile should succeed");
    assert!(
        compiled.program.imports.is_empty(),
        "module override should replace a same-named guest file module"
    );

    let mut vm = Vm::new(compiled.program);
    let status = vm.run().expect("vm should run");
    assert_eq!(status, VmStatus::Halted);
    assert_eq!(vm.stack(), &[Value::Int(42)]);

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_file(override_module);
    let _ = std::fs::remove_dir(root);
}

#[test]
fn compile_edge_source_file_supports_embedded_edge_upstream_wrapper_modules() {
    let root = unique_temp_root("edge_upstream_wrappers");
    let main_path = root.join("main.rss");
    std::fs::write(
        &main_path,
        r#"
        use edge::http::upstream as upstream;
        use edge::http::upstream::request as upstream_request;
        use edge::http::upstream::response as upstream_response;

        upstream_request::set_target("example.test", 80);
        upstream_request::set_header("accept", "text/plain");
        let proxy_handle = upstream::as_stream();
        let status = upstream_response::get_status();
        let line = upstream_response::read_line();
        let chunk = upstream_response::next_chunk(8);
        let done = upstream_response::eof();

        [proxy_handle, status, line, chunk, done];
    "#,
    )
    .expect("main source should write");

    let compiled = compile_edge_source_file(main_path.as_path()).expect("compile should succeed");
    let import_names = compiled
        .program
        .imports
        .iter()
        .map(|import| import.name.as_str())
        .collect::<Vec<_>>();

    assert!(
        import_names.contains(&"http::exchange::default_upstream"),
        "wrapper should import the canonical default exchange handle"
    );
    assert!(
        import_names.contains(&"http::exchange::set_target"),
        "wrapper should import canonical exchange setters"
    );
    assert!(
        import_names.contains(&"http::exchange::get_status"),
        "wrapper should import canonical exchange response getters"
    );
    assert!(
        import_names.contains(&"proxy::stream::exchange"),
        "wrapper should build proxy streams from explicit exchanges"
    );
    assert!(
        import_names.contains(&"http::exchange::body::next_chunk"),
        "wrapper should import canonical exchange body streaming calls"
    );
    assert!(
        !import_names
            .iter()
            .any(|name| name.starts_with("http::upstream::")
                || *name == "proxy::stream::default_upstream"),
        "wrapper modules should not reintroduce removed alias host calls"
    );

    let _ = std::fs::remove_file(main_path);
    let _ = std::fs::remove_dir(root);
}

fn compile_inline(label: &str, source: &str) -> CompiledProgram {
    let root = unique_temp_root(label);
    let main_path = root.join("main.rss");
    std::fs::write(&main_path, source).expect("main source should write");
    let compiled = compile_edge_source_file(main_path.as_path())
        .unwrap_or_else(|error| panic!("{label} should compile: {error:?}"));
    let _ = std::fs::remove_file(&main_path);
    let _ = std::fs::remove_dir(&root);
    compiled
}

fn compile_example(relative: &str) -> CompiledProgram {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    compile_edge_source_file(path.as_path())
        .unwrap_or_else(|error| panic!("{relative} should compile: {error:?}"))
}

#[cfg(any(not(feature = "mqtt"), not(feature = "webrtc")))]
fn catalog_excludes_protocol(root: &str) {
    let prefix = format!("{root}::");
    assert!(
        host_namespace_specs().iter().all(|spec| spec.root != root),
        "{root} runtime module must be omitted from the published catalog"
    );
    assert!(
        FUNCTIONS
            .iter()
            .all(|function| !function.name.starts_with(&prefix)),
        "{root} functions must be omitted from the published ABI table"
    );
}

#[cfg(any(not(feature = "mqtt"), not(feature = "webrtc")))]
fn assert_unresolved_protocol_imports_are_not_catalog_backed(
    compiled: &CompiledProgram,
    root: &str,
) {
    let prefix = format!("{root}::");
    let schemas = compiled.program.host_import_schemas();
    let mut seen = false;
    for (index, import) in compiled.program.imports.iter().enumerate() {
        if !import.name.starts_with(&prefix) {
            continue;
        }
        seen = true;
        assert!(
            function_by_name(&import.name).is_none(),
            "{} must be omitted from exact ABI catalog lookup",
            import.name
        );
        let schema = schemas.get(index).and_then(Option::as_ref);
        assert!(
            schema.is_none(),
            "{} must not carry a catalog schema when {root} is off",
            import.name
        );
    }
    assert!(
        seen,
        "the source must mention unresolved {root} host imports"
    );
}

fn assert_every_import_is_abi25_catalog_backed(compiled: &CompiledProgram, label: &str) {
    assert_eq!(ABI_VERSION, 25, "{label}: published ABI must stay at 25");
    let imports = &compiled.program.imports;
    let schemas = compiled.program.host_import_schemas();
    assert!(
        !imports.is_empty(),
        "{label}: representative source must import host functions"
    );
    assert_eq!(
        schemas.len(),
        imports.len(),
        "{label}: host_import_schemas must be aligned with imports"
    );
    let mut fingerprint = None;
    for (import, schema) in imports.iter().zip(schemas) {
        let schema = schema.as_ref().unwrap_or_else(|| {
            panic!(
                "{label}: {} must have Some(schema) from the ABI25 catalog",
                import.name
            )
        });
        assert_eq!(schema.name, import.name, "{label}");
        assert!(
            function_by_name(&import.name).is_some(),
            "{label}: {} must resolve through exact ABI25 catalog lookup",
            import.name
        );
        match fingerprint {
            None => fingerprint = Some(schema.fingerprint),
            Some(expected) => assert_eq!(
                schema.fingerprint, expected,
                "{label}: {} fingerprint drifted from the ABI25 catalog",
                import.name
            ),
        }
    }
}

#[cfg(feature = "mqtt")]
fn schema_for_import<'a>(compiled: &'a CompiledProgram, name: &str) -> &'a vm::HostImportSchema {
    let index = compiled
        .program
        .imports
        .iter()
        .position(|import| import.name == name)
        .unwrap_or_else(|| panic!("compiled program must import {name}"));
    compiled
        .program
        .host_import_schemas()
        .get(index)
        .and_then(Option::as_ref)
        .unwrap_or_else(|| panic!("{name} must carry Some(schema) from the ABI25 catalog"))
}

#[test]
fn published_edge_abi_is_version_25() {
    assert_eq!(ABI_VERSION, 25);
}

#[cfg(all(feature = "http", not(feature = "mqtt"), not(feature = "webrtc")))]
#[test]
fn default_http_catalog_excludes_mqtt_and_webrtc() {
    catalog_excludes_protocol("mqtt");
    catalog_excludes_protocol("webrtc");
    assert!(
        function_by_name("mqtt::connection::read_event").is_none(),
        "mqtt::connection::read_event must be omitted from the default/http catalog"
    );
    assert!(
        function_by_name("webrtc::connection::new").is_none(),
        "webrtc::connection::new must be omitted from the default/http catalog"
    );
}

#[cfg(not(feature = "mqtt"))]
#[test]
fn mqtt_imports_are_not_catalog_backed_without_mqtt_feature() {
    catalog_excludes_protocol("mqtt");
    assert!(
        function_by_name("mqtt::connection::read_event").is_none(),
        "mqtt ABI names must be omitted from the catalog when mqtt is off"
    );

    let compiled = compile_inline(
        "mqtt_disabled",
        r#"
        use mqtt;
        mqtt::connection::read_event(0);
    "#,
    );
    assert_unresolved_protocol_imports_are_not_catalog_backed(&compiled, "mqtt");
}

#[cfg(feature = "mqtt")]
#[test]
fn mqtt_feature_publishes_named_mqtt_event_and_runtime_module() {
    assert!(
        host_namespace_specs()
            .iter()
            .any(|spec| spec.root == "mqtt"),
        "mqtt runtime module must be published when mqtt is on"
    );
    assert!(
        FUNCTIONS
            .iter()
            .any(|function| function.name == "mqtt::connection::read_event"),
        "canonical mqtt::connection::read_event must be in the published ABI table"
    );
    let function = function_by_name("mqtt::connection::read_event")
        .expect("mqtt ABI names must resolve through exact catalog lookup when mqtt is on");
    assert_eq!(function.name, "mqtt::connection::read_event");

    let compiled = compile_example("examples/mqtt/upstream/sample_mqtt_publish_program.rss");
    assert_every_import_is_abi25_catalog_backed(&compiled, "mqtt sample");

    let schema = schema_for_import(&compiled, "mqtt::connection::read_event");
    match &schema.return_type {
        HostTypeSchema::Named { name, fields } => {
            assert_eq!(name, "MqttEvent");
            assert_eq!(fields.len(), 8);
            assert_eq!(fields[0].name, "kind");
            assert_eq!(fields[0].ty, HostTypeSchema::String);
        }
        other => panic!("expected Named MqttEvent, got {other:?}"),
    }
}

#[cfg(not(feature = "webrtc"))]
#[test]
fn webrtc_imports_are_not_catalog_backed_without_webrtc_feature() {
    catalog_excludes_protocol("webrtc");
    assert!(
        function_by_name("webrtc::connection::new").is_none(),
        "webrtc ABI names must be omitted from the catalog when webrtc is off"
    );

    let compiled = compile_inline(
        "webrtc_disabled",
        r#"
        use webrtc;
        webrtc::connection::new();
    "#,
    );
    assert_unresolved_protocol_imports_are_not_catalog_backed(&compiled, "webrtc");
}

#[cfg(all(feature = "http", feature = "tls", feature = "websocket"))]
#[test]
fn representative_sources_carry_abi25_catalog_schemas() {
    let cases = [
        "examples/http/proxy/sample_proxy_program.rss",
        "examples/transport/tls/sample_transport_tls_handshake_program.rss",
        "examples/websocket/proxy/sample_websocket_proxy_program.rss",
        "examples/proxy/forward/sample_forward_proxy_program.rss",
        "examples/transport/upstream/sample_upstream_transport_proxy_program.rss",
    ];
    for relative in cases {
        let compiled = compile_example(relative);
        assert_every_import_is_abi25_catalog_backed(&compiled, relative);
    }

    let udp = compile_inline(
        "udp_catalog",
        r#"
        use udp;
        udp::socket::new();
    "#,
    );
    assert_every_import_is_abi25_catalog_backed(&udp, "udp inline");
    assert!(
        udp.program
            .imports
            .iter()
            .any(|import| import.name == "udp::socket::new"),
        "udp representative source must import udp::socket::new"
    );
}
