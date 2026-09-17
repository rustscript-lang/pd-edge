//! Descriptor-derived edge host contracts.
//!
//! ## Single source of truth
//!
//! The guest contract of every edge host function is declared exactly once:
//! in `pd-edge-abi/src/abi_spec/**`, where each function carries the core
//! `#[pd_host_function(name = "...")]` attribute and is published through
//! `edge_abi::FUNCTIONS` (name, parameter names/types, return type, docs).
//! The `#[pd_edge_host_function]` attribute on the implementation contributes
//! only the *runtime binding*: the adapter function and its binding class.
//!
//! This module joins the two halves into a core [`HostFunctionDescriptor`] and
//! validates the join, so the edge runtime never keeps a second schema,
//! effect, or adapter source that could drift from the published ABI:
//!
//! ```text
//! abi_spec (core #[pd_host_function])  ---\
//!                                          >-- HostFunctionDescriptor
//! #[pd_edge_host_function] adapter     ---/    (schema + binding + adapter)
//! ```
//!
//! Closed records are overlayed as [`HostTypeSchema::Named`] on the compiler
//! catalog. Runtime values remain maps. That overlay changes the catalog
//! fingerprint tag, so it is an explicit ABI-versioned change (version 25).
//!
//! ## Intentional dynamic boundaries (allowlisted)
//!
//! Open collections stay dynamic, each family documented individually:
//!
//! - HTTP request header maps (`http::request::get_headers`)
//! - HTTP request query-argument maps (`http::request::get_query_args`)
//! - HTTP response header maps (`http::response::get_headers`)
//! - HTTP response trailer maps (`http::response::get_trailers`)
//! - HTTP exchange header maps (`http::exchange::get_headers`)
//! - HTTP exchange trailer maps (`http::exchange::get_trailers`)
//! - HTTP header-batch `any` parameters (`http::response::set_headers`,
//!   `http::response::apply_exchange_with_headers`,
//!   `http::exchange::prepare_default_upstream`)
//!
//! WebRTC has no map/any wire types. MQTT `read_event` is a closed
//! three-variant record and is typed as named `MqttEvent`.
//!
//! ## Raw handle-token boundaries
//!
//! Edge host handles travel as `int` wire values owned by runtime scope
//! state. Guest resource effects cannot represent those tokens without
//! changing the published ABI, so the enforceable metadata lives in
//! [`super::raw_handles`].

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use edge_abi::FUNCTIONS as EDGE_ABI_FUNCTIONS;
use edge_abi::{AbiFunction, AbiParamType, AbiValueType};
use vm::host_extension::guest_resource_effects;
use vm::{
    HostAdapterDescriptor, HostApiCatalog, HostBindingDescriptor, HostBindingKind,
    HostFunctionDescriptor, HostFunctionRegistry, HostFunctionSchema, HostImportSchema,
    HostParamSchema, HostStructField, HostTypeSchema, VmError, VmResult, catalog_import_schemas,
    catalog_named_struct_schemas,
};

use super::raw_handles;

/// Maps an ABI parameter wire type onto the descriptor schema language.
///
/// The mapping is a projection of the published contract, never a
/// strengthening of it: `Any` stays [`HostTypeSchema::Unknown`] and open
/// collections stay `Map(Unknown)`/`Array(Unknown)`. Closed records are
/// overlayed afterwards by [`overlay_named_return`].
pub(crate) fn edge_param_type_schema(param: AbiParamType) -> HostTypeSchema {
    match param {
        AbiParamType::Any => HostTypeSchema::Unknown,
        AbiParamType::Null => HostTypeSchema::Null,
        AbiParamType::Int => HostTypeSchema::Int,
        AbiParamType::Float => HostTypeSchema::Float,
        AbiParamType::Bool => HostTypeSchema::Bool,
        AbiParamType::String => HostTypeSchema::String,
        AbiParamType::Bytes => HostTypeSchema::Bytes,
        AbiParamType::Array => HostTypeSchema::Array(Box::new(HostTypeSchema::Unknown)),
        AbiParamType::Map => HostTypeSchema::Map(Box::new(HostTypeSchema::Unknown)),
        AbiParamType::Number => HostTypeSchema::Number,
    }
}

