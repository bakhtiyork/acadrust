# CadKit fork notes

This is the CadKit fork of [acadrust](https://github.com/hakanaktt/acadrust) (MPL-2.0), branch `cadkit`, started from
upstream tag `v0.6.5` (289a616, identical to the crates.io 0.6.5 sources apart from line endings). Its goal is "same
data" as [ACadSharp](https://github.com/DomCR/ACadSharp): a read yields the same document as ACadSharp, checked by
canonical dumps against the C# library, and written files read back the same in ACadSharp. The upstream Rust API stays.

Every behaviour change to upstream code is recorded here with its reason. ACadSharp-specific representation lives in
`src/compat/`. Upstream is merged in by tag, never rebased.

## Build setup

- `Cargo.lock` is committed on this branch (upstream ignores it) so tests and the CadKit bridge build the same
  dependency set, offline from `cargo vendor`.

## Behaviour changes

| Area | Change | Reason |
|---|---|---|
