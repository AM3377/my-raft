# Raft Consensus Algorithm: Educational Rust Implementation

---

This project offers a minimal Rust implementation of the Raft consensus algorithm. It's designed as an **educational tool** to facilitate a deep understanding of Raft by closely following the principles outlined in the original research paper, "In Search of an Understandable Consensus Algorithm."

**Purpose:** To provide a clear and concise simulation of a distributed consensus system, ideal for learning the core mechanics of Raft.
**Note:** This implementation is strictly for educational purposes and is **not suitable for production environments**.

## ✨ Features

* **Leader Election:** Nodes dynamically transition between Follower, Candidate, and Leader roles with randomized election timeouts.
* **Log Replication:** The leader efficiently replicates log entries to followers, ensuring strong consistency.
* **State Machine Application:** Committed log entries are logged to the console, simulating their application to a state machine.
* **gRPC Communication:** Utilizes gRPC for robust inter-node messaging.
* **Configurable Parameters:** Customizable election timeouts, heartbeat intervals, and quorum checks.

## 🚀 Getting Started

### Prerequisites

* **Rust:** Stable version 1.65 or later (recommended).
* **Cargo:** Rust's package manager (included with Rust installation).
* **`protoc`:** Protocol Buffers compiler (for gRPC code generation).

### Installation & Run

1.  **Clone the repository:**
    ```bash
    git clone git@github.com:AM3377/my-raft.git
    cd my-raft
    ```

2.  **Build the project:**
    ```bash
    cargo build
    ```
    This command compiles protobuf definitions and dependencies.

3.  **Run the simulation:**
    ```bash
    cargo run
    ```
    This launches three Raft nodes (`127.0.0.1:9001`, `127.0.0.1:9002`, `127.0.0.1:9003`). The simulation will elect a leader, propose commands, and log state changes and applied commands.

## ⚙️ Configuration

Default node parameters are set in `main.rs` but can be easily modified:

* **Node IDs:** `1, 2, 3`
* **Addresses:** `127.0.0.1:9001`, `127.0.0.1:9002`, `127.0.0.1:9003`
* **Election Timeout:** `10 ticks` (min: 5, max: 10)
* **Heartbeat Interval:** `4 ticks`
* **Quorum Check:** `Enabled`
* **Tick Duration:** `100ms`

## 💡 How It Works

The simulation operates with each Raft node running in a separate thread, managing its own Raft instance and network service. Nodes communicate via gRPC, converting between internal Raft message types and gRPC formats. Commands are proposed to the leader, replicated to followers, and applied to a simulated state machine (logged to console) upon commitment.

## ⚠️ Limitations (Educational Scope)

This implementation is simplified for clarity and learning, therefore it has the following limitations:

* **No Persistence:** Logs and state are not saved to disk.
* **Simplified State Machine:** Only logs entries to the console.
* **No Snapshots:** Log growth is not managed.
* **Basic Error Handling:** Limited handling of complex failure scenarios.
* **Synchronous Simulation:** Nodes use threads with synchronous Raft loops.

## 🤝 Contributing

Feedback and contributions are welcome! Please submit issues or pull requests to the repository. Ensure any changes align with the educational focus and maintain code clarity.

* [Submit an Issue](https://github.com/AM3377/my-raft/issues)
* [Open a Pull Request](https://github.com/AM3377/my-raft/pulls)

## 📄 License

This project is licensed under the Apache License, Version 2.0. See the `LICENSE` file for details.
