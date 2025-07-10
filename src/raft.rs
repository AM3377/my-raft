use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};

use anyhow::{Result, anyhow};
use tracing::{debug, info, warn};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Role {
    Follower,
    Candidate,
    Leader,
}

/// A constant represents invalid id of raft.
pub const INVALID_ID: u64 = 0;
pub const NO_ENTRY: u64 = 0; // Represents an empty or non-existent log entry index/term

// --- Log Entry and Log Storage ---

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Entry {
    pub index: u64,
    pub term: u64,
    pub data: Vec<u8>, // The actual command/data
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Log {
    // Entries are 1-indexed for Raft, but stored in a 0-indexed Vec.
    // Index 0 in Vec corresponds to Raft log index 1.
    // The dummy entry at index 0 (Raft index 0, term 0) is implicitly handled by `first_index`.
    pub entries: Vec<Entry>,
    pub committed: u64, // The highest log entry known to be committed
    pub applied: u64,   // The highest log entry applied to the state machine

                        // For simplicity, we'll assume log starts from index 1.
                        // In a real system, you'd handle snapshots and different offsets.
                        // For now, `first_index()` will always be 1.
}

impl Log {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(), // Starts empty
            committed: 0,
            applied: 0,
        }
    }

    /// Returns the Raft log index of the first entry.
    /// For this simplified model, it's always 1.
    pub fn first_index(&self) -> u64 {
        1 // Assuming log always starts from index 1 for simplicity
    }

    /// Returns the Raft log index of the last entry.
    pub fn last_index(&self) -> u64 {
        if self.entries.is_empty() {
            self.first_index() - 1 // If empty, last index is before first (0)
        } else {
            self.entries.last().unwrap().index
        }
    }

    /// Returns the term of the entry at `index`.
    /// Returns 0 (NO_ENTRY) if index is out of bounds or before first_index.
    pub fn term(&self, index: u64) -> u64 {
        if index < self.first_index() || index > self.last_index() {
            return NO_ENTRY;
        }
        self.entries[(index - self.first_index()) as usize].term
    }

    /// Appends a new entry to the log. Updates the index.
    pub fn append(&mut self, term: u64, data: Vec<u8>) {
        let new_index = self.last_index() + 1;
        self.entries.push(Entry {
            index: new_index,
            term,
            data,
        });
    }

    /// Appends a slice of entries. This assumes they are correct and contiguous.
    /// This is used by followers to replicate leader's entries.
    pub fn append_entries(&mut self, new_entries: &[Entry]) {
        if new_entries.is_empty() {
            return;
        }

        // Find the index where new entries might overlap or start
        let start_new_entries_idx = new_entries[0].index;

        // Truncate any conflicting entries
        if start_new_entries_idx <= self.last_index() {
            let truncate_from_idx = (start_new_entries_idx - self.first_index()) as usize;
            self.entries.truncate(truncate_from_idx);
        }

        // Append new entries
        for entry in new_entries {
            // Ensure appended entries maintain contiguity and increasing index
            if entry.index == self.last_index() + 1 {
                self.entries.push(entry.clone());
            } else {
                // This shouldn't happen if `truncate` logic is correct and `new_entries` are sequential
                warn!(
                    "Attempted to append non-contiguous entry at index {} after last index {}",
                    entry.index,
                    self.last_index()
                );
                // Handle this error or panic in a real system
                break;
            }
        }
    }

    /// Returns entries from `from_index` up to the end of the log.
    pub fn entries_from(&self, from_index: u64) -> &[Entry] {
        if from_index > self.last_index() + 1 {
            // If requesting beyond end or past last+1
            return &[];
        }
        let start_vec_idx = (from_index - self.first_index()) as usize;
        &self.entries[start_vec_idx..]
    }

    /// Applies committed entries to the state machine.
    /// In this simulation, it just prints them.
    pub fn apply_committed(&mut self) {
        while self.applied < self.committed {
            self.applied += 1;
            let entry_to_apply = &self.entries[(self.applied - self.first_index()) as usize];
            info!(
                "APPLYING: Log Entry Term {}, Index {}, Data: {:?}",
                entry_to_apply.term,
                entry_to_apply.index,
                String::from_utf8_lossy(&entry_to_apply.data)
            );
        }
    }
}

