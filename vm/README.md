# Quyn VM (QVM)

EVM-compatible execution via revm. This crate is **excluded from the workspace** until the revm dependency conflict (revm-primitives 2.x vs 3.x across revm, revm-interpreter, revm-precompile) is resolved.

To re-enable: add `"vm"` back to workspace members in the root `Cargo.toml` and ensure a single revm version is used (e.g. revm 7+ or a compatible revm 4/5/6 with aligned sub-deps). The executor and ABI code in this crate are ready once revm builds.
