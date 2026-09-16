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
//! ## Intentional dynamic boundaries (do not "fix" these)
//!
//! The edge ABI is a published wire contract (ABI version 24, shared with the
//! `pd-edge-abi` crate the VM compiler links). Its parameter and return types
//! are the ABI's coarse wire types, and several are *intentionally* dynamic:
//!
//! - `AbiParamType::Any` / `AbiValueType::Unknown` -> [`HostTypeSchema::Unknown`]:
//!   polymorphic payloads (`http::request::get_header` values, exchange
//!   bodies, proxy callbacks) whose shape is chosen by the guest program.
//! - `AbiParamType::Map` / `AbiValueType::Map` -> `Map(Unknown)`; `Array` ->
//!   `Array(Unknown)`: header dictionaries, JSON bodies, and payload batches
//!   whose keys are genuinely open-ended.
//!
//! Fixed-shape records in the edge ABI (HTTP head records, TLS handshake
//! records, WebSocket/MQTT/WebRTC connection records, transport handle
//! records) travel as maps of documented fields on this wire, so they stay
//! `Map(Unknown)` here: the wire shape *is* the guest contract, and re-typing
//! it in the descriptor alone would fork the contract from what the VM
//! compiler sees. Their field sets are documented next to the runtime readers
//! in `src/abi_impl/**` and in `docs/EDGE_HOST_DESCRIPTOR_MIGRATION.md`.
//!
//! ## Raw handle-token boundaries
//!
//! Edge host handles (exchange, TCP/UDP/TLS/WebSocket/MQTT/WebRTC/IO/proxy
//! handles, callbacks) travel as `int` wire values owned by the runtime scope
//! state, not as catalog resources: `HostTypeSchema::Resource` would require
//! the published ABI to declare matching resource keys, which it does not, and
//! emulating them as a guest resource handle is forbidden by the migration
//! policy. These boundaries are therefore documented and pinned by
//! [`is_raw_handle_boundary`] plus [`RAW_HANDLE_BOUNDARY_FAMILY_COUNTS`] and
//! the tests in this module, which fail whenever the boundary moves.

use std::collections::BTreeSet;

#[cfg(test)]
use edge_abi::FUNCTIONS as EDGE_ABI_FUNCTIONS;
use edge_abi::{AbiFunction, AbiParamType, AbiValueType};
use vm::host_extension::guest_resource_effects;
use vm::{
    HostAdapterDescriptor, HostApiCatalog, HostBindingDescriptor, HostBindingKind,
    HostFunctionDescriptor, HostFunctionRegistry, HostFunctionSchema, HostParamSchema,
    HostTypeSchema, VmError, VmResult,
};

/// Maps an ABI parameter wire type onto the descriptor schema language.
///
/// The mapping is a projection of the published contract, never a
/// strengthening of it: `Any` stays [`HostTypeSchema::Unknown`] and open
/// collections stay `Map(Unknown)`/`Array(Unknown)`.
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

/// The declared ABI entry for `name`, or `None` when the name is not part of
/// the published edge ABI.
pub(crate) fn edge_abi_function(name: &str) -> Option<&'static AbiFunction> {
    edge_abi::function_by_name(name)
}

/// Guest schema of one edge host function, derived from its ABI declaration.
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
        return_type: edge_return_type_schema(function.return_type),
        description: function.docs.to_string(),
    })
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
/// exactly what the runtime binds for them. The catalog is therefore a pure
/// derivation of the ABI declaration set, and it is the snapshot the installed
/// implementation descriptors are validated against.
#[cfg(test)]
pub(crate) fn edge_abi_catalog() -> HostApiCatalog {
    let descriptors = EDGE_ABI_FUNCTIONS
        .iter()
        .map(|function| {
            // The published declaration is authoritative for the whole ABI
            // surface; the observed list only pins the arity.
            let observed = function
                .param_names
                .iter()
                .map(|name| (*name, EdgeGuestValueType::Unknown))
                .collect::<Vec<_>>();
            edge_function_descriptor(
                function.name,
                &observed,
                HostBindingKind::Static,
                HostAdapterDescriptor::Static(crate::abi_impl::unbound_edge_abi_function),
            )
        })
        .collect::<Vec<_>>();
    HostFunctionDescriptor::collect_catalog(&descriptors).unwrap_or_else(|error| {
        panic!("the published edge ABI must produce a valid catalog: {error}")
    })
}