// --- Progress Tracker for Leader ---

pub struct ProgressTracker {
    pub voters: HashSet<u64>,      // IDs of voting members
    pub votes: HashMap<u64, bool>, // Map: peer ID → voted granted?

    // For leader only:
    // next_index[peer_id]: the next log index to send to that server.
    // match_index[peer_id]: the highest log index known to be replicated on that server.
    pub next_index: HashMap<u64, u64>,
    pub match_index: HashMap<u64, u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VoteResult {
    Won,
    Lost,
    Pending,
}

impl ProgressTracker {
    pub fn new(voters: impl IntoIterator<Item = u64>) -> Self {
        let voters: HashSet<u64> = voters.into_iter().collect();
        let mut next_index = HashMap::new();
        let mut match_index = HashMap::new();

        for &id in &voters {
            next_index.insert(id, NO_ENTRY); // Initialized later based on leader's log
            match_index.insert(id, NO_ENTRY);
        }

        Self {
            voters,
            votes: HashMap::new(),
            next_index,
            match_index,
        }
    }

    pub fn record_vote(&mut self, from: u64, granted: bool) {
        if self.voters.contains(&from) {
            self.votes.insert(from, granted);
        } else {
            debug!("Ignored vote from non-voter: {}", from);
        }
    }

    pub fn reset_votes(&mut self) {
        self.votes.clear();
    }

    /// TallyVotes returns the number of granted and rejected Votes, and whether the
    /// election outcome is known.
    pub fn tally_votes(&self) -> (usize, usize, VoteResult) {
        let mut granted = 0;
        let mut rejected = 0;

        // Count votes only from actual voters
        for voter_id in &self.voters {
            if let Some(&vote_granted) = self.votes.get(voter_id) {
                if vote_granted {
                    granted += 1;
                } else {
                    rejected += 1;
                }
            }
        }

        let result = self.vote_result();
        (granted, rejected, result)
    }

    /// Returns the Candidate's eligibility in the current election.
    pub fn vote_result(&self) -> VoteResult {
        let mut granted_count = 0;
        let mut rejected_count = 0;

        for voter_id in &self.voters {
            if let Some(&voted_granted) = self.votes.get(voter_id) {
                if voted_granted {
                    granted_count += 1;
                } else {
                    rejected_count += 1;
                }
            }
        }

        let total_voters = self.voters.len();
        // A majority is more than half. For N members, majority is floor(N/2) + 1.
        // E.g., 3 members: floor(3/2) + 1 = 1 + 1 = 2.
        // E.g., 5 members: floor(5/2) + 1 = 2 + 1 = 3.
        let majority = total_voters / 2 + 1;

        if granted_count >= majority {
            VoteResult::Won
        } else if rejected_count >= majority {
            VoteResult::Lost
        } else {
            VoteResult::Pending
        }
    }

    /// Initialize next_index and match_index for all peers.
    /// `last_log_index` is the leader's last log index.
    pub fn init_leader_progress(&mut self, leader_id: u64, last_log_index: u64) {
        for &peer_id in &self.voters {
            if peer_id == leader_id {
                self.next_index.insert(peer_id, last_log_index + 1);
                self.match_index.insert(peer_id, last_log_index);
            } else {
                // For other followers, next_index is leader's last_log_index + 1
                self.next_index.insert(peer_id, last_log_index + 1);
                self.match_index.insert(peer_id, NO_ENTRY); // Initially 0
            }
        }
        debug!(
            "Leader progress initialized: next_index: {:?}, match_index: {:?}",
            self.next_index, self.match_index
        );
    }
}

// --- Raft Core ---

pub struct Config {
    pub id: u64,                  // Node ID
    pub election_tick: usize,     // Election timeout (in ticks)
    pub heartbeat_tick: usize,    // Heartbeat interval (must be < election_tick)
    pub check_quorum: bool,       // Step down if quorum is not reachable
    pub min_election_tick: usize, // Randomized timeout range (start)
    pub max_election_tick: usize, // Randomized timeout range (end)
    pub peers: Vec<u64>,          // List of peer IDs
}

pub struct RaftCore {
    /// The current election term.
    pub term: u64,

