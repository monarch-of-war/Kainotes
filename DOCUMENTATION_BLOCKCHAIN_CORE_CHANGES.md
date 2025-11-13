Title: Changes Required After blockchain-core Modifications

Overview

This document enumerates the changes required across the repository to make the blockchain system fully functional after the user's modifications to the `blockchain-core` crate. It is written as a clear, actionable prompt suitable for an LLM coding agent to implement the changes. The document contains: a) assumptions, b) a small "contract" describing expected inputs/outputs and success criteria, c) a prioritized task list per crate with concrete tasks and acceptance criteria, d) tests and CI checklist, e) a specification prompt for creating a `blockchain-wallet` Rust crate, and f) a specification prompt for a TypeScript `wallet-bridge` (NPM package) that connects applications to the blockchain via the wallet.

Assumptions

1. The user modified `blockchain-core` (internals) but did not update all dependent crates. The changes may include structural changes to Block, Transaction, State, or public APIs.
2. The repository uses Cargo and Rust edition consistent with existing Cargo.toml files in the workspace.
3. The rest of the crates (consensus, networking, node, rpc, smart-contracts, storage, tokenomics, liquidity, blockchain-crypto) expect the original `blockchain-core` API. Where incompatible changes exist, dependent crates must be updated or adapter shims introduced.
4. No external service changes are assumed (e.g., no consensus algorithm swap beyond API surface changes). Any changes requiring database migrations will be handled via migration scripts reported below.

Contract (for the LLM coding agent)

- Inputs: repository at root (workspace described to the agent), modified `blockchain-core` crate source, Cargo manifest files for every crate.
- Outputs: code edits across crates to restore compile and integration correctness, new `blockchain-wallet` crate, new `wallet-bridge` TypeScript package scaffold, tests, and a final green build and test run on local machine.
- Data shapes: where types in `blockchain-core` changed (e.g., Block, Transaction, State), ensure consistent serialization (serde) and wire-level compatibility for P2P messages and RPC responses. If incompatible, propose and implement migration strategies and adapter types.
- Error modes: handle missing trait implementations, mismatched type aliases (e.g., Hash/Address types), failed serializations, and runtime panics in state transitions. Provide graceful error conversions and explicit well-documented panics only where unrecoverable.
- Success criteria: 1) cargo build workspace passes, 2) unit and integration tests included here pass, 3) node can start a local devnet (if a small devnet script is present), 4) RPC endpoints for basic chain operations are available, and 5) the wallet crate + TypeScript bridge compile and expose required APIs.

Top-level priority and approach

1. Discover and list all compiler errors caused by the modifications. Prefer automated `cargo build --workspace` to enumerate failures.
2. For each error: classify as API change (type/signature), trait/impl missing, or logical bug (failing test/runtime). Resolve in-place or through adapters.
3. Prefer minimal, low-risk changes: adapt callers rather than revert major changes in `blockchain-core` unless requested.
4. Add comprehensive unit tests and end-to-end integration test(s) that exercise block production, transaction submission, mempool handling, consensus integration point, P2P sync, and RPC methods.
5. Implement the `blockchain-wallet` crate and the `wallet-bridge` TypeScript adapter as separate commits with clear tests and usage examples.

Detailed tasks per crate (prioritized)

1) blockchain-core (verify & document)
- Task: Identify all exported types and public APIs modified by the user. Create an API compatibility report listing type changes: renamed fields, changed types, removed functions, trait signature changes, serde attribute changes.
- Acceptance: A short markdown file in the crate (API_CHANGES.md) summarizing each change and the reasoning when available.
- Task: Ensure every public type used across crates has stable serde derives and explicit versioned wire formats where appropriate. If any wire format changed, add `#[serde(alias = "...")]` or versioned enum variants and document migration.
- Acceptance: No serde-derived serialization errors; cargo test for the crate passes.

2) blockchain-crypto
- Task: Confirm hash types, keypair/signature APIs, and merkle root helpers still match `blockchain-core` expectations. If `blockchain-core` changed a Hash type alias or changed expected signature method names, update this crate accordingly or provide adapter traits that implement previous APIs.
- Acceptance: Compilation passes and signature verification flows are exercised in unit tests.

3) consensus
- Task: Inspect the consensus crate for uses of block validation, state transitions, and selection logic that rely on `blockchain-core` types. Update trait implementations and function calls to match new signatures or introduce adapter wrappers that convert between old/new shapes without modifying logic.
- Acceptance: Consensus unit tests pass; consensus can validate sample blocks.

4) networking
- Task: Verify P2P message types and wire protocol structures used in gossip/p2p modules. Update serialization and message handlers to accept the new `blockchain-core` block/tx representations or add serialization adapters that map to the new shapes.
- Acceptance: Networking compiles and a local p2p handshake (unit/integration test) completes without serialization deserialization errors.

