# Review

## Main.rs

The `main.rs` file serves as the entry point for the actor application. It handles:

1. Command-line argument parsing and configuration loading
2. Initialization of the error handling system
3. Execution of different commands:
   - `start`: Launches the node with specified configuration
   - `init`: Initializes a new node with default configuration
   - `testnet`: Sets up a test network
   - `distributed-testnet`: Creates a distributed test network
   - `dump-wal`: Dumps the Write-Ahead Log (WAL) contents

Integrates with the actor system through the `ActorNode` implementation. It supports both file-based and default configurations, and handles the setup of logging, metrics, and runtime environments.

## Detailed Implementation

### Configuration Management
- Supports multiple configuration sources:
  - File-based configuration
  - Default configuration
  - Value-based configuration
- Handles home directory and config file paths
- Manages genesis and private key files

### Command Execution
1. **Start Command**
   - Loads node configuration
   - Initializes logging with specified level and format
   - Sets up metrics if enabled
   - Builds and runs the runtime environment
   - Launches the node with specified start height

2. **Init Command**
   - Creates initial node configuration
   - Generates necessary files:
     - Genesis file
     - Private validator key file
     - Configuration file

3. **Testnet Commands**
   - `testnet`: Creates a local test network
   - `distributed-testnet`: Sets up a distributed test network across multiple machines
   - Both commands initialize nodes with default configurations

4. **Dump WAL Command**
   - Uses ProtobufCodec for WAL operations
   - Dumps the contents of the Write-Ahead Log

### Runtime Environment
- Configures logging levels and formats
- Sets up metrics collection
- Manages runtime parameters
- Handles graceful shutdown

## Spawn.rs

The `spawn.rs` file implements the actor spawning and initialization.

### Core Functionality

#### Actor Spawning
- Manages actor creation and initialization
- Handles actor configuration
- Controls actor lifecycle
- Implements actor linking

#### Actor Types
1. **Mempool Actors**
   - `MempoolNetwork`: Network communication for mempool
   - `Mempool`: Core transaction pool management
   - `MempoolLoad`: Transaction load generation

2. **Network Actors**
   - `Network`: P2P networking for consensus

3. **Host Actor**
   - `Host`: Block creation and proposal handling

4. **Consensus Actors**
   - `Consensus`: Consensus protocol implementation

5. **Other Actors**
   - `Wal`: Write-Ahead Log management
   - `Sync`: Block/ Value synchronization

#### Actor Configuration
1. **Network Configuration**
   - Peer discovery settings
   - Connection parameters
   - Message handling options
   - Network topology

2. **Mempool Configuration**
   - Pool size limits
   - Transaction batching
   - Load generation
   - Network integration

3. **Host Configuration**
   - Block size limits
   - Proposal parameters
   - Storage settings
   - State management

4. **Consensus Configuration**
   - Validator set
   - Initial height
   - Timeout settings
   - Threshold
   - Value payload mode
   - ..

#### Actor Linking
- Establishes communication channels
- Sets up message routing
- Manages actor dependencies
- Controls actor hierarchy

## Node.rs

The `node.rs` file implements the core node functionality for the actor-based application. It provides the infrastructure for running and managing nodes in the network.

### Core Components

#### ActorNode Structure
- Manages node configuration and state
- Handles home directory and configuration sources
- Supports custom start heights
- Implements the Node trait for standard node operations

#### Configuration Management
- Supports multiple configuration sources:
  - File-based configuration
  - Default configuration
  - Value-based configuration
- Manages file paths for:
  - Genesis configuration
  - Private validator keys
  - Node configuration

#### Node Implementation
1. **Configuration Loading**
   - Loads configuration from specified sources
   - Handles default configurations
   - Manages file-based configurations

2. **Key Management**
   - Handles private key generation
   - Manages public key derivation
   - Supports keypair creation
   - Implements key file operations

3. **Genesis Management**
   - Loads genesis configuration
   - Manages validator sets
   - Handles initial state setup

#### Node Operations
1. **Start Operation**
   - Initializes node configuration
   - Sets up logging and metrics
   - Creates necessary directories
   - Spawns node actors
   - Manages node lifecycle