    /// Which peer this raft is voting for.
    pub vote: u64, // The ID of the peer that this node voted for in the current term

    /// The ID of this node.
    pub id: u64,

    /// The current role of this node.
    pub role: Role,

    /// The leader id.
    pub leader_id: u64,

    /// Ticks since it reached last electionTimeout when it is leader or candidate.
    /// Number of ticks since it reached last electionTimeout or received a
    /// valid message from current leader when it is a follower.
    pub election_elapsed: usize,

    /// Number of ticks since it reached last heartbeatTimeout.
    /// only leader keeps heartbeatElapsed.
    heartbeat_elapsed: usize,

    /// Whether to check the quorum
    pub check_quorum: bool,

    heartbeat_timeout: usize,
    election_timeout: usize, // This is the base election timeout, not the randomized one

    // randomized_election_timeout is a random number between
    // [min_election_timeout, max_election_timeout - 1]. It gets reset
    // when raft changes its state to follower or candidate.
    randomized_election_timeout: usize,
    min_election_timeout: usize,
    max_election_timeout: usize,
    /// The logger for the raft structure.
    pub log: Log, // The Raft log
}

pub struct Raft {
    pub prs: ProgressTracker,
    /// The list of messages to be sent out.
    pub msgs: Vec<Message>,
    pub r: RaftCore,
}

impl Deref for Raft {
    type Target = RaftCore;

    #[inline]
    fn deref(&self) -> &RaftCore {
        &self.r
    }
}

impl DerefMut for Raft {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.r
    }
}

impl Raft {
    pub fn new(config: Config) -> Raft {
        let mut raft = Raft {
            prs: ProgressTracker::new(config.peers.iter().copied()),
            msgs: Vec::new(),
            r: RaftCore {
                term: 0,
                vote: INVALID_ID,
                id: config.id,
                role: Role::Follower,
                leader_id: INVALID_ID,
                election_elapsed: 0,
                heartbeat_elapsed: 0,
                check_quorum: config.check_quorum,
                heartbeat_timeout: config.heartbeat_tick,
                election_timeout: config.election_tick,
                randomized_election_timeout: 0,
                min_election_timeout: config.min_election_tick,
                max_election_timeout: config.max_election_tick,
                log: Log::new(),
            },
        };
        raft.reset_randomized_election_timeout();
        raft
    }

    /// Returns true to indicate that there will probably be some readiness need to be handled.
    pub fn tick(&mut self) -> bool {
        match self.role {
            Role::Follower | Role::Candidate => self.tick_election(),
            Role::Leader => self.tick_heartbeat(),
        }
    }

    /// Run by followers and candidates after self.election_timeout.
    ///
    /// Returns true to indicate that there will probably be some readiness need to be handled.
    pub fn tick_election(&mut self) -> bool {
        self.election_elapsed += 1;
        if !self.pass_election_timeout() {
            return false;
        }

        self.election_elapsed = 0;
        if self.role == Role::Follower || self.role == Role::Candidate {
            self.hup();
            true
        } else {
            false
        }
    }

    // tick_heartbeat is run by leaders to send a MsgBeat after self.heartbeat_timeout.
    // Returns true to indicate that there will probably be some readiness need to be handled.
    fn tick_heartbeat(&mut self) -> bool {
        self.heartbeat_elapsed += 1;
        self.election_elapsed += 1; // Leaders also track election elapsed for quorum checks

        let mut has_ready = false;

        if self.election_elapsed >= self.election_timeout {
            self.election_elapsed = 0;
            // In a more complete Raft, a leader might step down here if it loses quorum.
            // For now, we just reset the timer.
        }

        if self.heartbeat_elapsed >= self.heartbeat_timeout {
            self.heartbeat_elapsed = 0;
            has_ready = true;
            // Send heartbeat (AppendEntries with no entries)
            self.bcast_append_entries();
        }
        has_ready
    }

    /// Resets the current node to a given term.
    pub fn reset(&mut self, term: u64) {
        if self.term != term {
            self.term = term;
            self.vote = INVALID_ID;
        }
        self.leader_id = INVALID_ID;
        self.reset_randomized_election_timeout();
        self.election_elapsed = 0;
        self.heartbeat_elapsed = 0;

        self.prs.reset_votes();
    }

