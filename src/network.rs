use crate::raft::{Message, MessageType};
use anyhow::{Context, Result};
use raft_proto::raft_service_server::{RaftService, RaftServiceServer};
use raft_proto::{LogEntry, RaftMessage};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tonic::{Request, Response, Status, transport::Server};
use tracing::{debug, error, info, warn};

pub mod raft_proto {
    tonic::include_proto!("raft");
}

pub struct RaftNodeService {
    node_id: u64,
    node_sender: Sender<Message>,
}

#[tonic::async_trait]
impl RaftService for RaftNodeService {
    async fn send_message(
        &self,
        request: Request<RaftMessage>,
    ) -> Result<Response<RaftMessage>, Status> {
        let msg = request.into_inner();
        debug!(
            "Node {}: Received gRPC message: type={}, from={}, to={}, prev_log_index={}, prev_log_term={}, entries_len={}, leader_commit={}",
            self.node_id,
            msg.msg_type,
            msg.from,
            msg.to,
            msg.prev_log_index,
            msg.prev_log_term,
            msg.entries.len(),
            msg.leader_commit
        );

        let raft_msg =
            convert_to_raft_message(&msg).map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.node_sender
            .send(raft_msg)
            .map_err(|e| Status::internal(format!("Failed to send to Raft: {}", e)))?;

        let response = RaftMessage {
            msg_type: "Ack".to_string(),
            term: 0,
            from: self.node_id,
            to: msg.from,
            last_log_index: 0,
            last_log_term: 0,
            granted: false,
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![],
            leader_commit: 0,
        };
        Ok(Response::new(response))
    }
}

pub struct Network {
    node_id: u64,
    peers: HashMap<u64, String>,
    node_sender: Sender<Message>,
}

impl Network {
    pub fn new(node_id: u64, peers: HashMap<u64, String>, node_sender: Sender<Message>) -> Self {
        Network {
            node_id,
            peers,
            node_sender,
        }
    }

    pub fn start(&self, address: &str, receiver: Receiver<Message>) -> Result<()> {
        let node_id = self.node_id;
        let addr = address
            .parse()
            .context(format!("Invalid address: {}", address))?;
        let service = RaftNodeService {
            node_id,
            node_sender: self.node_sender.clone(),
        };

        let server_future = async move {
            info!("Node {}: Starting gRPC server on {}", node_id, addr);
            Server::builder()
                .add_service(RaftServiceServer::new(service))
                .serve(addr)
                .await
                .map_err(|e| anyhow::anyhow!("gRPC server error: {}", e))?;
            Ok::<(), anyhow::Error>(())
        };

        let rt = tokio::runtime::Runtime::new()?;
        std::thread::spawn(move || {
            rt.block_on(server_future).unwrap();
        });

        let peers = self.peers.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut clients: HashMap<u64, Option<raft_proto::raft_service_client::RaftServiceClient<tonic::transport::Channel>>> = HashMap::new();

                for (peer_id, addr) in &peers {
                    match create_client_with_retries(*peer_id, addr).await {
                        Ok(client) => {
                            clients.insert(*peer_id, Some(client));
                            info!("Node {}: Created gRPC client for peer {}", node_id, peer_id);
                        }
                        Err(e) => {
                            error!("Node {}: Failed to create client for peer {}: {}", node_id, peer_id, e);
                            clients.insert(*peer_id, None);
                        }
                    }
                }

                while let Ok(msg) = receiver.recv() {
                    let peer_id = msg.to;
                    let client = clients.get_mut(&peer_id);

                    if let Some(Some(client)) = client {
                        let grpc_msg = convert_from_raft_message(&msg);
                        debug!(
                            "Node {}: Sending gRPC message to {}: type={}, from={}, to={}, prev_log_index={}, prev_log_term={}, entries_len={}, leader_commit={}",
                            node_id, peer_id, grpc_msg.msg_type, grpc_msg.from, grpc_msg.to, grpc_msg.prev_log_index, grpc_msg.prev_log_term, grpc_msg.entries.len(), grpc_msg.leader_commit
                        );
                        match client.send_message(grpc_msg).await {
                            Ok(_) => debug!("Node {}: Sent message to peer {}", node_id, peer_id),
                            Err(e) => {
                                error!("Node {}: Failed to send message to peer {}: {}", node_id, peer_id, e);
                            }
                        }
                    } else {
                        warn!("Node {}: No client for peer {}. Attempting to reconnect.", node_id, peer_id);
                        if let Some(addr) = peers.get(&peer_id) {
                            match create_client_with_retries(peer_id, addr).await {
                                Ok(new_client) => {
                                    clients.insert(peer_id, Some(new_client));
                                    info!("Node {}: Reconnected to peer {}", node_id, peer_id);
                                    let grpc_msg = convert_from_raft_message(&msg);
                                    if let Some(client) = clients.get_mut(&peer_id).unwrap() {
                                        match client.send_message(grpc_msg).await {
                                            Ok(_) => debug!("Node {}: Sent message to peer {} after reconnection", node_id, peer_id),
                                            Err(e) => error!("Node {}: Failed to send message to peer {} after reconnection: {}", node_id, peer_id, e),
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Node {}: Reconnection to peer {} failed: {}", node_id, peer_id, e);
                                }
                            }
                        } else {
                            error!("Node {}: No address for peer {}", node_id, peer_id);
                        }
                    }
                }
                Ok::<(), anyhow::Error>(())
            })
            .unwrap();
        });

        Ok(())
    }
}