/// Maps an ABI return wire type onto the descriptor schema language.
pub(crate) fn edge_return_type_schema(value: AbiValueType) -> HostTypeSchema {
    match value {
        AbiValueType::Unknown => HostTypeSchema::Unknown,
        AbiValueType::Null => HostTypeSchema::Null,
        AbiValueType::Int => HostTypeSchema::Int,
        AbiValueType::Float => HostTypeSchema::Float,
        AbiValueType::Bool => HostTypeSchema::Bool,
        AbiValueType::String => HostTypeSchema::String,
        AbiValueType::Bytes => HostTypeSchema::Bytes,
        AbiValueType::Array => HostTypeSchema::Array(Box::new(HostTypeSchema::Unknown)),
        AbiValueType::Map => HostTypeSchema::Map(Box::new(HostTypeSchema::Unknown)),
    }
}

/// Closed MQTT connection event: `publish` | `closed` | `failed`.
///
/// Runtime values remain maps; the compiler catalog types the shape as a
/// named struct so field access and `.has` stay exact.
pub(crate) fn mqtt_event_schema() -> HostTypeSchema {
    HostTypeSchema::named_struct(
        "MqttEvent",
        vec![
            HostStructField::new("kind", HostTypeSchema::String),
            HostStructField::new(
                "topic",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::String)),
            ),
            HostStructField::new(
                "payload_text",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::String)),
            ),
            HostStructField::new(
                "payload_base64",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::String)),
            ),
            HostStructField::new(
                "qos",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::Int)),
            ),
            HostStructField::new(
                "retain",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::Bool)),
            ),
            HostStructField::new(
                "dup",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::Bool)),
            ),
            HostStructField::new(
                "reason",
                HostTypeSchema::Optional(Box::new(HostTypeSchema::String)),
            ),
        ],
    )
}

/// Overlay closed records as named schemas without changing the coarse ABI
/// wire type. Runtime values remain maps.
fn overlay_named_return(name: &str, fallback: HostTypeSchema) -> HostTypeSchema {
    match name {
        "mqtt::connection::read_event" => mqtt_event_schema(),
        _ => fallback,
    }
}

/// Open header/query/trailer maps that remain `Map(Unknown)` on purpose.
#[cfg(test)]
pub(crate) const DYNAMIC_MAP_RETURNS: &[&str] = &[
    "http::request::get_headers",
    "http::request::get_query_args",
    "http::response::get_trailers",
    "http::response::get_headers",
    "http::exchange::get_headers",
    "http::exchange::get_trailers",
];

/// Polymorphic header-batch `any` parameters that remain Unknown.
#[cfg(test)]
pub(crate) const DYNAMIC_ANY_PARAMS: &[(&str, usize)] = &[
    ("http::response::set_headers", 0),
    ("http::response::apply_exchange_with_headers", 1),
    ("http::exchange::prepare_default_upstream", 3),
];

/// The declared ABI entry for `name`, or `None` when the name is not part of
/// the published edge ABI.
pub(crate) fn edge_abi_function(name: &str) -> Option<&'static AbiFunction> {
    edge_abi::function_by_name(name)
}

/// Guest schema of one edge host function, derived from its ABI declaration
/// plus the named-struct overlay for closed records.
///
/// A name that is not declared in the ABI spec is a hard error: an edge host
/// function may not invent a guest contract the VM compiler cannot see.
pub(crate) fn edge_function_schema(name: &str) -> Result<HostFunctionSchema, String> {
    let function = edge_abi_function(name).ok_or_else(|| {
        format!("edge host function '{name}' is not declared in the edge ABI spec")
    })?;
    if function.param_names.len() != function.param_types.len() {
        return Err(format!(
            "edge ABI function '{name}' declares {} parameter names for {} parameter types",
            function.param_names.len(),
            function.param_types.len()
        ));
    }
    Ok(HostFunctionSchema {
        name: function.name.to_string(),
        params: function
            .param_names
            .iter()
            .zip(function.param_types.iter())
            .map(|(param_name, param_type)| {
                HostParamSchema::value(*param_name, edge_param_type_schema(*param_type))
            })
            .collect(),
        return_type: overlay_named_return(
            function.name,
            edge_return_type_schema(function.return_type),
        ),
        description: function.docs.to_string(),
    })
}