    /// Converts this node to a follower.
    pub fn become_follower(&mut self, term: u64, leader_id: u64) {
        self.reset(term);
        self.leader_id = leader_id;
        let from_role = self.role;
        self.role = Role::Follower;
        info!(
            "Node {}: became follower at term {} from_role {:?} with leader {}",
            self.id, self.term, from_role, self.leader_id
        );
    }

    /// Panics if a leader already exists.
    pub fn become_candidate(&mut self) {
        assert_ne!(
            self.role,
            Role::Leader,
            "Node {}: invalid transition [leader -> candidate]",
            self.id
        );
        let term = self.term + 1;
        self.reset(term);
        let id = self.id;
        self.vote = id; // Vote for self
        self.role = Role::Candidate;
        info!("Node {}: became candidate at term {}", self.id, self.term);
    }

    /// Panics if this is a follower node.
    pub fn become_leader(&mut self) {
        debug!("Node {}: ENTER become_leader", self.id);
        assert_ne!(
            self.role,
            Role::Follower,
            "Node {}: invalid transition [follower -> leader]",
            self.id
        );
        let term = self.term;
        self.reset(term);
        self.leader_id = self.id;
        self.role = Role::Leader;

        // Initialize next_index and match_index for all peers.
        // Raft requires next_index for all peers to be initialized to leader's last_log_index + 1
        let last_log_index = self.log.last_index();
        self.prs.init_leader_progress(self.id, last_log_index);

        info!("Node {}: became leader at term {}", self.id, self.term);
        debug!("Node {}: EXIT become_leader", self.id);

        // Leader immediately sends heartbeats upon becoming leader (which can include entries)
        self.bcast_append_entries();
    }

    pub fn campaign(&mut self) {
        self.become_candidate();
        let term = self.term;
        let self_id = self.id;

        // The candidate votes for itself immediately
        self.prs.record_vote(self_id, true);

        // If this is a single-node cluster, we win immediately.
        if self.prs.voters.len() == 1 {
            info!(
                "Node {}: Single-node cluster, became leader at term {}",
                self.id, self.term
            );
            self.become_leader();
            return;
        }

        // Send RequestVote messages to other peers.
        let last_log_index = self.log.last_index();
        let last_log_term = self.log.term(last_log_index);

        for id in self.prs.voters.iter().copied() {
            if id == self_id {
                continue;
            }
            let m = Message::request_vote(term, self_id, id, last_log_index, last_log_term);
            self.r.send(m, &mut self.msgs);
        }
    }

    /// Processes a vote from a peer and checks election outcome.
    fn poll(&mut self, from: u64, msg_type: MessageType, vote_granted: bool) -> VoteResult {
        self.prs.record_vote(from, vote_granted);
        let (_gr, _rj, res) = self.prs.tally_votes();

        if from != self.id {
            info!(
                "Node {}: received {:?} from {}: granted: {}, rejected: {} (Current votes: {:?})",
                self.id, msg_type, from, _gr, _rj, self.prs.votes
            );
        }

        match res {
            VoteResult::Won => {
                info!("Node {}: election won at term {}", self.id, self.term);
                self.become_leader();
            }
            VoteResult::Lost => {
                info!("Node {}: election lost at term {}", self.id, self.term);
                self.become_follower(self.term, INVALID_ID); // Step down, no known leader for now
            }
            VoteResult::Pending => {
                info!(
                    "Node {}: election still pending at term {}",
                    self.id, self.term
                );
            }
        }
        res
    }

    /// Sends AppendEntries RPCs to all other peers.
    /// This is used for heartbeats and log replication.
    fn bcast_append_entries(&mut self) {
        let leader_id = self.id;
        let current_term = self.term;
        let commit_index = self.log.committed;

        for peer_id in self.prs.voters.iter().copied() {
            if peer_id == leader_id {
                continue;
            }

            let next_idx = *self.prs.next_index.get(&peer_id).unwrap_or(&1); // Default to 1 if not initialized

            let prev_log_index = if next_idx > self.log.first_index() {
                next_idx - 1
            } else {
                NO_ENTRY // Effectively 0 for the first entry
            };
            let prev_log_term = self.log.term(prev_log_index);

            let entries_to_send = self.log.entries_from(next_idx).to_vec(); // Clone entries for message

            let m = Message::append_entries(
                current_term,
                leader_id,
                peer_id,
                prev_log_index,
                prev_log_term,
                entries_to_send,
                commit_index,
            );
            self.r.send(m, &mut self.msgs);
        }
    }