5) node
- Task: Update the node runtime wiring, config, and initializers to match any constructor changes for the core data types. Ensure runtime.rs and main.rs call updated APIs correctly and that the node starts without panics.
- Acceptance: `cargo run -p node` (development run) starts and logs a ready state (or passes a smoke test that checks startup exit code 0).

6) rpc
- Task: Check that RPC request and response types (server.rs, methods.rs, types.rs) serialize and deserialize using the new shapes. Update method handlers to convert to/from internal types as necessary.
- Acceptance: The RPC test harness (or a small integration test that calls e.g., chain head, block by hash, submit tx) returns the expected shapes.

7) smart-contracts
- Task: Confirm VM and gas accounting still link correctly with state types. If the state representation changed, ensure precompiles and VM state accesses are adapted.
- Acceptance: Contract tests (if existing) pass; otherwise add a minimal contract execution test.

8) storage
- Task: Check DB key formats and serialization of persisted types (blocks, transactions, state snapshots). If the format changed, add a migration strategy and a one-shot migration utility that reads old-format DB (if present) and writes new-format DB.
- Acceptance: Storage unit tests pass and migration utility is documented.

9) tokenomics, liquidity and other app crates
- Task: Update any direct uses of core types, e.g., transaction processing, token transfer, supply/mint functions, to match new definitions. Verify business logic compiles and unit tests for tokenomics pass.
- Acceptance: Each crate compiles and key unit tests execute.

Cross-cutting tasks

- Update Cargo.toml dependencies: if `blockchain-core` bumped its version or changed public features, ensure dependent crates in the workspace reference the compatible path or version. Use workspace path dependencies where appropriate to avoid version drift.
- Add adapter modules (e.g., crates/common_adapters) only if necessary: prefer localized adapters inside dependent crates for clarity.
- Add feature flags for breaking changes behind opt-in features (e.g., new-block-format) and a migration guide.
- Update or add doc comments to key public APIs explaining behavior and expected invariants.

Wire-level compatibility and migrations

- If block or transaction binary wire formats changed in a non-backwards-compatible way, implement versioned message envelopes containing a format version integer. Update networking and RPC to advertise supported versions and include compatibility checks during handshake.
- Provide a migration plan for persisted data in `storage`: list steps to backup DB, run migration utility, verify migration, and optional rollback plan.

Testing plan (minimum)

1. Unit tests: add missing unit tests for any changed public function signatures.
2. Integration tests: one test that runs a node with a memory DB, creates one validator, produces a few blocks, submits a transaction, and checks final state change.
3. RPC tests: small test that starts rpc server and calls a few endpoints: chain head, block by hash, submit tx.
4. Mempool tests: ensure mempool accepts, rejects or reorders txs as expected.
5. CI: Add a job that runs `cargo build --workspace` and `cargo test --workspace` with a Rust toolchain pinned or a rust-toolchain file added.

CI and Linting

- Add or update the CI workflow to run build and tests for all crates in the workspace. Include Rustfmt/Clippy runs and fail the job on warnings if desired.
- Ensure a minimal rust-toolchain file exists (add if missing) to reduce environment drift.

Deliverables for the coding agent (explicit checklist)

- Run `cargo build --workspace`, collect all errors, and create an errors.json summarizing errors by crate (type mismatch, missing trait impl, etc.).
- Implement fixes (adapters or updates) to restore successful `cargo build --workspace`.
- Add/modify tests listed in the Testing plan. Run `cargo test --workspace` and iterate until green.
- Add an API_CHANGES.md in `blockchain-core` describing the exact changes introduced by the user and the chosen mitigation (adapter/rename/backwards-compatible wrapper or migration).
- Implement migration utility for storage if wire format changed, with docs on usage and rollback.
- Commit changes in small commits with clear commit messages; open a PR per major area (core fixes, networking updates, wallet, wallet-bridge).

Prompt for creating the `blockchain-wallet` Rust crate (LLM coding-agent prompt)

Goal: Create a new crate named `blockchain-wallet` providing secure wallet functionality to manage keys, sign transactions, derive addresses, and optionally run as an in-process signer for local nodes or as a remote signing service.

Requirements (no code in this prompt; this is a spec for the implementer):

- Responsibilities:
  - Key management: generate keys from mnemonic seed (BIP39 style) and manage multiple accounts.
  - Address derivation: provide commonly used derivation paths and a deterministic address format consistent with `blockchain-core` address/hash types.
  - Signing: provide functions to sign raw transactions and typed transaction objects compatible with `blockchain-core` transaction types.
  - Verification: verify signatures given a transaction and a public key/address.
  - Secure storage: store encrypted key material on disk using a local keystore format (e.g., password-encrypted) and provide an interface to export/import keys securely.
  - JSON-RPC signing server (optional submodule): a tiny local HTTP/WS server that exposes signing endpoints to trusted clients (for integration testing and local wallets).
  - CLI: optional minimal CLI to create wallets, list addresses, sign sample transactions, and run the signing server (for dev use).