2. **Node Handle**
   - Provides event subscription
   - Manages node lifecycle
   - Handles graceful shutdown
   - Controls node state

#### Test Network Support
1. **Local Testnet**
   - Creates single-machine test networks
   - Configures local node instances
   - Sets up test environment

2. **Distributed Testnet**
   - Supports multi-machine test networks
   - Manages distributed configurations
   - Handles cross-machine communication

#### Actor System
The node implementation spawns several actors that work together to provide the complete node functionality:

1. **Mempool Actors**
   - `MempoolNetwork`: Handles P2P communication for mempool
     - Manages peer connections
     - Handles message broadcasting
     - Implements gossip protocol
     - Dedicated network layer for transaction propagation
   - `Mempool`: Core mempool functionality
     - Manages transaction pool
     - Handles transaction batching
     - Controls transaction limits
   - `MempoolLoad`: Simulates transaction load
     - Generates test transactions
     - Controls load patterns
     - Manages transaction flow

2. **Network Actors**
   - `Network`: Manages P2P networking for consensus
     - Handles peer discovery
     - Manages connections
     - Implements gossip protocol
     - Controls message routing
     - Dedicated network layer for consensus messages
     - Separate from mempool networking to ensure isolation

3. **Host Actor**
   - `Host`: Core node functionality
     - Manages block creation and assembly
     - Handles proposal parts and streaming
     - Manages block store and part store
     - Coordinates with consensus for block finalization
     - Handles validator operations through the MockHost implementation
     - Manages transaction execution simulation
     - Controls block pruning and retention

4. **Consensus Actors**
   - `Consensus`: Implements consensus protocol
     - Manages consensus rounds
     - Handles voting
     - Controls block finalization
     - Manages validator set

5. **Storage Actors**
   - `Wal`: Write-Ahead Log
     - Persists consensus state
     - Handles crash recovery
   - `Sync`: State synchronization
     - Handles block synchronization

6. **Node Actor**
   - `Node`: Top-level coordination
     - Manages actor lifecycle
     - Coordinates between components
     - Handles shutdown
     - Controls node state

Each actor is spawned with specific configurations and dependencies:
- Actors are linked together for communication
- Each has its own metrics and logging
- Actors can be monitored and controlled
- Graceful shutdown is supported
- Error handling is implemented at each level


## Actor.rs

The `actor.rs` file implements the core Host actor functionality that coordinates between different components of the node. It serves as the central coordinator for block creation, proposal handling, and consensus coordination.

### Core Functionality

#### Message Handling
- Implements the Actor trait for the Host
- Handles various message types:
  - `ConsensusReady`: Initializes consensus (with the start height) when ready
    - Called when consensus system is ready to start
    - Determines the next block height to start from
    - Initializes the consensus with the current validator set
    - Ensures proper state transition from initialization to active consensus
  
  - `StartedRound`: Manages round transitions
    - Updates the current height and round
    - Sets the proposer for the current round
    - Updates the node's role in the current round
    - Replays any undecided values from previous rounds
  
  - `GetHistoryMinHeight`: Provides block history information
    - Returns the minimum height of blocks in the block store
    - Used for block synchronization
    - Helps determine the range of available blocks
  
  - `GetValue`: Handles block proposal creation
    - Creates new block proposals when the node is proposer
    - Coordinates with mempool to get transactions
    - Manages proposal part streaming
    - Handles proposal assembly and validation
  
  - `RestreamValue`: Manages proposal part streaming
    - Re-broadcasts the parts of a known to peers
  
  - `ReceivedProposalPart`: Processes incoming proposal parts
    - Receives and validates proposal parts from peers
    - Assembles complete proposals from parts
    - Manages part storage and ordering
    - Coordinates with consensus for proposal validation when all parts are received
  
  - `GetValidatorSet`: Provides validator information
    - Returns the current validator set
    - Used by consensus for voting and validation
    - Ensures proper validator set management
  
  - `Decided`: Handles block finalization
    - Processes consensus decisions on blocks
    - Updates block store with finalized blocks
    - Coordinates with mempool for transaction cleanup
    - Manages block pruning and retention
  
  - `GetDecidedValue`: Retrieves finalized blocks
    - Returns finalized blocks from the block store
    - Used for block synchronization
  
  - `ProcessSyncedValue`: Handles block synchronization
    - Processes blocks received during block sync
    - Validates synced blocks
    - Updates local state with synced blocks

