# pd-edge

`pd-edge` is the edge data-plane runtime plus the edge ABI split from the original `project-d` history.

## Repository split

- RustScript core VM and standard library: https://github.com/rustscript-lang/rustscript
- CLR VM: https://github.com/rustscript-lang/rustscript-clr-vm
- Edge runtime and ABI: https://github.com/rustscript-lang/pd-edge
- Controller: https://github.com/rustscript-lang/pd-controller

## Local crates

The split keeps VM and ABI crates local so the edge runtime test suite can run without unpublished remote dependencies.

For downstream Cargo manifests, the intended repository references are:

```toml
pd-vm = { git = "https://github.com/rustscript-lang/rustscript", package = "pd-vm" }
pd-edge-abi = { git = "https://github.com/rustscript-lang/pd-edge", package = "pd-edge-abi" }
pd-edge = { git = "https://github.com/rustscript-lang/pd-edge", package = "pd-edge" }
```

## Test

```bash
cargo test --workspace --jobs 4
cargo build --workspace --release --jobs 4
```