    /// Attempts to append a command to the leader's log.
    pub fn propose(&mut self, data: Vec<u8>) -> Result<()> {
        if self.role != Role::Leader {
            return Err(anyhow!("Node {} is not a leader, cannot propose", self.id));
        }

        let new_entry_index = self.log.last_index() + 1;
        info!(
            "Node {}: Proposing entry at index {}, term {}",
            self.id, new_entry_index, self.term
        );

        let self_id = self.id;
        let term = self.term;
        self.log.append(term, data);

        // After appending, update leader's own match_index and next_index
        // Use `get_mut` with the copied `self_id`
        *self.prs.match_index.get_mut(&self_id).unwrap() = new_entry_index;
        *self.prs.next_index.get_mut(&self_id).unwrap() = new_entry_index + 1;

        self.bcast_append_entries(); // Immediately send out new entries
        Ok(())
    }

    /// Tries to advance the commit index.
    /// This should be called by the leader after receiving AppendEntriesResponses.
    fn maybe_commit(&mut self) {
        if self.role != Role::Leader {
            return;
        }

        let mut sorted_match_indices: Vec<u64> = self.prs.match_index.values().copied().collect();
        sorted_match_indices.sort_unstable(); // Sort to find the median

        // The N-th largest match_index (where N is floor(cluster_size / 2) + 1)
        // For a 3-node cluster, 3/2 + 1 = 2. So the 2nd largest match_index.
        // For a 5-node cluster, 5/2 + 1 = 3. So the 3rd largest match_index.
        let majority_index = self.prs.voters.len() / 2; // 0-indexed for vec, gives the median if sorted
        let n_majority_match_index = sorted_match_indices[majority_index];

        // Raft Rule for Leader: Commit if N_majority_match_index is greater than current commit_index
        // AND the entry at N_majority_match_index in the leader's log was created in the current term.
        if n_majority_match_index > self.log.committed
            && self.log.term(n_majority_match_index) == self.term
        {
            info!(
                "Node {}: Leader committing log up to index {}",
                self.id, n_majority_match_index
            );
            self.log.committed = n_majority_match_index;
            self.log.apply_committed();
        }
    }