/// Unbound placeholder for an ABI function this build does not implement.
pub(crate) fn unbound_edge_descriptor(name: &str) -> HostFunctionDescriptor {
    let schema = edge_function_schema(name)
        .unwrap_or_else(|error| panic!("invalid unbound edge descriptor: {error}"));
    HostFunctionDescriptor {
        effects: guest_resource_effects(&schema),
        schema,
        binding: HostBindingDescriptor {
            kind: HostBindingKind::Static,
        },
        adapter: HostAdapterDescriptor::Static(crate::abi_impl::unbound_edge_abi_function),
        resource_types: Vec::new(),
    }
}

/// The guest value type an edge host function parameter carries on the wire.
///
/// This is the parameter shape the `#[pd_edge_host_function]` expansion sees
/// in the Rust signature, used only to describe the *edge extension surface*
/// (functions no published catalog declares). Dynamic and open-ended payloads
/// stay [`Self::Unknown`]/[`Self::Map`]; nothing is narrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeGuestValueType {
    /// `Value` / `&Value`: a polymorphic payload.
    Unknown,
    Int,
    Bool,
    String,
    /// `VmMap` / `&VmMap`: an open-ended map.
    ///
    /// No edge host function takes an open map today; the variant exists
    /// because the macro emits it for any `VmMap`/`&VmMap` guest parameter.
    #[allow(dead_code)]
    Map,
}

impl EdgeGuestValueType {
    fn to_schema(self) -> HostTypeSchema {
        match self {
            Self::Unknown => HostTypeSchema::Unknown,
            Self::Int => HostTypeSchema::Int,
            Self::Bool => HostTypeSchema::Bool,
            Self::String => HostTypeSchema::String,
            Self::Map => HostTypeSchema::Map(Box::new(HostTypeSchema::Unknown)),
        }
    }
}

