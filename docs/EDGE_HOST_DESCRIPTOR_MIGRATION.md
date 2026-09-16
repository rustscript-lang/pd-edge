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

## Intentional dynamic boundaries

The edge ABI is a published wire contract (ABI version 24, shared with the
`pd-edge-abi` crate the VM compiler links). The following stay dynamic in the
descriptor schema and must not be "fixed":

| ABI wire type | Descriptor schema | Retained because |
|---|---|---|
| `any` / `unknown` | `HostTypeSchema::Unknown` | Polymorphic payloads: `http::request::get_header` values, exchange bodies, proxy callbacks. Their shape is chosen by the guest program. |
| `map` | `Map(Unknown)` | Header dictionaries, JSON bodies, and MQTT/WebRTC event maps whose keys are genuinely open-ended. |
| `array` | `Array(Unknown)` | Payload batches and byte buffers of dynamic length. |

Fixed-shape *records* (HTTP head records, TLS handshake records,
WebSocket/MQTT/WebRTC connection records, transport handle records) travel as
maps of documented fields on this wire. Re-typing them in the descriptor alone
would fork the contract from what the VM compiler sees, so they stay
`Map(Unknown)`; their field sets are documented next to the runtime readers in
`src/abi_impl/**`.

## Raw handle-token boundaries

Edge host handles (exchange, TCP/UDP/TLS/WebSocket/MQTT/WebRTC/proxy handles)
travel as `int` wire values owned by the runtime scope state. They are *not*
catalog resources: `HostTypeSchema::Resource` would require the published ABI
to declare matching resource keys, which it does not, and emulating them as a
guest resource handle is forbidden by the migration policy.

The boundary is documented and pinned by
`src/abi_impl/descriptor.rs::is_raw_handle_boundary` /
`RAW_HANDLE_BOUNDARY_FAMILY_COUNTS` and the
`raw_handle_boundary_families_match_the_documented_counts` test, which fails
whenever a handle family gains or loses a member:

| Family | Functions on the boundary |
|---|---:|
| `tcp::` | 19 |
| `udp::` | 17 |
| `tls::` | 21 |
| `websocket::` | 21 |
| `mqtt::` | 19 |
| `webrtc::` | 19 |
| `proxy::` | 8 |
| `http::exchange::` | 27 |

The `io::*` overrides are handle-producing as well; they are part of the
reviewed extension surface above.

## Registry installation

`src/abi_impl/registry.rs` builds one registry per scope mask:

1. every declared ABI function is registered with the unbound placeholder
   adapter (the behavior the ABI promises for a scope this build does not
   implement), then
2. the scope's implemented functions are installed from their descriptors in
   one transaction, ordered by guest name.

Adapters keep the edge dispatch identity the ABI publishes (name + arity, which
is what compiled guest programs carry: edge imports are compiled without full
schemas), while schema, binding class, and adapter all come from the
descriptor. A rejected descriptor set leaves the registry untouched.

## Fixture status

`sweep` (`tests/rss_fixture_compile_tests.rs`) compiles every checked-in `.rss`
file programmatically and fails if the count changes. The MQTT broker sample
`examples/mqtt/downstream/sample_transport_mqtt_broker_program.rss` is pinned as
a documented frozen-core incompatibility: the frozen core's stricter guest
inference rejects its hand-written dynamic packet decoder
(`BinaryOperandTypeMismatch: int vs unknown`). The entry must be removed — and
the sample re-typed — if the core stops rejecting it.
