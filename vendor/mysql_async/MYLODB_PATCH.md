# mysql_async 0.36.2 with one fix

A copy of the published crate (`src/`, `Cargo.toml`, licences, readme) under its own MIT OR Apache-2.0 licence, used through
`[patch.crates-io]` in the workspace `Cargo.toml`.

**Changed:** `src/io/tls/rustls_io.rs` only. `skip_domain_validation` (the *Verify CA* SSL mode) looked for the text
`NotValidForName`, which current rustls no longer prints, so a certificate that did not name the host was refused. The error is now
matched by its variant, as in upstream pull request https://github.com/blackbeam/mysql_async/pull/408 (merged 2026-09-21).

**Remove this folder** and the patch entry once MyLoDB moves to a mysql_async release that contains that fix (0.37.1 does not).
`crates/myq-driver/tests/ssl.rs` has a test that fails without it.