/// Installs one descriptor-driven set of edge adapters into `registry`.
///
/// The complete set is validated first (schemas, guest resource effects,
/// duplicates) and installed second, in one transaction: a rejected descriptor
/// leaves the registry untouched. Adapters keep the edge dispatch identity the
/// published ABI uses — the guest calls an edge import by name and arity —
/// while every schema, binding, and adapter value comes from the descriptor,
/// which is itself derived from the ABI declaration.
pub(crate) fn install_edge_descriptors(
    registry: &mut HostFunctionRegistry,
    descriptors: &[HostFunctionDescriptor],
) -> VmResult<HostApiCatalog> {
    let catalog = HostFunctionDescriptor::collect_catalog(descriptors).map_err(|error| {
        VmError::HostError(format!("edge host descriptors are invalid: {error}"))
    })?;
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
    registry.transactionally(|registry| {
        for descriptor in &ordered {
            install_edge_descriptor(registry, descriptor)?;
        }
        Ok(())
    })?;
    Ok(catalog)
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

/// Registers one validated descriptor under the edge dispatch identity.
fn install_edge_descriptor(
    registry: &mut HostFunctionRegistry,
    descriptor: &HostFunctionDescriptor,
) -> VmResult<()> {
    let name = descriptor.schema.name.clone();
    let arity = u8::try_from(descriptor.schema.params.len()).map_err(|_| {
        VmError::HostError(format!(
            "edge host function '{name}' declares more than 255 parameters"
        ))
    })?;
    match (&descriptor.binding.kind, &descriptor.adapter) {
        (HostBindingKind::StaticStack, HostAdapterDescriptor::StaticStack(function)) => {
            registry.register_static_stack(name, arity, *function);
        }
        (HostBindingKind::StaticArgs, HostAdapterDescriptor::StaticArgs(function)) => {
            registry.register_static_args(name, arity, *function);
        }
        (binding, adapter) => {
            return Err(VmError::HostError(format!(
                "edge host function '{name}' cannot be installed: binding {binding:?} carries a {:?} adapter",
                adapter_binding_kind(adapter)
            )));
        }
    }
    Ok(())
}

/// Namespace families whose ABI functions exchange raw runtime-owned handle
/// tokens (created by the runtime, resolved by the runtime scope state, never
/// by a guest resource handle).
#[cfg(test)]
pub(crate) const RAW_HANDLE_NAMESPACES: &[&str] = &[
    "tcp::",
    "udp::",
    "tls::",
    "websocket::",
    "mqtt::",
    "webrtc::",
    "proxy::",
    "http::exchange::",
];

/// Whether an ABI function sits on a raw runtime-owned handle boundary.
///
/// The rule is intentionally structural: the function belongs to a
/// handle-owning namespace family *and* its contract carries an `int` in its
/// parameters or return, i.e. a token the runtime must resolve. Counter-style
/// `int` returns outside those families (`console::stdout::write`,
/// `http::response::platform_status`) are not part of the boundary.
#[cfg(test)]
pub(crate) fn is_raw_handle_boundary(function: &AbiFunction) -> bool {
    let in_handle_family = RAW_HANDLE_NAMESPACES
        .iter()
        .any(|prefix| function.name.starts_with(prefix));
    in_handle_family
        && (function.return_type == AbiValueType::Int
            || function.param_types.contains(&AbiParamType::Int))
}

/// Every ABI function on a raw handle boundary.
#[cfg(test)]
pub(crate) fn raw_handle_boundary_functions() -> Vec<&'static AbiFunction> {
    EDGE_ABI_FUNCTIONS
        .iter()
        .filter(|function| is_raw_handle_boundary(function))
        .collect()
}