- API surface (description):
  - Synchronous and asynchronous APIs for key management and signing to support both blocking CLI and async node integration.
  - A small trait abstraction for Signer that the node and RPC server can depend on; implement a file-based keystore and an in-memory test signer.
- Security:
  - Keys must be encrypted at rest; do not log private keys.
  - Support user passphrase and optional KDF parameters.
  - Provide clear warning text in docs about production key management and advice to integrate hardware wallets or secure enclaves.
- Tests:
  - Unit tests for key generation/derivation, sign/verify round-trip, keystore encryption/decryption.
  - Integration test that signs a transaction and the node accepts it (use local test signer in memory).
- Documentation:
  - A README describing usage, security notes, and the JSON-RPC signing API if the server is implemented.

Prompt for creating the TypeScript `wallet-bridge` (NPM package) (LLM coding-agent prompt)

Goal: Create a TypeScript library named `wallet-bridge` that can be used by front-end or Node.js apps to interact with the `blockchain-wallet` signing service or directly with a local wallet implementation. It will be the bridge enabling TypeScript apps to build and sign transactions and submit them to the network.

Requirements (spec for implementer):

- Features:
  - Connect to a JSON-RPC HTTP/WS signing server or a local Node.js native addon (if desired) to request signing operations.
  - Provide high-level utilities to build transactions compatible with `blockchain-core` transaction shapes (JS friendly objects) and serialize them for submission.
  - Expose secure signing flows: request signature, present an approval callback to the UI, and return signed transaction bytes ready for submission.
  - Derive addresses from a given public key format and validate addresses.
  - Provide TypeScript types and JSDoc for developer experience.
  - Provide a small CLI or example app that demonstrates how to: derive an address from a mnemonic, prepare a transaction, request signing via the bridge, and submit via the existing RPC server.
- API design (conceptual):
  - Constructor accepts a connection config object (host/port/ws/http, auth token, CORS settings for browsers, fallback strategies).
  - Methods: connect(), disconnect(), getAccounts(), signTransaction(tx), signMessage(message), sendSignedTransaction(signedTx), on(event, handler) for status updates.
  - Errors: robust typed errors for connectivity issues, auth, user-rejected-signature, invalid-payload.
- Security considerations:
  - When used in browsers, avoid exposing private keys; prefer connecting to a remote signing service or hardware wallet via WebUSB/WebHID.
  - Provide guidance for using secure channels and token-based authentication for the signing server.
- Packaging & tests:
  - Provide a package.json, type declarations, unit tests (Jest or vitest), small integration/demo script to exercise full flow.

How the LLM agent should implement the Wallet + Bridge

1. Implement `blockchain-wallet` with a Signer trait and at least two implementations: file-keystore signer and in-memory test signer.
2. Add a minimal JSON-RPC HTTP/WS server module inside `blockchain-wallet` exposing methods to sign transactions and messages and to list addresses.
3. Add strong type validations and use the same canonical serialization used by `blockchain-core` when creating the signing digest.
4. Implement the TypeScript `wallet-bridge` library to speak the signing server protocol and perform local preflight checks. Include a small example app that demonstrates sign-and-submit.

PR and commit guidance for the LLM agent

- Make small focused commits per crate or responsibility.
- Add CHANGELOG entries and update Cargo.toml where version bumps are necessary.
- For each PR: include a short summary, list of files changed, why the change is safe, and testing notes.

Acceptance criteria and verification steps (final checks)

1. `cargo build --workspace` exits 0.
2. `cargo test --workspace` passes all included tests written as part of the tasks above.
3. Node run smoke test: start node (dev) and it initializes, accepts RPC calls for chain head and transaction submission.
4. Wallet integration: run the TypeScript demo to sign a sample transaction using the signing server and successfully submit the signed transaction via RPC to a running dev node.
5. Deliver an API_CHANGES.md describing the `blockchain-core` modifications and how they were fixed or adapted.

Files to create (recommended)

- /crates/blockchain-core/API_CHANGES.md (describes changes and migration plan)
- /crates/blockchain-core/tests/compat_tests.rs (unit/integration tests for serialization)
- /crates/storage/migration_tool.rs (use only if migration required)
- /crates/blockchain-wallet/ (new crate root with README, tests, and optional signing server)
- /packages/wallet-bridge/ (TypeScript package scaffold with README and tests)
- /docs/MIGRATIONS.md (high-level migration docs for operators)

Notes and guidance for the LLM implementer

- When in doubt, prefer preserving data compatibility. If backwards compatibility cannot be preserved safely, highlight the reason and provide a migration with a clear rollback path.
- Keep changes minimal and well-tested. Provide clear documentation strings for every public function changed.
- If a change is invasive (e.g., changing the block merkle structure), coordinate the change across networking, consensus, storage, and RPC simultaneously in the same PR to avoid partial breakage.

End of spec
