# Utility-Backed Blockchain Protocol - Rust Implementation

A novel blockchain protocol implementing Proof-of-Active-Stake (PoAS) consensus with utility-driven tokenomics, where validator stakes actively fuel network utility rather than remaining idle.

## 🏗️ Architecture Overview

This implementation is structured as a Rust workspace with multiple specialized crates:

### Core Crates (Currently Implemented)

#### 1. `blockchain-crypto`
Cryptographic primitives and operations:
- **Hash Functions**: SHA256, SHA3-256, Blake3
- **Digital Signatures**: Ed25519 and SECP256k1
- **Key Management**: KeyPair, PublicKey, SecretKey
- **Merkle Trees**: Efficient data verification
- **Address Derivation**: Ethereum-style addresses

#### 2. `blockchain-core`
Core blockchain data structures and logic:
- **Blocks**: Block and BlockHeader structures
- **Transactions**: Multiple transaction types (Transfer, Stake, Unstake, Liquidity operations)
- **State Management**: WorldState and Account management
- **Blockchain**: Main chain logic with validation
- **Types**: Amount, Gas, Nonce, UtilityScore

### Future Crates (To Be Implemented)

#### 3. `consensus`
Proof-of-Active-Stake consensus mechanism:
- Validator selection algorithm
- Stake weighting based on utility contribution
- Slashing conditions and penalties
- Block production and finalization

#### 4. `tokenomics`
Dual-phase economic model:
- **Phase 1**: Bootstrap minting (adoption-driven)
- **Phase 2**: Utility-driven minting
- Utility index calculation
- Reward distribution
- Token burning mechanisms

#### 5. `liquidity`
Active liquidity deployment:
- Utility pool management
- Liquidity deployment strategies
- Risk-adjusted returns
- Yield generation tracking

#### 6. `smart-contracts`
EVM-compatible smart contract execution:
- Contract deployment and execution
- Gas metering
- State management
- Precompiled contracts

#### 7. `networking`
P2P networking layer:
- Block propagation
- Transaction broadcasting
- Peer discovery
- Chain synchronization

#### 8. `storage`
Persistent data storage:
- RocksDB/Sled integration
- State pruning
- Efficient indexing
- Archive nodes support

#### 9. `rpc`
JSON-RPC API server:
- Ethereum-compatible endpoints
- Custom protocol methods
- WebSocket support
- Event subscriptions

#### 10. `node`
Full node implementation:
- Configuration management
- Runtime coordination
- CLI interface
- Monitoring and metrics

## 🚀 Getting Started

### Prerequisites

```bash
# Install Rust (1.70+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Verify installation
rustc --version
cargo --version
```

### Building

```bash
# Clone the repository
git clone <repository-url>
cd utility-blockchain

# Build all crates
cargo build --release

# Run tests
cargo test --all

# Build specific crate
cargo build -p blockchain-crypto --release
```

### Running Examples

```bash
# Run crypto examples
cargo run --example keypair_generation

# Run blockchain examples
cargo run --example create_genesis
```

## 📋 Current Implementation Status

### ✅ Completed

- [x] Cryptographic primitives (hashing, signatures, keypairs)
- [x] Merkle tree implementation
- [x] Address derivation
- [x] Block structure and validation
- [x] Transaction types and signing
- [x] Account and state management
- [x] Basic blockchain logic
- [x] State root calculation
- [x] **Consensus mechanism (PoAS)** ⭐
  - [x] Validator registration and management
  - [x] Weighted validator selection
  - [x] Slashing conditions and penalties
  - [x] Security metrics (Nakamoto coefficient, Gini)
- [x] **Tokenomics implementation** ⭐
  - [x] Dual-phase minting (Bootstrap & Utility-Driven)
  - [x] Utility index calculation
  - [x] Phase transition management
  - [x] Reward distribution
  - [x] Token burning mechanisms
- [x] **Liquidity Management** ⭐
  - [x] Active liquidity deployment
  - [x] Risk assessment and optimization
  - [x] AMM pools (constant product formula)
  - [x] Lending/borrowing protocols
  - [x] Network treasury with milestone grants
- [x] **Smart Contracts (EVM)** ⭐
  - [x] EVM-compatible execution environment
  - [x] Contract deployment and calls
  - [x] Gas metering (Ethereum-compatible)
  - [x] Precompiled contracts (9 standard precompiles)
  - [x] State management and storage