/// The pinned size of every raw handle-token family (see
/// [`is_raw_handle_boundary`]).
///
/// Each handle family is the reviewed record of how many ABI functions within
/// it exchange a runtime-owned handle token. A count that changes means the
/// boundary moved: a function was added, removed, or started/stopped carrying
/// an `int` handle, and the change must be reviewed and recorded here.
///
/// Families that are empty in the current feature set (for example `mqtt::`
/// without the `mqtt` feature) are skipped, so one build checks only the
/// families it compiles.
#[cfg(test)]
pub(crate) const RAW_HANDLE_BOUNDARY_FAMILY_COUNTS: &[(&str, usize)] = &[
    ("tcp::", 19),
    ("udp::", 17),
    ("tls::", 21),
    ("websocket::", 21),
    ("mqtt::", 19),
    ("webrtc::", 19),
    ("proxy::", 8),
    ("http::exchange::", 27),
];

#[cfg(test)]
mod tests {
    use super::*;

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
                edge_return_type_schema(function.return_type)
            );
        }
    }

    #[test]
    fn dynamic_abi_types_stay_dynamic() {
        let mut checked = 0usize;
        for function in EDGE_ABI_FUNCTIONS {
            let schema = edge_function_schema(function.name).expect("declared schema");
            for (index, param) in schema.params.iter().enumerate() {
                match function.param_types[index] {
                    AbiParamType::Any => {
                        assert_eq!(
                            param.ty,
                            HostTypeSchema::Unknown,
                            "'{}' must keep its `any` parameter dynamic",
                            function.name
                        );
                        checked += 1;
                    }
                    AbiParamType::Map => {
                        assert_eq!(
                            param.ty,
                            HostTypeSchema::Map(Box::new(HostTypeSchema::Unknown)),
                            "'{}' must keep its open map parameter dynamic",
                            function.name
                        );
                        checked += 1;
                    }
                    _ => {}
                }
            }
        }
        assert!(
            checked > 0,
            "the ABI surface must contain at least one intentionally dynamic parameter"
        );
    }

    #[test]
    fn edge_abi_catalog_is_deterministic_and_complete() {
        let first = edge_abi_catalog();
        let second = edge_abi_catalog();
        assert_eq!(first.functions().len(), EDGE_ABI_FUNCTIONS.len());
        assert_eq!(
            first.fingerprint(),
            second.fingerprint(),
            "deriving the edge ABI catalog twice must produce one fingerprint"
        );
    }

    #[test]
    fn undeclared_descriptors_are_rejected() {
        let error = edge_function_schema("edge::not::declared")
            .expect_err("an undeclared name must not produce a schema");
        assert!(error.contains("edge::not::declared"), "{error}");
    }

    #[test]
    fn raw_handle_boundary_families_match_the_documented_counts() {
        let mut observed = Vec::new();
        for family in RAW_HANDLE_NAMESPACES {
            let count = raw_handle_boundary_functions()
                .iter()
                .filter(|function| function.name.starts_with(family))
                .count();
            observed.push((*family, count));
        }
        let mismatch = observed
            .iter()
            .filter(|(family, count)| {
                let expected = RAW_HANDLE_BOUNDARY_FAMILY_COUNTS
                    .iter()
                    .find(|(name, _)| name == family)
                    .map(|(_, expected)| *expected)
                    .unwrap_or(0);
                *count != 0 && *count != expected
            })
            .copied()
            .collect::<Vec<_>>();
        assert!(
            mismatch.is_empty(),
            "the raw handle boundary moved; update RAW_HANDLE_BOUNDARY_FAMILY_COUNTS.\
             \nobserved={observed:?}\ndocumented={RAW_HANDLE_BOUNDARY_FAMILY_COUNTS:?}"
        );
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