    /// Steps the raft along via a message. This should be called everytime your raft receives a
    /// message from a peer.
    pub fn step(&mut self, m: Message) -> Result<()> {
        // Handle the message term first.
        if m.term > self.term {
            info!(
                "Node {}: received message with higher term from {} (local term: {}, msg term: {}), stepping down.",
                self.id, m.from, self.term, m.term
            );
            let new_leader_id = if m.get_msg_type() == MessageType::AppendEntries {
                m.from
            } else {
                INVALID_ID
            };
            self.become_follower(m.term, new_leader_id);
        } else if m.term < self.term && m.term != 0 {
            // Ignore local messages with term 0
            info!(
                "Node {}: ignoring message with lower term from {} (local term: {}, msg term: {}) msg type {:?}",
                self.id,
                m.from,
                self.term,
                m.term,
                m.get_msg_type()
            );
            // Special handling for AppendEntries from old leader to force it to step down.
            if self.check_quorum && m.get_msg_type() == MessageType::AppendEntries {
                let to_send = Message::append_entries_response(
                    self.term, // Send current term back
                    self.id,
                    m.from,
                    false,                 // Indicate failure (log consistency or old term)
                    self.log.last_index(), // For followers, this is the last index they have
                );
                self.r.send(to_send, &mut self.msgs);
            }
            return Ok(());
        }

        // Now handle messages based on current role and message type (terms are equal or m.term is 0)
        match m.get_msg_type() {
            MessageType::RequestVote => {
                // Raft Rule: If RPC request or response contains term T > currentTerm:
                // set currentTerm = T, convert to follower. (Handled above)
                // If terms are equal, we only grant vote if:
                // 1. We haven't voted in this term, or we already voted for this candidate.
                // 2. The candidate's log is at least as up-to-date as ours.
                let mut vote_granted = false;
                if self.vote == INVALID_ID || self.vote == m.from {
                    let last_log_index = self.log.last_index();
                    let last_log_term = self.log.term(last_log_index);

                    // Log comparison rule:
                    // candidateLastTerm > myLastTerm, OR
                    // candidateLastTerm == myLastTerm AND candidateLastIndex >= myLastIndex
                    let candidate_is_up_to_date = m.last_log_term > last_log_term
                        || (m.last_log_term == last_log_term && m.last_log_index >= last_log_index);

                    if candidate_is_up_to_date {
                        vote_granted = true;
                        info!(
                            "Node {}: granting vote to {} in term {} (candidate log: T{}, I{}; my log: T{}, I{})",
                            self.id,
                            m.from,
                            self.term,
                            m.last_log_term,
                            m.last_log_index,
                            last_log_term,
                            last_log_index
                        );
                        self.election_elapsed = 0; // Reset election timeout on granting vote
                        self.vote = m.from; // Record the vote
                    } else {
                        info!(
                            "Node {}: rejecting vote to {} in term {} (candidate log outdated: T{}, I{}; my log: T{}, I{})",
                            self.id,
                            m.from,
                            self.term,
                            m.last_log_term,
                            m.last_log_index,
                            last_log_term,
                            last_log_index
                        );
                    }
                } else {
                    info!(
                        "Node {}: rejecting vote to {} in term {} (already voted for {})",
                        self.id, m.from, self.term, self.vote
                    );
                }

                let to_send =
                    Message::request_vote_response(self.term, self.id, m.from, vote_granted);
                self.r.send(to_send, &mut self.msgs);
            }
            MessageType::RequestVoteResponse => {
                if self.role == Role::Candidate {
                    let granted = m.granted.unwrap_or(false);
                    info!(
                        "Node {}: received VoteResponse from {} (granted: {})",
                        self.id, m.from, granted
                    );
                    self.poll(m.from, m.msg_type, granted);
                } else {
                    debug!(
                        "Node {}: (Role: {:?}) ignoring VoteResponse from {}",
                        self.id, self.role, m.from
                    );
                }
            }
            MessageType::AppendEntries => {
                if self.role == Role::Follower {
                    info!(
                        "Node {}: (Follower) received AppendEntries from Leader {}",
                        self.id, m.from
                    );
                    self.election_elapsed = 0; // Reset election timeout
                    self.leader_id = m.from; // Update leader

                    let mut success = false;
                    let mut current_last_index = self.log.last_index();

                    // Raft Rule for AppendEntries:
                    // 1. Reply false if term < currentTerm (handled above)
                    // 2. Reply false if log doesn’t contain an entry at prevLogIndex
                    //    whose term matches prevLogTerm.
                    let prev_log_term_matches = self.log.term(m.prev_log_index) == m.prev_log_term;
                    if m.prev_log_index > current_last_index || !prev_log_term_matches {
                        if m.prev_log_index > current_last_index {
                            debug!(
                                "Node {}: AppendEntries from {} failed: prev_log_index {} too high (my last_index {})",
                                self.id, m.from, m.prev_log_index, current_last_index
                            );
                        } else {
                            debug!(
                                "Node {}: AppendEntries from {} failed: term mismatch at prev_log_index {} (leader T{}, my T{})",
                                self.id,
                                m.from,
                                m.prev_log_index,
                                m.prev_log_term,
                                self.log.term(m.prev_log_index)
                            );
                        }
                    } else {
                        // 3. If an existing entry conflicts with a new one (same index but different terms),
                        //    delete the existing entry and all that follow it.
                        // 4. Append any new entries not already in the log.
                        self.log.append_entries(&m.entries);
                        success = true;
                        current_last_index = self.log.last_index(); // Update after appending

                        // 5. If leaderCommit > commitIndex, set commitIndex =
                        //    min(leaderCommit, index of last new entry).
                        if m.leader_commit > self.log.committed {
                            self.log.committed = std::cmp::min(m.leader_commit, current_last_index);
                            self.log.apply_committed();
                        }
                    }

                    let to_send = Message::append_entries_response(
                        self.term,
                        self.id,
                        m.from,
                        success,
                        current_last_index, // Always send back the last index we have
                    );
                    self.r.send(to_send, &mut self.msgs);
                } else {
                    // Leader or Candidate receives AppendEntries from a potential new leader
                    if m.term == self.term && self.role != Role::Follower {
                        warn!(
                            "Node {}: (Role: {:?}) received AppendEntries from {} in same term {}. Stepping down.",
                            self.id, self.role, m.from, m.term
                        );
                        self.become_follower(m.term, m.from);
                        // After stepping down, act as a follower and send response.
                        let mut success = false;
                        let mut current_last_index = self.log.last_index();
                        let prev_log_term_matches =
                            self.log.term(m.prev_log_index) == m.prev_log_term;

                        if m.prev_log_index <= current_last_index && prev_log_term_matches {
                            self.log.append_entries(&m.entries);
                            success = true;
                            current_last_index = self.log.last_index();
                            if m.leader_commit > self.log.committed {
                                self.log.committed =
                                    std::cmp::min(m.leader_commit, current_last_index);
                                self.log.apply_committed();
                            }
                        }
                        let to_send = Message::append_entries_response(
                            self.term,
                            self.id,
                            m.from,
                            success,
                            current_last_index,
                        );
                        self.r.send(to_send, &mut self.msgs);
                    } else {
                        debug!(
                            "Node {}: (Role: {:?}) ignoring AppendEntries from {} in term {}",
                            self.id, self.role, m.from, m.term
                        );
                    }
                }
            }
            MessageType::AppendEntriesResponse => {
                if self.role == Role::Leader {
                    let success = m.granted.unwrap_or(false);
                    let peer_id = m.from;
                    let follower_last_index = m.last_log_index; // This is the last index the follower *has*

                    info!(
                        "Node {}: (Leader) received AppendEntriesResponse from {} (success: {}, follower_last_index: {})",
                        self.id, peer_id, success, follower_last_index
                    );

                    if success {
                        // If AppendEntries was successful, update next_index and match_index for that follower
                        *self.prs.match_index.get_mut(&peer_id).unwrap() = follower_last_index;
                        *self.prs.next_index.get_mut(&peer_id).unwrap() = follower_last_index + 1;
                        self.maybe_commit(); // Try to commit new entries
                    } else {
                        // If AppendEntries failed, decrement next_index and retry (will be sent on next tick)
                        // This handles log inconsistency.
                        let current_next_index =
                            self.prs.next_index.get(&peer_id).copied().unwrap_or(1);
                        if current_next_index > 1 {
                            let new_next_index = current_next_index - 1;
                            *self.prs.next_index.get_mut(&peer_id).unwrap() = new_next_index;
                            warn!(
                                "Node {}: AppendEntries to {} failed, decrementing next_index to {}",
                                self.id, peer_id, new_next_index
                            );
                            // Immediately send another AppendEntries with the decremented next_index
                            // In a real impl, you might do this more smartly (e.g., specific RPC for this peer)
                            // but for this simple simulation, we'll just let the next tick handle it,
                            // or manually re-send to that specific peer.
                            // For now, let's trigger a single message send directly for that peer.
                            let prev_log_index = if new_next_index > self.log.first_index() {
                                new_next_index - 1
                            } else {
                                NO_ENTRY
                            };
                            let prev_log_term = self.log.term(prev_log_index);
                            let entries_to_send = self.log.entries_from(new_next_index).to_vec();

                            let m = Message::append_entries(
                                self.term,
                                self.id,
                                peer_id,
                                prev_log_index,
                                prev_log_term,
                                entries_to_send,
                                self.log.committed,
                            );
                            self.r.send(m, &mut self.msgs);
                        } else {
                            warn!(
                                "Node {}: AppendEntries to {} failed, next_index already at 1. Cannot decrement further.",
                                self.id, peer_id
                            );
                        }
                    }
                } else {
                    debug!(
                        "Node {}: (Role: {:?}) ignoring AppendEntriesResponse from {}",
                        self.id, self.role, m.from
                    );
                }
            }
        }
        Ok(())
    }

