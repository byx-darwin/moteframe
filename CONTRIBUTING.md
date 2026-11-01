# Contributing

Use the pinned toolchain and keep runtime crates no_std, allocation-free,
unsafe-forbidden and independent of chip, board and product types. Build
synthetic host fixtures; do not add device dumps, product artwork or third-party
resources without a verified redistribution license.

Public APIs require docs, error behavior and Debug implementations. Preserve
finite wait obligations and first-error/cleanup semantics. Test behavior and
boundary failures rather than mirroring the implementation. Run:

```sh
make check build example packages
make supply-chain secrets hooks
```

Shared checks run offline with locked dependencies. Install pre-commit,
cargo-deny, typos and gitleaks separately when needed. Use `make hooks-install`
to activate pre-commit and Conventional Commit validation. Commits use subjects
such as `fix(display): preserve failure after cleanup`.

First-party contributions are accepted under Apache-2.0. Preserve applicable
copyright and source notices and mark modified files as required by section 4.
Every crate must carry the root LICENSE and NOTICE; package auditing rejects a
missing notice or an unintended alternative first-party license file. Commercial
redistribution must carry the applicable notice and license too. Dependencies
retain their respective licenses.

Cargo registry publication is disabled in every package. GitHub source publication,
crates.io uploads and releases are separate owner decisions. Passing CI does not
constitute hardware, radio or timing qualification.
