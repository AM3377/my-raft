Raft Consensus Algorithm: Educational Implementation
This project is a minimal Rust implementation of the Raft consensus algorithm, developed for the purpose of understanding the algorithm by closely following the Raft research paper ("In Search of an Understandable Consensus Algorithm" by Diego Ongaro and John Ousterhout). It is intended as a learning tool to explore the mechanics of Raft and is not designed for production use. The implementation focuses on clarity and adherence to the Raft algorithm, simulating a distributed consensus system with multiple nodes.
Overview
The Raft consensus algorithm ensures a cluster of nodes agrees on a shared state despite failures. This implementation was created to deepen understanding of Raft's leader election, log replication, and state machine application. It includes a Raft core, a gRPC-based network layer, and a simulation of multiple nodes running concurrently.
Key Features

Leader Election: Nodes transition between Follower, Candidate, and Leader roles with randomized election timeouts.
Log Replication: The leader replicates log entries to followers, ensuring consistency.
State Machine Application: Committed entries are logged to the console (simulating a state machine).
Network Communication: Uses gRPC for inter-node messaging.
Configurable Parameters: Supports customizable election timeouts, heartbeat intervals, and quorum checks.

Project Structure
The implementation consists of three main files:

main.rs: The entry point, setting up multiple Raft nodes in separate threads and simulating client proposals.
raft.rs: Contains the core Raft logic, including:
Raft and RaftCore structs for node state and behavior.
Log struct for managing log entries.
ProgressTracker for tracking votes and replication progress.
Message types for Raft communication (e.g., RequestVote, AppendEntries).


network.rs: Implements gRPC-based networking for inter-node communication, including:
Network struct for managing gRPC clients and servers.
RaftNodeService for handling incoming messages.
Conversion functions between Raft and gRPC message types.



Dependencies
The project uses the following Rust crates:

anyhow: For error handling.
tracing: For logging and debugging.
tokio: For asynchronous gRPC communication.
tonic: For gRPC implementation.
prost: For Protocol Buffers support.
rand: For randomized election timeouts.
std::sync::mpsc: For inter-thread communication.

Add these to your Cargo.toml:
[dependencies]
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
tokio = { version = "1.0", features = ["rt-multi-thread", "time"] }
tonic = "0.8"
prost = "0.11"
rand = "0.8"

The project uses a Protocol Buffers definition (raft.proto) included via tonic::include_proto!.
Building and Running
Prerequisites

Rust (stable, version 1.65 or later recommended).
Cargo for dependency management.
protoc (Protocol Buffers compiler) for gRPC code generation.

Steps to Run

Clone the Repository:
git clone <repository-url>
cd <repository-directory>


Build the Project:
cargo build

This compiles the protobuf definitions and dependencies.

Run the Simulation:
cargo run

This launches three Raft nodes on 127.0.0.1:9001, 127.0.0.1:9002, and 127.0.0.1:9003. The nodes will:

Elect a leader.
Propose commands every 10 ticks.
Log state and applied commands using tracing.



Configuration
Nodes are configured in main.rs with:

Node IDs: 1, 2, 3.
Addresses: 127.0.0.1:9001, 127.0.0.1:9002, 127.0.0.1:9003.
Election Timeout: 10 ticks (min: 5, max: 10).
Heartbeat Interval: 4 ticks.
Quorum Check: Enabled.
Tick Duration: 100ms.

Modify these in the Config struct in main.rs to experiment.
How It Works
Simulation Loop

Node Setup: Each node runs in a separate thread with its own Raft instance, network service, and communication channels.
Message Handling: Nodes use gRPC for communication, converting between Raft Message and gRPC RaftMessage types.
Proposals: Every 10 ticks, a command (e.g., command_1) is proposed to all nodes via proposal channels. Only the leader processes proposals.
Logging: Node state (role, term, log indices) and applied commands are logged via tracing.

Raft Algorithm

Leader Election: Followers become Candidates after an election timeout, sending RequestVote messages. A majority of votes elects a Leader.
Log Replication: The Leader appends entries and sends AppendEntries to followers, who verify and append them. The Leader commits entries when a majority replicate them.
State Machine: Committed entries are logged to the console.
Quorum Checks: Enabled via check_quorum, though simplified in this implementation.

Network Layer

gRPC: Each node runs a gRPC server and clients for peer communication, with retries for failed connections.
Message Conversion: Functions handle serialization between Raft and gRPC messages.

Example Output
Running cargo run produces logs like:
INFO  Node 1: became candidate at term 1
DEBUG Node 1: sending RequestVote from 1 to 2
INFO  Node 2: granting vote to 1 in term 1
INFO  Node 1: election won at term 1
INFO  Node 1: became leader at term 1
INFO  Node 1: Proposing entry at index 1, term 1
INFO  Node 2: APPLYING: Log Entry Term 1, Index 1, Data: "command_1"

This shows Node 1 becoming the leader, proposing a command, and Node 2 applying it.
Limitations
This implementation is a minimal, educational tool and not suitable for production due to:

No Persistence: Logs and state are not saved to disk.
Simplified State Machine: Only logs entries to the console.
No Snapshots: Log growth is not managed.
Basic Error Handling: Limited handling of network partitions or crashes.
Synchronous Simulation: Nodes use threads with synchronous Raft loops.

Purpose and Scope
This project was developed to gain a deeper understanding of the Raft algorithm by implementing its core components as described in the Raft paper. It prioritizes clarity and fidelity to the paper's description over performance or robustness, serving as a learning tool rather than a production-ready system.
Future Improvements

Add log and state persistence.
Implement log snapshots.
Enhance the state machine for real applications.
Simulate more complex failure scenarios.
Support dynamic cluster membership changes.

Contributing
Feedback is welcome! Submit issues or pull requests to the repository. Ensure changes align with the educational focus and maintain code clarity.
License
Licensed under the Apache License, Version 2.0. See the LICENSE file for details.
