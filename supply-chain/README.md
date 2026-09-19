# Supply chain

CI runs `cargo deny check` and `cargo vet --locked` on every pull request.
`cargo vet` answers one question per dependency: who, if anyone, has looked at
this code? This file says how Keryx answers it, and where the answer is still
"nobody".

## How a crate gets past cargo vet

In order of preference:

1. **An imported audit.** `config.toml` imports the audit sets published by
   Mozilla, Google, the Bytecode Alliance, Embark, ISRG, Zcash, Fermyon and
   Ariel OS. `imports.lock` pins what was imported, so `cargo vet --locked`
   needs no network.
2. **Trust in a publisher.** `audits.toml` records `[[trusted]]` entries: we
   accept a named publisher's releases of a named crate. The rule is strict on
   purpose. Trust is recorded per crate, never with `--all`, and only where
   **at least two** of the imported organisations already trust that
   publisher. Trust is delegation, not review.
3. **Our own audit.** `[[audits]]` entries in `audits.toml`, each read in full
   and written up in its notes. The `who` field says who actually read it,
   including when that was an AI assistant.
4. **An exemption.** `[[exemptions]]` in `config.toml` record that a crate is
   unreviewed. An exemption is not a review and must never be described as one.

## Where things stand (0.6.0)

| | Crates |
|---|---|
| Fully audited, through imports, trust or our own audits | 261 |
| Exempted | 465 (501 versions) |
| of which are in `Cargo.lock` but never compiled | 65 |

The 65 are optional dependencies of something Keryx uses with the feature
off, for example the Arrow crates behind SeaORM's `with-arrow`. Cargo locks
them and cargo vet therefore asks about them, but they never reach a build.

## Knowingly unreviewed

These are large, have no public audit, and are not realistic to read in
full. They are exempted with open eyes, not overlooked.

| Family | Why it is here | Main crates |
|---|---|---|
| PDF rendering | `keryx publish`. The largest unaudited tree, and it predates 0.6.0 | fulgur, blitz-dom, stylo, krilla, resvg, fontique |
| Terminal UI | `keryx tui` | ratatui, termwiz, crossterm |
| Blob storage | S3 and the disk backend | opendal-core, opendal-service-s3, opendal-http-transport-reqwest, reqsign-aws-v4, reqsign-aws-core, reqsign-core |
| Database | SQLite and Postgres | sea-orm, sea-orm-migration, sea-query, sea-schema, sqlx, sqlx-core, sqlx-sqlite, sqlx-postgres, libsqlite3-sys |
| OCI sharing | `keryx share`, `pull`, `inspect` | oci-client, oci-spec, jsonwebtoken, http-auth |
| Platform TLS trust | reqwest 0.13 verifies against the OS store | rustls-platform-verifier, rustls-native-certs, security-framework, schannel, jni |
| Cryptography | TLS and Web Push | ring, plus the RustCrypto curve and AEAD crates |

Mitigations that do not depend on review: every dependency is declared once
in `[workspace.dependencies]` with `default-features = false` and an explicit
feature list; crates.io releases younger than 14 days are refused at resolve
time; `cargo deny` blocks unknown registries and git sources, known
advisories and unapproved licences; and `aws-lc-rs` is kept out of the graph
by a CI check.

`stylo` is built from the SimCubeLtd fork pinned by revision (see
`[patch.crates-io]` in the root `Cargo.toml`). `[policy.stylo]` sets
`audit-as-crates-io`, so the fork is held to the same standard as the
crates.io release rather than trusted as first-party code.

## Working with it

Use cargo-vet 0.10.2 or newer (`cargo install cargo-vet --version 0.10.2
--locked`), the version CI pins. Older releases, including the 0.10.0 prebuilt
binary, cannot parse the `trusted-publisher` entries newer versions write to
`imports.lock`.

Adding or updating a dependency:

```sh
cargo vet                      # what is unvetted now?
cargo vet suggest              # smallest things to review first, with trust hints
cargo vet inspect <crate> <version>
cargo vet certify <crate> <version>    # only for code you actually read
cargo vet regenerate exemptions        # last resort, for what is left
cargo vet prune                        # drop exemptions that are no longer needed
```

Put `supply-chain/` changes in their own commit, before the commit that
changes `Cargo.lock`, so every commit passes `cargo vet --locked`. Never write
exemptions or audits by hand.

Refreshing imported audits picks up reviews others have published since:

```sh
cargo vet            # without --locked, fetches the imports
cargo vet prune
```

Worth doing before each release: upstream audits of the families above would
shrink the unreviewed set without anyone here reading ten million lines.