#### Block Management
1. **Proposal Creation**
   - Builds new block proposals
   - Manages proposal parts
   - Handles proposal streaming
   - Coordinates with mempool for transactions

2. **Block Assembly**
   - Collects proposal parts
   - Assembles complete blocks
   - Validates block structure
   - Manages block storage

3. **Block Finalization**
   - Processes consensus decisions
   - Updates block store
   - Manages block pruning
   - Coordinates with mempool for transaction cleanup

#### State Management
- Maintains current height and round
- Tracks proposer information
- Manages validator set
- Controls block store state
- Handles part store management

#### Integration Points
1. **Consensus Integration**
   - Coordinates with consensus protocol
   - Handles proposal validation
   - Controls block finalization

2. **Network Integration**
   - Manages proposal part streaming

3. **Mempool Integration**
   - Coordinates transaction selection
   - Manages transaction execution
   - Handles transaction cleanup
   - Controls mempool updates

### MockHost Implementation
The Host actor uses a MockHost implementation to simulate real blockchain operations:

1. **Configuration**
   - `MockHostParams`: Controls simulation parameters
     - `max_block_size`: Maximum size of blocks
     - `txs_per_part`: Number of transactions per proposal part
     - `time_allowance_factor`: Time allowance for operations
     - `exec_time_per_tx`: Simulated execution time per transaction
     - `max_retain_blocks`: Maximum number of blocks to retain

2. **Core Functionality**
   - `build_new_proposal`: Creates new block proposals
     - Coordinates with mempool for transactions
     - Manages proposal part creation
     - Handles proposal streaming
     - Simulates transaction execution time
   
   - `send_known_proposal`: Re-broadcasts existing proposals
     - Retrieves stored proposal parts
     - Manages proposal part streaming
     - Handles proposal re-broadcasting
   
   - `decision`: Processes consensus decisions
     - Updates local state
     - Manages block finalization
     - Coordinates with other components

3. **State Management**
   - Maintains part store for proposal parts
   - Manages transaction execution simulation
   - Controls block assembly process
   - Handles proposal part storage

4. **Integration Points**
   - Coordinates with mempool for transactions
   - Manages proposal part streaming
   - Handles block assembly
   - Controls transaction execution simulation

The MockHost implementation provides a controlled environment for testing and development, simulating real blockchain operations while maintaining deterministic behavior.

# Mempool.rs

The `mempool.rs` file implements the transaction pool management system, handling transaction collection, validation, and distribution across the network.
It currently gets the transactions from the `mempool_load` actor (see below)

## Core Components

### Mempool Actor
- Manages the transaction pool
- Handles transaction batching
- Controls transaction limits
- Implements the Actor trait for message handling

### Message Types
1. **Network Events**
   - `NetworkEvent`: Handles P2P network events
     - Peer connections/disconnections
     - Message reception
     - Network state changes

2. **Transaction Management**
   - `AddBatch`: Adds new transaction batches
     - Validates transactions
     - Updates transaction pool
     - Manages batch processing
   
   - `Reap`: Retrieves transactions for block proposals
     - Selects transactions based on height
     - Controls number of transactions
     - Manages transaction selection
   
   - `Update`: Updates transaction pool state
     - Removes processed transactions
     - Updates transaction status
     - Manages pool cleanup

### State Management
- Maintains transaction pool
- Tracks transaction status
- Manages transaction lifecycle
- Controls pool size limits

### Network Integration
- Coordinates with MempoolNetwork
- Handles P2P communication
- Manages transaction broadcasting

### Transaction Processing
1. **Validation**
   - Validates transaction format
   - Checks transaction limits
   - Manages pool capacity
   - Controls batch sizes

2. **Pool Management**
   - Maintains transaction order
   - Controls pool size
   - Manages transaction removal
   - Handles pool cleanup

3. **Batch Processing**
   - Creates transaction batches
   - Manages batch sizes
   - Controls batch distribution
   - Handles batch validation