async fn create_client_with_retries(
    peer_id: u64,
    addr: &str,
) -> Result<raft_proto::raft_service_client::RaftServiceClient<tonic::transport::Channel>, String> {
    let max_retries = 5;
    let initial_retry_delay = Duration::from_millis(100);
    let mut delay = initial_retry_delay;

    for attempt in 1..=max_retries {
        info!(
            "Attempt {} to connect to peer {} at {}",
            attempt, peer_id, addr
        );
        match timeout(
            Duration::from_secs(2),
            raft_proto::raft_service_client::RaftServiceClient::connect(format!("http://{}", addr)),
        )
        .await
        {
            Ok(Ok(client)) => {
                info!("Connected to peer {} on attempt {}", peer_id, attempt);
                return Ok(client);
            }
            Ok(Err(e)) => {
                warn!("Attempt {} failed for peer {}: {}", attempt, peer_id, e);
            }
            Err(_) => {
                warn!("Attempt {} timed out for peer {}", attempt, peer_id);
            }
        }
        sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(2));
    }
    Err(format!(
        "Failed to connect to peer {} after {} attempts",
        peer_id, max_retries
    ))
}

fn convert_to_raft_message(msg: &RaftMessage) -> Result<Message> {
    let msg_type = match msg.msg_type.as_str() {
        "RequestVote" => MessageType::RequestVote,
        "RequestVoteResponse" => MessageType::RequestVoteResponse,
        "AppendEntries" => MessageType::AppendEntries,
        "AppendEntriesResponse" => MessageType::AppendEntriesResponse,
        _ => return Err(anyhow::anyhow!("Unknown message type: {}", msg.msg_type)),
    };
    Ok(Message {
        msg_type,
        term: msg.term,
        from: msg.from,
        to: msg.to,
        last_log_index: msg.last_log_index,
        last_log_term: msg.last_log_term,
        granted: if msg.granted {
            Some(true)
        } else {
            msg.granted.then_some(false)
        },
        prev_log_index: msg.prev_log_index,
        prev_log_term: msg.prev_log_term,
        entries: msg
            .entries
            .iter()
            .map(|e| crate::raft::Entry {
                index: e.index,
                term: e.term,
                data: e.data.clone(),
            })
            .collect(),
        leader_commit: msg.leader_commit,
    })
}

fn convert_from_raft_message(msg: &Message) -> RaftMessage {
    let msg_type = match msg.msg_type {
        MessageType::RequestVote => "RequestVote".to_string(),
        MessageType::RequestVoteResponse => "RequestVoteResponse".to_string(),
        MessageType::AppendEntries => "AppendEntries".to_string(),
        MessageType::AppendEntriesResponse => "AppendEntriesResponse".to_string(),
    };
    RaftMessage {
        msg_type,
        term: msg.term,
        from: msg.from,
        to: msg.to,
        last_log_index: msg.last_log_index,
        last_log_term: msg.last_log_term,
        granted: msg.granted.unwrap_or(false),
        prev_log_index: msg.prev_log_index,
        prev_log_term: msg.prev_log_term,
        entries: msg
            .entries
            .iter()
            .map(|e| LogEntry {
                index: e.index,
                term: e.term,
                data: e.data.clone(),
            })
            .collect(),
        leader_commit: msg.leader_commit,
    }
}