/// Resolves the guest schema of an edge host function from the single
/// authority that declares it.
///
/// 1. A name declared by the published edge ABI spec (the primary source) uses
///    that declaration, and the locally observed guest arity must agree with
///    it.
/// 2. A name that no published catalog declares must be part of the reviewed
///    [`EDGE_EXTENSION_FUNCTIONS`] surface; its schema is then the locally
///    observed parameter list with an untyped return, because the guest
///    compiler resolves such calls dynamically as well.
fn resolve_edge_schema(
    name: &str,
    observed_params: &[(&'static str, EdgeGuestValueType)],
) -> Result<HostFunctionSchema, String> {
    if edge_abi_function(name).is_some() {
        let schema = edge_function_schema(name)?;
        if schema.params.len() != observed_params.len() {
            return Err(format!(
                "edge host function '{name}' implements {} guest parameters but its ABI declaration has {}",
                observed_params.len(),
                schema.params.len()
            ));
        }
        return Ok(schema);
    }
    if !EDGE_EXTENSION_FUNCTIONS.contains(&name) {
        return Err(format!(
            "edge host function '{name}' is not declared in the edge ABI spec; an edge extension \
             must be reviewed and listed in EDGE_EXTENSION_FUNCTIONS"
        ));
    }
    Ok(HostFunctionSchema {
        name: name.to_string(),
        params: observed_params
            .iter()
            .map(|(param_name, value_type)| {
                HostParamSchema::value(*param_name, value_type.to_schema())
            })
            .collect(),
        return_type: HostTypeSchema::Unknown,
        description: String::new(),
    })
}

/// The reviewed edge extension surface.
///
/// These host functions are implemented by the edge runtime but are not part
/// of the published edge ABI declaration set; they extend either a standard
/// core builtin surface or an edge-only helper namespace. Every entry is a
/// deliberate, documented decision, and the test below fails when the surface
/// moves in either direction:
///
/// - `io::*`: the runtime overrides the core IO builtins with edge-scoped
///   implementations (`EdgeHostScope::Io`), including helpers the standard
///   catalog does not publish in every build (`io::exists`, `io::read_line`,
///   `io::popen`, ...).
/// - `http::request::body::*`: downstream request-body streaming helpers used
///   by the checked-in request-transform example, resolved dynamically by the
///   guest compiler (`arity`/`Unknown` imports).
/// - `test::*`: edge-only scaffolding used by the runtime's own tests.
pub(crate) const EDGE_EXTENSION_FUNCTIONS: &[&str] = &[
    "io::close",
    "io::exists",
    "io::flush",
    "io::open",
    "io::popen",
    "io::read_all",
    "io::read_line",
    "io::write",
    "http::request::body::eof",
    "http::request::body::next_chunk",
    "test::sync_return_with_vm",
    "test::yield_pending_tls",
];

/// Builds one edge host function's descriptor from its ABI declaration and
/// runtime binding.
///
/// `binding` and `adapter` must describe the same dispatch class; a mismatch
/// is rejected before the descriptor exists, so a generated wrapper cannot be
/// registered under a binding class it does not implement.
pub(crate) fn edge_function_descriptor(
    name: &str,
    observed_params: &[(&'static str, EdgeGuestValueType)],
    binding: HostBindingKind,
    adapter: HostAdapterDescriptor,
) -> HostFunctionDescriptor {
    let schema = resolve_edge_schema(name, observed_params)
        .unwrap_or_else(|error| panic!("invalid edge host descriptor: {error}"));
    let adapter_kind = adapter_binding_kind(&adapter);
    assert_eq!(
        adapter_kind, binding,
        "edge host function '{name}' declares binding {binding:?} but carries a {adapter_kind:?} adapter"
    );
    HostFunctionDescriptor {
        effects: guest_resource_effects(&schema),
        schema,
        binding: HostBindingDescriptor { kind: binding },
        adapter,
        resource_types: Vec::new(),
    }
}

/// The binding class a concrete adapter implements.
pub(crate) fn adapter_binding_kind(adapter: &HostAdapterDescriptor) -> HostBindingKind {
    match adapter {
        HostAdapterDescriptor::Static(_) => HostBindingKind::Static,
        HostAdapterDescriptor::StaticStack(_) => HostBindingKind::StaticStack,
        HostAdapterDescriptor::StaticStackRuntimeOwned(_) => {
            HostBindingKind::StaticStackRuntimeOwned
        }
        HostAdapterDescriptor::StaticArgs(_) => HostBindingKind::StaticArgs,
        HostAdapterDescriptor::StaticNonYieldingArgs(_) => HostBindingKind::StaticNonYieldingArgs,
        HostAdapterDescriptor::Owned(_) => HostBindingKind::Owned,
    }
}

/// The descriptor-derived guest catalog of the complete published edge ABI.
///
/// Every declared ABI function has exactly one descriptor; functions without a
/// bound implementation in this build carry the *unbound* adapter, which is
/// exactly what the runtime binds for them. Named closed records are part of
/// this catalog, so a second generation is fingerprint-identical.
pub(crate) fn edge_abi_catalog_arc() -> Arc<HostApiCatalog> {
    static CATALOG: OnceLock<Arc<HostApiCatalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            let descriptors = EDGE_ABI_FUNCTIONS
                .iter()
                .map(|function| unbound_edge_descriptor(function.name))
                .collect::<Vec<_>>();
            Arc::new(
                HostFunctionDescriptor::collect_catalog(&descriptors).unwrap_or_else(|error| {
                    panic!("the published edge ABI must produce a valid catalog: {error}")
                }),
            )
        })
        .clone()
}

/// The descriptor-derived guest catalog of the complete published edge ABI.
#[cfg(test)]
pub(crate) fn edge_abi_catalog() -> HostApiCatalog {
    edge_abi_catalog_arc().as_ref().clone()
}

/// Installs one descriptor-driven set of edge adapters into `registry`.
///
/// Validation (ABI contract, uniqueness, raw-handle metadata) runs first.
/// Published ABI names are installed through the frozen core catalog exact-
/// adapter path, using the same catalog snapshot the compiler sees
/// ([`edge_abi_catalog_arc`]). Reviewed extensions stay name-only so the
/// guest compiler's dynamic arity/`Unknown` imports still bind. Both halves
/// share one outer transaction: a rejected set leaves no registry entries,
/// capabilities, or generation bump.
pub(crate) fn install_edge_descriptors(
    registry: &mut HostFunctionRegistry,
    descriptors: &[HostFunctionDescriptor],
) -> VmResult<HostApiCatalog> {
    let mut ordered = descriptors.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.schema.name.cmp(&right.schema.name));
    let mut seen = BTreeSet::new();
    for descriptor in &ordered {
        if !seen.insert(descriptor.schema.name.as_str()) {
            return Err(VmError::HostError(format!(
                "duplicate edge host function '{}'",
                descriptor.schema.name
            )));
        }
        assert_declared_abi_contract(descriptor)?;
    }
    raw_handles::validate_raw_handle_metadata(descriptors).map_err(VmError::HostError)?;
    let catalog = edge_abi_catalog_arc();
    registry.transactionally(|registry| {
        registry.install_named_struct_schemas(catalog_named_struct_schemas(catalog.as_ref()))?;
        for descriptor in descriptors {
            install_one_edge_descriptor(registry, catalog.as_ref(), descriptor)?;
            registry.authorize_registered_builtin_import(&descriptor.schema.name);
        }
        Ok(catalog.as_ref().clone())
    })
}

fn install_one_edge_descriptor(
    registry: &mut HostFunctionRegistry,
    catalog: &HostApiCatalog,
    descriptor: &HostFunctionDescriptor,
) -> VmResult<()> {
    if edge_abi_function(&descriptor.schema.name).is_some() {
        install_abi_descriptor_from_catalog(registry, catalog, descriptor)
    } else {
        install_extension_name_only(registry, descriptor)
    }
}

fn install_abi_descriptor_from_catalog(
    registry: &mut HostFunctionRegistry,
    catalog: &HostApiCatalog,
    descriptor: &HostFunctionDescriptor,
) -> VmResult<()> {
    let matched: Vec<HostImportSchema> = catalog_import_schemas(catalog, &descriptor.schema.name);
    let schema = match matched.as_slice() {
        [schema] => schema.clone(),
        [] => {
            return Err(VmError::HostError(format!(
                "host function '{}' is missing from the edge ABI catalog",
                descriptor.schema.name
            )));
        }
        _ => {
            return Err(VmError::HostError(format!(
                "host function '{}' is ambiguous in the edge ABI catalog",
                descriptor.schema.name
            )));
        }
    };
    install_catalog_adapter(registry, descriptor, schema)
}

fn install_catalog_adapter(
    registry: &mut HostFunctionRegistry,
    descriptor: &HostFunctionDescriptor,
    schema: HostImportSchema,
) -> VmResult<()> {
    let mut runtime_owned_pending: Option<String> = None;
    let result = match (&descriptor.binding.kind, &descriptor.adapter) {
        (HostBindingKind::Static, HostAdapterDescriptor::Static(function)) => {
            registry.register_catalog_static(schema, *function)
        }
        (HostBindingKind::StaticStack, HostAdapterDescriptor::StaticStack(function)) => {
            registry.register_catalog_static_stack(schema, *function)
        }
        (
            HostBindingKind::StaticStackRuntimeOwned,
            HostAdapterDescriptor::StaticStackRuntimeOwned(function),
        ) => {
            runtime_owned_pending = Some(schema.name.clone());
            registry.register_catalog_static_stack(schema, *function)
        }
        (HostBindingKind::StaticArgs, HostAdapterDescriptor::StaticArgs(function)) => {
            registry.register_catalog_static_args(schema, *function)
        }
        (
            HostBindingKind::StaticNonYieldingArgs,
            HostAdapterDescriptor::StaticNonYieldingArgs(function),
        ) => registry.register_catalog_static_non_yielding_args(schema, *function),
        (HostBindingKind::Owned, HostAdapterDescriptor::Owned(factory)) => {
            registry.register_catalog_owned(schema, *factory)
        }
        _ => {
            return Err(VmError::HostError(format!(
                "host function '{}' has a binding/adapter mismatch",
                descriptor.schema.name
            )));
        }
    };
    result.map_err(|error| {
        VmError::HostError(format!(
            "failed to install host function '{}': {error}",
            descriptor.schema.name
        ))
    })?;
    if let Some(name) = runtime_owned_pending {
        registry.mark_exact_runtime_owned_pending(&name)?;
    }
    Ok(())
}

fn install_extension_name_only(
    registry: &mut HostFunctionRegistry,
    descriptor: &HostFunctionDescriptor,
) -> VmResult<()> {
    let name = descriptor.schema.name.as_str();
    let arity = u8::try_from(descriptor.schema.params.len()).map_err(|_| {
        VmError::HostError(format!(
            "edge extension '{name}' has more than 255 parameters"
        ))
    })?;
    match (&descriptor.binding.kind, &descriptor.adapter) {
        (HostBindingKind::Static, HostAdapterDescriptor::Static(function)) => {
            registry.register_static(name, arity, *function);
        }
        (HostBindingKind::StaticStack, HostAdapterDescriptor::StaticStack(function)) => {
            registry.register_static_stack(name, arity, *function);
        }
        (
            HostBindingKind::StaticStackRuntimeOwned,
            HostAdapterDescriptor::StaticStackRuntimeOwned(function),
        ) => {
            registry.register_static_stack(name, arity, *function);
            registry.mark_exact_runtime_owned_pending(name)?;
        }
        (HostBindingKind::StaticArgs, HostAdapterDescriptor::StaticArgs(function)) => {
            registry.register_static_args(name, arity, *function);
        }
        (
            HostBindingKind::StaticNonYieldingArgs,
            HostAdapterDescriptor::StaticNonYieldingArgs(function),
        ) => {
            registry.register_static_non_yielding_args(name, arity, *function);
        }
        _ => {
            return Err(VmError::HostError(format!(
                "edge extension '{name}' has a binding/adapter mismatch"
            )));
        }
    }
    Ok(())
}

/// Rejects a descriptor whose guest contract disagrees with the published ABI
/// declaration for the same name.
///
/// Descriptors derive their schema from that declaration, so this is the
/// fail-closed guard that keeps a future hand-built descriptor (or a stale
/// generated wrapper) from installing a contract the VM compiler cannot see.
fn assert_declared_abi_contract(descriptor: &HostFunctionDescriptor) -> VmResult<()> {
    let name = descriptor.schema.name.as_str();
    if edge_abi_function(name).is_none() {
        if EDGE_EXTENSION_FUNCTIONS.contains(&name) {
            return Ok(());
        }
        return Err(VmError::HostError(format!(
            "edge host function '{name}' is not declared in the edge ABI spec nor reviewed as an \
             edge extension"
        )));
    }
    let declared = edge_function_schema(name)
        .map_err(|error| VmError::HostError(format!("edge host contract mismatch: {error}")))?;
    if declared.params != descriptor.schema.params
        || declared.return_type != descriptor.schema.return_type
    {
        return Err(VmError::HostError(format!(
            "edge host function '{}' does not match its declared ABI contract",
            descriptor.schema.name
        )));
    }
    Ok(())
}

/// Whether an ABI function sits on a raw runtime-owned handle boundary.
#[cfg(test)]
pub(crate) fn is_raw_handle_boundary(function: &AbiFunction) -> bool {
    raw_handles::is_raw_handle_boundary(function)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi_impl::raw_handles::{
        RAW_HANDLE_EFFECTS, RAW_HANDLE_FAMILIES, RawHandleEffect, RawHandleMode, RawHandleSlot,
        raw_handle_effects_for, raw_handle_effects_for_family, raw_handle_metadata_mismatches,
    };

    #[test]
    fn every_declared_abi_function_produces_its_declared_schema() {
        for function in EDGE_ABI_FUNCTIONS {
            let schema =
                edge_function_schema(function.name).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(schema.name, function.name);
            assert_eq!(schema.params.len(), usize::from(function.arity));
            assert_eq!(schema.description, function.docs);
            for (param, (name, ty)) in schema
                .params
                .iter()
                .zip(function.param_names.iter().zip(function.param_types.iter()))
            {
                assert_eq!(&param.name, name);
                assert_eq!(param.ty, edge_param_type_schema(*ty));
                assert_eq!(param.passing, vm::HostParamPassing::Value);
            }
            assert_eq!(
                schema.return_type,
                overlay_named_return(function.name, edge_return_type_schema(function.return_type))
            );
        }
    }

    #[test]
    fn remaining_dynamic_abi_types_match_the_allowlist() {
        let mut map_returns = Vec::new();
        let mut any_params = Vec::new();
        for function in EDGE_ABI_FUNCTIONS {
            if function.return_type == AbiValueType::Map {
                map_returns.push(function.name);
            }
            for (index, param) in function.param_types.iter().enumerate() {
                if *param == AbiParamType::Any {
                    any_params.push((function.name, index));
                }
                assert_ne!(
                    *param,
                    AbiParamType::Map,
                    "'{}' has an unexpected map parameter; add it to the allowlist or type it",
                    function.name
                );
            }
        }
        let remaining_maps: Vec<_> = map_returns
            .iter()
            .copied()
            .filter(|name| *name != "mqtt::connection::read_event")
            .collect();
        assert_eq!(
            remaining_maps, DYNAMIC_MAP_RETURNS,
            "open map returns must stay an explicit allowlist"
        );
        assert_eq!(
            any_params, DYNAMIC_ANY_PARAMS,
            "open any parameters must stay an explicit allowlist"
        );
        for name in DYNAMIC_MAP_RETURNS {
            if let Some(function) = edge_abi_function(name) {
                let schema = edge_function_schema(name).expect("declared schema");
                assert_eq!(
                    schema.return_type,
                    HostTypeSchema::Map(Box::new(HostTypeSchema::Unknown)),
                    "'{name}' is an open map and must stay Map(Unknown)"
                );
                assert_eq!(function.return_type, AbiValueType::Map);
            }
        }
        for (name, index) in DYNAMIC_ANY_PARAMS {
            if let Some(function) = edge_abi_function(name) {
                assert_eq!(function.param_types[*index], AbiParamType::Any);
                let schema = edge_function_schema(name).expect("declared schema");
                assert_eq!(schema.params[*index].ty, HostTypeSchema::Unknown);
            }
        }
    }

    #[test]
    fn webrtc_has_no_map_or_any_wire_types() {
        let mut hits = Vec::new();
        for function in EDGE_ABI_FUNCTIONS {
            if !function.name.starts_with("webrtc::") {
                continue;
            }
            if function.return_type == AbiValueType::Map
                || function.param_types.contains(&AbiParamType::Map)
                || function.param_types.contains(&AbiParamType::Any)
            {
                hits.push(function.name);
            }
        }
        assert!(
            hits.is_empty(),
            "WebRTC has no map/any wire types; observed {hits:?}"
        );
    }

    #[cfg(feature = "mqtt")]
    #[test]
    fn mqtt_read_event_returns_named_mqtt_event() {
        let function = edge_abi_function("mqtt::connection::read_event")
            .expect("mqtt read_event is published");
        assert_eq!(function.return_type, AbiValueType::Map);
        let schema = edge_function_schema("mqtt::connection::read_event").expect("declared schema");
        match schema.return_type {
            HostTypeSchema::Named { name, fields } => {
                assert_eq!(name, "MqttEvent");
                assert_eq!(fields.len(), 8);
                assert_eq!(fields[0].name, "kind");
                assert_eq!(fields[0].ty, HostTypeSchema::String);
            }
            other => panic!("expected Named MqttEvent, got {other:?}"),
        }
        let catalog = edge_abi_catalog();
        assert!(
            catalog
                .structs()
                .iter()
                .any(|record| record.name == "MqttEvent"),
            "the ABI catalog must install the MqttEvent named struct"
        );
    }

    #[test]
    fn edge_abi_catalog_is_deterministic_and_complete() {
        fn derive() -> HostApiCatalog {
            let descriptors = EDGE_ABI_FUNCTIONS
                .iter()
                .map(|function| unbound_edge_descriptor(function.name))
                .collect::<Vec<_>>();
            HostFunctionDescriptor::collect_catalog(&descriptors).unwrap_or_else(|error| {
                panic!("the published edge ABI must produce a valid catalog: {error}")
            })
        }
        let first = derive();
        let second = derive();
        assert_eq!(first.functions().len(), EDGE_ABI_FUNCTIONS.len());
        assert_eq!(
            first.fingerprint(),
            second.fingerprint(),
            "deriving the edge ABI catalog twice must produce one fingerprint"
        );
        assert_eq!(first.fingerprint(), edge_abi_catalog().fingerprint());
    }

    #[test]
    fn undeclared_descriptors_are_rejected() {
        let error = edge_function_schema("edge::not::declared")
            .expect_err("an undeclared name must not produce a schema");
        assert!(error.contains("edge::not::declared"), "{error}");
    }

    #[test]
    fn raw_handle_metadata_is_complete_and_coherent() {
        let mismatches = raw_handle_metadata_mismatches(&EDGE_ABI_FUNCTIONS);
        assert!(
            mismatches.is_empty(),
            "raw-handle metadata drifted: {mismatches:?}"
        );
        for family in RAW_HANDLE_FAMILIES {
            assert!(
                raw_handle_effects_for_family(family).next().is_some(),
                "raw-handle family '{family}' must have a non-vacuous static declaration"
            );
        }
        assert!(
            raw_handle_effects_for("http::response::apply_exchange")
                .next()
                .is_some(),
            "http::response::apply_exchange must declare raw-handle metadata"
        );
        assert!(
            raw_handle_effects_for("http::response::apply_exchange_with_headers")
                .next()
                .is_some(),
            "http::response::apply_exchange_with_headers must declare raw-handle metadata"
        );
    }

    #[test]
    fn raw_handle_take_is_never_labelled_borrow() {
        for effect in RAW_HANDLE_EFFECTS {
            if effect.function.ends_with("::close")
                || effect.function == "mqtt::connection::disconnect"
            {
                assert_eq!(
                    effect.mode,
                    RawHandleMode::Take,
                    "{} close/disconnect must be take, not {:?}",
                    effect.function,
                    effect.mode
                );
                assert!(
                    matches!(effect.slot, RawHandleSlot::Arg(_)),
                    "take/close must name an argument on {}",
                    effect.function
                );
            }
            if effect.mode == RawHandleMode::Create {
                assert_eq!(
                    effect.slot,
                    RawHandleSlot::Return,
                    "create must name the return on {}",
                    effect.function
                );
            }
        }
    }

    #[test]
    fn raw_handle_metadata_rejects_unknown_family_and_pending_capture() {
        let unknown = RawHandleEffect {
            function: "tcp::stream::close",
            slot: RawHandleSlot::Arg(0),
            family: "not-a-family::",
            mode: RawHandleMode::Take,
        };
        assert!(
            !RAW_HANDLE_FAMILIES.contains(&unknown.family),
            "the unknown-family probe must stay outside the allowlist"
        );
        let mut registry = HostFunctionRegistry::empty();
        let mut descriptor = edge_function_descriptor(
            "runtime::exit",
            &[],
            HostBindingKind::StaticStack,
            HostAdapterDescriptor::StaticStack(|_vm, _args| {
                Err(VmError::HostError("unused".to_string()))
            }),
        );
        descriptor.binding = HostBindingDescriptor {
            kind: HostBindingKind::StaticStackRuntimeOwned,
        };
        descriptor.adapter = HostAdapterDescriptor::StaticStackRuntimeOwned(|_vm, _args| {
            Err(VmError::HostError("unused".to_string()))
        });
        let error = install_edge_descriptors(&mut registry, &[descriptor])
            .expect_err("runtime-owned pending without metadata must fail");
        assert!(
            error.to_string().contains("runtime-owned pending"),
            "{error}"
        );
        assert!(!registry.contains_name("runtime::exit"));
    }

    #[test]
    fn raw_handle_boundary_excludes_plain_counters() {
        for name in ["console::stdout::write", "http::response::get_status"] {
            let function = edge_abi_function(name)
                .unwrap_or_else(|| panic!("{name} should be part of the default edge ABI"));
            assert_eq!(function.return_type, AbiValueType::Int);
            assert!(
                !is_raw_handle_boundary(function),
                "{name} returns a counter, not a runtime-owned handle token"
            );
        }
    }
}
