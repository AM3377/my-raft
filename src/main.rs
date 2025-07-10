use anyhow::Result;
use raft::{Config, Message, Raft};
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};
use tracing::{debug, error, info};

mod network;
mod raft;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let peer_ids = vec![1, 2, 3];
    let ports = vec!["127.0.0.1:9001", "127.0.0.1:9002", "127.0.0.1:9003"];
    let peers: HashMap<u64, String> = peer_ids
        .iter()
        .copied()
        .zip(ports.iter().map(|s| s.to_string()))
        .collect();

    let mut node_handles = Vec::new();
    let (_proposal_tx, _): (Sender<(u64, Vec<u8>)>, Receiver<(u64, Vec<u8>)>) = mpsc::channel();

    // Create per-node proposal channels
    let mut proposal_senders: HashMap<u64, Sender<Vec<u8>>> = HashMap::new();

    // Spawn a Raft node for each peer
    for (id, addr) in peer_ids.iter().copied().zip(ports.iter()) {
        let config = Config {
            id,
            election_tick: 10,
            heartbeat_tick: 4,
            check_quorum: true,
            min_election_tick: 5,
            max_election_tick: 10,
            peers: peer_ids.clone(),
        };

        let (msg_tx, msg_rx): (Sender<Message>, Receiver<Message>) = mpsc::channel();
        let (node_tx, node_rx): (Sender<Message>, Receiver<Message>) = mpsc::channel();
        let (prop_tx, prop_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
        proposal_senders.insert(id, prop_tx);

        let network = network::Network::new(id, peers.clone(), node_tx);
        let addr = addr.to_string();
        network.start(&addr, msg_rx)?;

        let handle = thread::spawn(move || {
            let mut node = Raft::new(config);
            let mut tick = 0;

            loop {
                tick += 1;
                debug!("Node {}: Tick {}", node.id, tick);

                while let Ok(msg) = node_rx.try_recv() {
                    if let Err(e) = node.step(msg) {
                        error!("Node {}: Error processing message: {}", node.id, e);
                    }
                }

                node.tick();

                let node_id = node.id;
                for msg in node.msgs.drain(..) {
                    if let Err(e) = msg_tx.send(msg) {
                        error!("Node {}: Failed to send message to network: {}", node_id, e);
                    }
                }

                if node.role == raft::Role::Leader {
                    while let Ok(data) = prop_rx.try_recv() {
                        if let Err(e) = node.propose(data) {
                            error!("Node {}: Failed to propose: {}", node_id, e);
                        }
                    }
                }

                info!(
                    "Node {}: Role: {:?}, Term: {}, Leader: {}, Elapsed: {}, Log: (LastIdx: {}, Term: {}, Comm: {}, App: {})",
                    node.id,
                    node.role,
                    node.term,
                    node.leader_id,
                    node.election_elapsed,
                    node.log.last_index(),
                    node.log.term(node.log.last_index()),
                    node.log.committed,
                    node.log.applied
                );
                if node.role == raft::Role::Leader {
                    debug!(
                        "Node {}: next_index: {:?}, match_index: {:?}",
                        node.id, node.prs.next_index, node.prs.match_index
                    );
                }

                thread::sleep(Duration::from_millis(100));
                if tick > 50 {
                    break;
                }
            }
        });

        node_handles.push(handle);
    }

    let mut tick = 0;
    let mut proposal_counter = 0;
    loop {
        tick += 1;
        if tick % 10 == 0 {
            proposal_counter += 1;
            let data = format!("command_{}", proposal_counter).as_bytes().to_vec();
            for id in peer_ids.iter().copied() {
                if let Some(prop_tx) = proposal_senders.get(&id) {
                    if let Err(e) = prop_tx.send(data.clone()) {
                        error!("Failed to send proposal to node {}: {}", id, e);
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(100));
        if tick > 50 {
            break;
        }
    }

    for handle in node_handles {
        handle.join().unwrap();
    }

    Ok(())
}