- [x] **P2P Networking** ⭐
  - [x] Peer discovery and management
  - [x] Block propagation
  - [x] Transaction broadcasting
  - [x] Sync management
  - [x] Gossip protocol

### 🚧 In Progress

- [ ] Storage layer (RocksDB/Sled)
- [ ] RPC API server

### 📅 Planned

- [ ] Full node orchestration
- [ ] Comprehensive testing suite
- [ ] Performance benchmarks
- [ ] Documentation and examples

## 🔧 Key Features

### Proof-of-Active-Stake (PoAS)

Unlike traditional PoS where stakes remain idle:
- Validators stake tokens to participate
- Staked tokens are **actively deployed** as liquidity
- Selection weight based on: `Stake × Utility_Score × Uptime`
- Validators earn both protocol rewards and DeFi yields

### Dual-Phase Tokenomics

**Phase 1: Bootstrap** (Adoption-Driven)
```rust
Validator_Reward = Base_Reward × (Validator_Stake / Total_Network_Stake) × Time_Staked
```

**Phase 2: Utility-Driven**
```rust
Network_Mint_Rate = Base_Rate × Utility_Index
Utility_Index = Σ(Metric_i × Weight_i) / Baseline_i
```

Utility metrics include:
- Transaction volume (30%)
- Total Value Locked (25%)
- Unique active addresses (20%)
- Smart contract interactions (15%)
- Cross-chain bridging (10%)

### Transaction Types

```rust
pub enum TransactionType {
    Transfer { to: Address, amount: Amount },
    Stake { amount: StakeAmount },
    Unstake { amount: StakeAmount },
    DeployLiquidity { pool_id: u64, amount: Amount },
    WithdrawLiquidity { pool_id: u64, amount: Amount },
    ContractDeployment { bytecode: Vec<u8>, constructor_args: Vec<u8> },
    ContractCall { contract: Address, data: Vec<u8> },
}
```

## 📚 Documentation

### Module Documentation

```bash
# Generate and open documentation
cargo doc --open --no-deps

# Generate docs for specific crate
cargo doc -p blockchain-core --open
```

### Examples

See the `examples/` directory for usage examples:
- Basic cryptographic operations
- Creating and signing transactions
- Building blocks
- Managing state

## 🧪 Testing

```bash
# Run all tests
cargo test --all

# Run tests for specific crate
cargo test -p blockchain-crypto

# Run tests with output
cargo test -- --nocapture

# Run integration tests
cargo test --test integration_tests
```

## 🎯 Benchmarks

```bash
# Run benchmarks
cargo bench

# Benchmark specific operations
cargo bench --bench crypto_bench
```

## 📊 Performance Characteristics

- **Block Time**: 3 seconds
- **Finality**: 2 blocks (~6 seconds)
- **TPS**: 10,000+ (with Layer 2 scaling)
- **Smart Contracts**: EVM-compatible

## 🔐 Security Features

- Ed25519 and SECP256k1 signature schemes
- Multiple hash function support (SHA256, SHA3, Blake3)
- Merkle proof verification
- Slashing for malicious behavior
- Multi-signature validator controls
- Circuit breakers for abnormal activities

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run `cargo fmt` and `cargo clippy`
5. Submit a pull request

### Code Style

```bash
# Format code
cargo fmt --all

# Run linter
cargo clippy --all -- -D warnings
```

## 📄 License

This project is dual-licensed under Apache-2.0.

## 🗺️ Roadmap

### Q4 2025
- [x] Core data structures
- [x] Cryptographic primitives
- [ ] Consensus mechanism
- [ ] Testnet launch

### Q1 2026
- [ ] Smart contract support
- [ ] P2P networking
- [ ] Mainnet Phase 1 (Bootstrap)

### Q2 2026
- [ ] DApp integrations
- [ ] Ecosystem grants program
- [ ] Cross-chain bridges

### Q3 2026
- [ ] Reach Initial Volume Threshold (IVT)
- [ ] Layer 2 scaling solutions

### Q4 2026
- [ ] Phase 2 transition (Utility-Driven)
- [ ] Enterprise partnerships

## 📞 Contact

- Website: [To be added]
- GitHub: [Repository URL]
- Discord: [Community link]
- Twitter: [Social media handle]

## 🙏 Acknowledgments

Special thanks to the teams behind:
- Rust programming language
- Ethereum and related projects
- Cryptographic libraries (ed25519-dalek, secp256k1)
- All open-source contributors

---

Built with ❤️ using Rust