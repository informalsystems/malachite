# Transaction Flow Diagram

## Actor Architecture

The application spawns the following actors with their respective ActorRef types and relationships:

```mermaid
graph TD
    subgraph "Application Layer"
        ConsensusHost[ConsensusHost]
        Host[Host]
    end
    
    subgraph "Consensus Layer"
        Consensus[ConsensusEngine]
        ConsensusNetwork[ConsensusNetwork]
        WAL[WAL]
        Sync[Sync]
    end
    
    subgraph "Mempool Layer"
        Mempool[Mempool]
        MempoolNetwork[MempoolNetwork]
        MempoolLoad[MempoolLoad]
    end
    
    Consensus -->|NetworkMsg| ConsensusNetwork
    Consensus -->|ConsensusHostMsg| ConsensusHost
    Sync -->|ConsensusHostMsg| ConsensusHost
    WAL -->|ConsensusMsg| Consensus
    
    ConsensusHost -->|HostMsg| Host
    Host -->|MempoolMsg| Mempool
    Host -->|NetworkMsg| ConsensusNetwork
    
    %% Subscriptions in pre_start()
    Host -.->|Subscribe| Mempool
    Mempool -.->|Subscribe| MempoolNetwork
    Consensus -.->|Subscribe| ConsensusNetwork
    
    MempoolLoad -->|MempoolMsg::Add| Mempool
    Mempool -->|NetworkMsg| MempoolNetwork
    
    classDef consensusActor fill:#e1f5fe
    classDef appActor fill:#f3e5f5
    classDef mempoolActor fill:#e8f5e8
    
    class Consensus,WAL,Sync,ConsensusNetwork consensusActor
    class Host,ConsensusHost appActor
    class Mempool,MempoolNetwork,MempoolLoad mempoolActor
```

### Actor Types:
- **Engine Actors** (blue): Core malachite engine components
- **App Actors** (purple): Custom application-specific actors
- **Network Actors** (green): Network and mempool infrastructure

### Key Relationships:
- `ConsensusHost` acts as a bridge between engine components and the custom `Host`
- `Host` handles both consensus messages and mempool events
- `MempoolLoad` generates transactions for testing/simulation
- All engine components (`Consensus`, `Sync`) communicate through `ConsensusHost`

## Transaction Flow Sequence

This diagram shows the flow for a valid transaction sent by `MempoolLoad`.
For transactions received via gossip layer, i.e. from `MempoolNetwork` the flow is the same, except there is no reply gossiped back to the network.

```mermaid
sequenceDiagram
    participant ML as MempoolLoad
    participant M as Mempool
    participant H as Host
    participant S as HostState
    
    Note over ML: Generate transaction
    ML->>+M: MempoolMsg::Add { tx, reply }
    M->>+H: MempoolEvent::CheckTx { tx, reply }
    Note over H: Route to mempool event handler
    H->>+S: check_tx(&tx)
    Note over S: Create Transaction from RawTx<br/>Compute hash<br/>Return CheckTxOutcome::Success(hash)
    S-->>-H: CheckTxOutcome::Success
    H->>M: MempoolMsg::CheckTxResult { tx, result, reply }
    Note over M: Add to mempool if valid<br/> reject if invalid<br/>or mempool is full
    M-->>-ML: Reply with outcome or error
    Note over ML: Continue or handle error
```

## Flow Description

1. **MempoolLoad** generates a transaction and sends it to the **Mempool** via `MempoolMsg::Add`
2. **Mempool** forwards the transaction directly to the **Host** via `MempoolEvent::CheckTx` 
3. **Host** routes the event to its mempool handler and calls **HostState**'s `check_tx` method
4. **HostState** creates a `Transaction` from the `RawTx`, computes its hash, and returns a `CheckTxOutcome::Success`
5. **Host** sends the result back to **Mempool** via `MempoolMsg::CheckTxResult`
6. **Mempool** adds the transaction to its pool if valid and space is available, rejects if invalid, or drops if the mempool is full, then replies to **MempoolLoad** with the outcome

## Transaction Removal Flow - Decided Block

This diagram shows how transactions are removed from the mempool when a block is decided by consensus.

```mermaid
sequenceDiagram
    participant C as ConsensusEngine
    participant CH as ConsensusHost
    participant H as Host
    participant M as Mempool
    
    Note over C: Block decided by consensus
    C->>+CH: ConsensusHostMsg::Decided { certificate }
    Note over CH: Relay to Host
    CH->>+H: HostMsg::Consensus(Decided)
    Note over H: Extract transactions from block<br/> compute hashes
    loop For each transaction hash
        H->>M: MempoolMsg::Remove { tx_hash }
        Note over M: Remove transaction<br/>from mempool
    end
    H-->>-CH: Processing complete
    CH-->>-C: Ack
    Note over C: Continue consensus
```

## Removal Flow Description

1. **ConsensusEngine** decides on a block and sends `ConsensusHostMsg::Decided` with the certificate to **ConsensusHost**
2. **ConsensusHost** relays this as `HostMsg::Consensus(Decided)` to the **Host**
3. **Host** extracts the transaction hashes from the decided block's certificate
4. For each transaction hash, **Host** sends `MempoolMsg::Remove { tx_hash }` to **Mempool**
5. **Mempool** removes the corresponding transactions from its pool
6. **Host** completes processing and acknowledges back through **ConsensusHost**

## Key Points

- The `Host` actor serves as a bridge between the mempool and application state validation logic
- The `CheckTxOutcome` contains the transaction hash that will be used later for removal from the mempool
- Network transactions are processed in batches but validated individually through the same `CheckTx` flow
- Transactions can be dropped if the mempool is full, regardless of their origin (RPC or network gossip)
- Hash consistency between addition (`check_tx`) and removal (`Decided`) is critical for proper mempool management
- The same transaction hash computed during validation is used for removal when blocks are decided
