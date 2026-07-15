# Native WAF Execution Plan — pd-edge

> **For Hermes:** Integrate only after pd-edge-waf differential tests and pd-vm plan validation are complete.

**Goal:** Load, publish, pool, execute, and observe native WAF plan artifacts alongside existing VMBC programs.

**Architecture:** `LoadedProgram` gains an optional validated rule-plan artifact owned by pd-vm. Program and plan publication is atomic and content-addressed. Request handlers construct typed transaction input, invoke the plan executor, and map the typed decision into the existing response/proxy flow.

**Tech Stack:** Rust, ArcSwap, pd-vm rule-plan API, existing admin upload and VM pool infrastructure.

---

## Stage 1: Versioned artifact upload

### Task 1: Define the upload envelope

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/admin.rs` or the current admin route owner
- Test: existing admin/runtime tests

**Requirements:**
- Envelope contains VMBC, optional native plan, schema version, and content hashes.
- Validate both artifacts before publication.
- Reject partial or incompatible uploads without replacing the active program.

### Task 2: Content-addressed publication

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/runtime/vm_runner.rs`

**Requirements:**
- Re-upload of identical VMBC and plan hashes keeps the existing loaded object and VM pool.
- Changed artifacts create a new loaded object.
- No trace reuse across different hashes.

## Stage 2: Request execution path

1. Build typed transaction input from the existing HTTP request context.
2. Execute the native plan before proxy forwarding.
3. Map block status, score, HTTP status, and matched IDs into current response headers.
4. Retain RSS execution as fallback during rollout.
5. Add a runtime selector for RSS, native plan, and differential shadow mode.

## Stage 3: Pooling and observability

1. Share immutable plan/operator assets across all VMs for the loaded program.
2. Keep VM-local dynamic assets local.
3. Add counters for native-plan requests, rules evaluated, regex asset count, block point, execution latency, and RSS/native mismatches.
4. Remove any separate warmed-pool design unless measurements show a scheduling issue.

## Verification

```bash
cargo fmt --check
cargo test --release
cargo clippy --all-targets --all-features -- -D warnings
```

From pd-edge-waf:

```bash
cargo test --release --test e2e
```

## Acceptance criteria

- Artifact publication is atomic.
- Identical uploads retain the exact loaded object and pool.
- Native and RSS decisions can run in shadow mode and report mismatches.
- Blocked requests never reach upstream.
- Existing VMBC-only uploads remain compatible.