### Integration Points
- Coordinates with Host actor
- Manages network communication
- Handles transaction selection
- Controls transaction flow

## MempoolLoad.rs

The `mempool_load.rs` file implements the transaction load generation system, which is used for testing and simulation purposes.

### Core Functionality
1. **Load Generation**
   - Generates test transactions
   - Controls transaction flow
   - Manages load patterns

2. **Load Types**
   - `UniformLoad`: Generates uniform transaction load
     - Controls transaction rate
     - Simulates steady-state conditions
   
   - `BurstLoad`: Generates burst transaction load
     - Creates load spikes
     - Tests system under stress
     - Simulates peak conditions

3. **Configuration**
   - Controls load parameters
   - Manages generation rates
   - Sets load patterns

### Integration with Mempool
- Injects test transactions
- Monitors pool behavior
- Tests system limits

## Mempool actor details (as it might be confusing)

1. **malachitebft-test-mempool (Dependency)**
   - Provides the core networking infrastructure:
     - P2P networking using libp2p
     - Gossip protocol implementation
     - Network message types and serialization
     - Peer discovery and management
     - Network event handling
   - Offers testing utilities:
     - Network simulation capabilities
     - Transaction batch handling
     - Protocol definitions
     - Network configuration options

2. **mempool.rs (Local Implementation)**
   - Implements the core mempool functionality:
     - Transaction pool management
     - Transaction validation and storage
     - Batch processing
     - Transaction selection for blocks
   - Handles business logic:
     - Transaction lifecycle management
     - Pool size limits
     - Transaction ordering
     - Batch creation and distribution

3. **mempool/network.rs (Local Implementation)**
   - Acts as a bridge between the local mempool and the network:
     - Wraps the test-mempool networking functionality
     - Manages network events
     - Handles message broadcasting
     - Controls peer connections
   - Provides actor-based interface:
     - Message handling
     - State management
     - Event subscription
     - Network control

The key differences are:
- `malachitebft-test-mempool` provides the low-level networking infrastructure and testing capabilities
- `mempool.rs` implements the business logic for transaction management
- `mempool/network.rs` creates an actor-based abstraction over the networking layer

# Dependencies

The actor system relies on several key dependencies that provide essential functionality:

1. **Core Consensus and Types**
   - `malachitebft-core-consensus`: Provides the core consensus protocol implementation
     - Used for peer identification and role management
     - Implements consensus message handling
     - Manages validator roles and voting
   - `malachitebft-core-types`: Contains fundamental data structures
     - Defines types for rounds, heights, and values
     - Implements commit certificates
     - Manages validity and value origins

2. **Engine and Network**
   - `malachitebft-engine`: Core engine implementation
     - Manages consensus message handling
     - Provides host functionality
     - Implements network message types
   - `malachitebft-network`: Network communication layer
     - Handles P2P networking
     - Manages peer connections
     - Implements message broadcasting

3. **Storage and State Management**
   - `malachitebft-wal`: Write-Ahead Log implementation
     - Persists consensus state
     - Handles crash recovery
   - `malachitebft-sync`: Block synchronization
     - Manages block synchronization
     - Runs concurrently with consensus
   - `malachitebft-app`: Application layer
     - Defines the Node trait
     - Implements the part store

4. **Configuration and Metrics**
   - `malachitebft-config`: Configuration management
     - Configuration for different actors
   - `malachitebft-metrics`: Metrics collection
     - Tracks performance metrics
     - Monitors system health
     - Shared registry for all actors

5. **Security and Signing**
   - `malachitebft-signing-ed25519`: Cryptographic operations
     - Handles transaction signing
     - Manages key pairs
     - Provides cryptographic primitives

6. **Testing and Development**
   - `malachitebft-test-cli`: Testing utilities
     - Provides CLI testing tools
     - Manages test scenarios
   - `malachitebft-test-mempool`: Mempool testing
     - Provides the core networking infrastructure:
     - Offers testing utilities:

7. **Protocol and Codec**
   - `malachitebft-proto`: Protocol buffer definitions
     - Defines message formats
     - Handles serialization
   - `malachitebft-codec`: Message encoding/decoding
     - Implements message codecs
     - Handles data serialization


