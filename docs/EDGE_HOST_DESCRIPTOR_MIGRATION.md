# Edge host descriptor and effect migration

This repository is pinned to the frozen RustScript descriptor core
`b1d6cffede77f49410bf63525f30b9a46b02dc01` (a git dependency for `pd-vm` and
`pd-host-function`). This note records how the edge host surface is derived
after the migration, and which boundaries are intentionally *not* typed.

## One contract source per host function

```text
pd-edge-abi/src/abi_spec/**          src/abi_impl/**
(core #[pd_host_function])           (#[pd_edge_host_function])
        │                                     │
        │  guest schema, effects, docs        │  runtime binding: adapter + binding class
        ▼                                     ▼
        edge_abi::FUNCTIONS  ───────►  HostFunctionDescriptor  ◄─── descriptor factory
                                     (schema + binding + adapter)
                                              │
                                              ▼
                                       edge host registry
```

* The **guest contract** of every edge host function is declared exactly once,
  in `pd-edge-abi/src/abi_spec/**`, where each function carries the core
  `#[pd_host_function(name = "...")]` attribute. The ABI generator publishes it
  as `edge_abi::FUNCTIONS` (name, parameter names/types, return type, docs) and
  as the checked-in `pd-edge-abi/abi.json` manifest (regenerated from a
  `--all-features` build; a test fails when it drifts).
* The **runtime binding** is declared exactly once, by
  `#[pd_edge_host_function(name = ..., scope = ...)]` on the implementation.
  The macro emits a `HostFunctionDescriptor` factory whose schema is resolved
  from the ABI declaration and whose adapter is the generated static wrapper.
* `src/abi_impl/descriptor.rs` joins both halves, validates the join (a binding
  class that the adapter does not implement, an arity that disagrees with the
  ABI declaration, or a name that no catalog declares all fail closed), and
  installs the adapters transactionally.
* The linkme inventory (`PD_EDGE_HOST_FUNCTIONS`) carries **discovery metadata
  only**: the scope and the descriptor factory. Name, arity, documentation, and
  adapter come from the descriptor, so the inventory cannot drift.

## Edge extension surface (reviewed, not ABI-declared)

These host functions are implemented by the edge runtime but are not part of
the published ABI declaration set. They are listed in
`src/abi_impl/descriptor.rs::EDGE_EXTENSION_FUNCTIONS` and a test fails when the
surface moves in either direction:

| Family | Why it is an extension |
|---|---|
| `io::open`, `io::close`, `io::read_all`, `io::read_line`, `io::write`, `io::flush`, `io::exists`, `io::popen` | The edge runtime overrides the core IO builtins (`EdgeHostScope::Io`); several helpers are not published by the standard catalog in every build. |
| `http::request::body::eof`, `http::request::body::next_chunk` | Downstream request-body streaming helpers used by the checked-in request-transform sample; the guest compiler resolves them dynamically (`arity` + `Unknown`). |
| `test::sync_return_with_vm`, `test::yield_pending_tls` | Edge-only scaffolding used by the runtime's own tests. |

Their descriptor schema is the observed parameter list with an untyped return —
exactly how the guest compiler represents such calls. They must not be moved
into the published ABI without a versioned ABI change.

## Named closed records (ABI version 25)

Closed fixed-shape returns are overlayed as `HostTypeSchema::Named` on the
compiler catalog. Runtime values remain maps. The overlay changes the catalog
fingerprint tag, so it is an explicit ABI-versioned change.

| Function | Named schema | Runtime representation |
|---|---|---|
| `mqtt::connection::read_event` | `MqttEvent` (`kind` plus optional publish/close/fail fields) | map |

Compile options install this catalog so the guest compiler sees the named
contract. The coarse `pd-edge-abi` wire type for `read_event` remains `map`.

Published ABI namespaces (`runtime`, `tcp`, `http`, …) are exact catalog
modules. A guest file of the same name is not a host-namespace fallback;
local modules still compile when their namespace is not in the catalog.

## Intentional dynamic boundaries

The edge ABI is a published wire contract (ABI version 25, shared with the
`pd-edge-abi` crate the VM compiler links). The following stay dynamic and are
pinned by an allowlist test. WebRTC has no map/any wire types.

| Family | Functions | Why it stays dynamic |
|---|---|---|
| HTTP request headers | `http::request::get_headers` | Open header dictionary. |
| HTTP request query | `http::request::get_query_args` | Open query-argument map. |
| HTTP response headers | `http::response::get_headers` | Open header dictionary. |
| HTTP response trailers | `http::response::get_trailers` | Open trailer dictionary. |
| HTTP exchange headers | `http::exchange::get_headers` | Open header dictionary. |
| HTTP exchange trailers | `http::exchange::get_trailers` | Open trailer dictionary. |
| HTTP header-batch `any` | `http::response::set_headers`, `http::response::apply_exchange_with_headers`, `http::exchange::prepare_default_upstream` | Guest-chosen header batches. |
| `array` | payload batches | Dynamic-length collections. |

## Raw handle-token boundaries

Edge host handles (exchange, TCP/UDP/TLS/WebSocket/MQTT/WebRTC/proxy handles)
travel as `int` wire values owned by the runtime scope state. They are *not*
catalog resources: `HostTypeSchema::Resource` would require the published ABI
to declare matching resource keys, which it does not, and emulating them as a
guest resource handle is forbidden by the migration policy.

Guest resource effects cannot represent raw `int` tokens without changing the
wire schema. Enforceable function-local metadata lives in
`src/abi_impl/raw_handles.rs` (`RAW_HANDLE_EFFECTS`): each handle argument and
return names its family and mode (`Borrow`, `BorrowMut`, `Take`, `Create`,
`RuntimeOwnedPending`). Completeness tests fail for a missing/extra function,
wrong slot/type/mode, unknown family, or pending capture without a declaration.
Feature-disabled families keep non-vacuous static declarations.

`http::response::apply_exchange` and
`http::response::apply_exchange_with_headers` are part of the table.

The `io::*` overrides are handle-producing as well; they are part of the
reviewed extension surface above.

## Registry installation

`src/abi_impl/registry.rs` builds one registry per scope mask from a complete
descriptor list (implemented adapters, unbound ABI placeholders, reviewed
extensions) and installs it through
`HostModuleDescriptor::install_descriptors`. That path registers catalog exact
adapters and, inside the same transaction, authorizes each installed import so
`HostFunctionRegistry::restricted()` grants exactly those names. A rejected
set leaves no registry entries, capabilities, or generation bump. Linkme
remains discovery-only.

## Fixture status

`sweep` (`tests/rss_fixture_compile_tests.rs`) compiles every checked-in `.rss`
file programmatically (26 files) and fails if the count changes. There is no
accepted-error allowlist.