    fn hup(&mut self) {
        if self.role == Role::Leader {
            debug!("Node {}: ignoring MsgHup because already leader", self.id);
            return;
        }

        info!(
            "Node {}: starting a new election at term {}",
            self.id, self.term
        );
        self.campaign();
    }

    /// `pass_election_timeout` returns true iff `election_elapsed` is greater
    /// than or equal to the randomized election timeout.
    pub fn pass_election_timeout(&self) -> bool {
        self.election_elapsed >= self.randomized_election_timeout
    }

    /// Regenerates and stores the election timeout.
    pub fn reset_randomized_election_timeout(&mut self) {
        let prev_timeout = self.randomized_election_timeout;
        let timeout = rand::random_range(self.min_election_timeout..self.max_election_timeout);
        debug!(
            "Node {}: reset election timeout {} -> {} (election_elapsed was {})",
            self.id, prev_timeout, timeout, self.election_elapsed
        );
        self.randomized_election_timeout = timeout;
    }
}

impl RaftCore {
    fn send(&mut self, mut m: Message, msgs: &mut Vec<Message>) {
        if m.from == INVALID_ID {
            m.from = self.id;
        }
        debug!(
            "Node {}: sending {:?} from {} to {}",
            self.id, m.msg_type, m.from, m.to
        );
        msgs.push(m);
    }
}

