# Edge host descriptor and effect migration

This repository is pinned to the frozen RustScript descriptor core
`2f433edab3f1907e8cb060e901357d7e2af016ce` (a git dependency for `pd-vm` and
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

## Named closed records (ABI version 26)

Closed fixed-shape returns are overlayed as `HostTypeSchema::Named` on the
compiler catalog. Runtime values remain maps. The overlay changes the catalog
fingerprint tag, so it is an explicit ABI-versioned change.

| Function | Named schema | Runtime representation |
|---|---|---|
| `mqtt::connection::read_event` | `MqttEvent`: always-present `kind` plus `Option<T>` for every variant-specific member | map with canonical `Option` carriers |

Compile options install this catalog so the guest compiler sees the named
contract. The coarse `pd-edge-abi` wire type for `read_event` remains `map`.
A member the active variant does not carry is an explicit `None`
carrier, never a missing map entry and never a nullable (`optional<T>`) field:
the frozen compiler rejects a nullable field on a host output and requires the
canonical `Option` carrier for an absent member.

Published ABI namespaces (`runtime`, `tcp`, `http`, …) are exact catalog
modules. A guest file of the same name is not a host-namespace fallback;
local modules still compile when their namespace is not in the catalog.

## Fallible optional-text getters (ABI version 26)

A getter for a member that may be absent publishes
`Result<Option<string>, string>` instead of returning an empty string:

| Function | Guest contract |
|---|---|
| `http::request::get_header` | `Ok(None)` absent, `Ok(Some(""))` present-but-empty, `Err` value is not text |
| `http::request::get_query_arg` | same; a repeated key keeps the first value |
| `http::response::get_header` | same |
| `http::response::get_trailer` | same |
| `http::exchange::get_header` | same |

The wire tag is `result_option_string`; the carrier is the canonical
`Result`/`Option` map pair, so absence is never a bare Null and an undecodable
value is never an empty-string sentinel. An invalid header or trailer name
stays a fatal host error. Fields that always exist (`method`, `path`, `uri`,
`version`, `status`, ...) keep returning `string`/`int`.

## Core IO readers overridden by the edge runtime (ABI version 26)

`io::read_all` and `io::read_line` are edge overrides of core builtins: the
runtime keeps its own handles, but the guest contract is the one the frozen
compiler resolves from the core IO module. Both are published as explicit
results:

| Function | Guest contract |
|---|---|
| `io::read_all` | `Result<string, string>`: the remaining text, or the guest error |
| `io::read_line` | `Result<Option<string>, string>`: `Ok(None)` at end of stream, `Ok(Some(""))` for a blank line, one trailing `\n` stripped |
| `io::write` | `Result<int, string>`: bytes written |
| `io::flush` | `Result<(), string>` |
| `io::close` | `Result<(), string>` |

A handle or transport failure stays a fatal host error. The embedded
`http::upstream::response` wrapper propagates both text contracts (`next_line`,
`read_line`, `read_all`) instead of collapsing them.

`io::open` stays the one edge override whose value cannot match its core
contract: core publishes `Result<io.file resource, string>`, while the edge
runtime returns a raw virtual-handle `int` (see the raw-handle metadata
family). Minting a core `io.file` resource is not possible from edge scope, so
the open result keeps the raw token and no guest is currently typed against it.

## Intentional dynamic boundaries

The edge ABI is a published wire contract (ABI version 26, shared with the
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