// --- Message Types ---

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub enum MessageType {
    RequestVote,
    RequestVoteResponse,
    AppendEntries,
    AppendEntriesResponse,
}

#[derive(PartialEq, Clone, Debug)]
pub struct Message {
    pub msg_type: MessageType,
    pub term: u64,
    pub from: u64,
    pub to: u64,

    // Fields for RequestVote / RequestVoteResponse
    pub last_log_index: u64,
    pub last_log_term: u64,
    pub granted: Option<bool>, // Used for vote and append entry responses

    // Fields for AppendEntries / AppendEntriesResponse
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<Entry>,
    pub leader_commit: u64,
}

impl Message {
    // Constructor for RequestVote message
    pub fn request_vote(
        term: u64,
        from: u64,
        to: u64,
        last_log_index: u64,
        last_log_term: u64,
    ) -> Self {
        Self {
            msg_type: MessageType::RequestVote,
            term,
            from,
            to,
            last_log_index,
            last_log_term,
            granted: None,
            prev_log_index: NO_ENTRY,
            prev_log_term: NO_ENTRY,
            entries: Vec::new(),
            leader_commit: NO_ENTRY,
        }
    }

    // Constructor for RequestVoteResponse message
    pub fn request_vote_response(term: u64, from: u64, to: u64, granted: bool) -> Self {
        Self {
            msg_type: MessageType::RequestVoteResponse,
            term,
            from,
            to,
            last_log_index: NO_ENTRY,
            last_log_term: NO_ENTRY,
            granted: Some(granted),
            prev_log_index: NO_ENTRY,
            prev_log_term: NO_ENTRY,
            entries: Vec::new(),
            leader_commit: NO_ENTRY,
        }
    }

    // Constructor for AppendEntries message
    pub fn append_entries(
        term: u64,
        from: u64,
        to: u64,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<Entry>,
        leader_commit: u64,
    ) -> Self {
        Self {
            msg_type: MessageType::AppendEntries,
            term,
            from,
            to,
            last_log_index: NO_ENTRY, // Not used for AppendEntries
            last_log_term: NO_ENTRY,  // Not used for AppendEntries
            granted: None,
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit,
        }
    }

    // Constructor for AppendEntriesResponse message
    pub fn append_entries_response(
        term: u64,
        from: u64,
        to: u64,
        success: bool,
        last_log_index: u64, // The responding follower's last log index
    ) -> Self {
        Self {
            msg_type: MessageType::AppendEntriesResponse,
            term,
            from,
            to,
            last_log_index, // This field is repurposed to carry follower's last log index.
            last_log_term: NO_ENTRY, // Not used
            granted: Some(success),
            prev_log_index: NO_ENTRY,
            prev_log_term: NO_ENTRY,
            entries: Vec::new(),
            leader_commit: NO_ENTRY,
        }
    }

    pub fn get_msg_type(&self) -> MessageType {
        self.msg_type.clone()
    }
}

