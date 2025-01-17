// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use futures::{
    channel::{
        mpsc::{UnboundedReceiver, UnboundedSender},
        oneshot,
    },
    SinkExt, StreamExt,
};
use tokio::time::Duration;

use consensus_types::{common::Author, executed_block::ExecutedBlock};
use diem_logger::prelude::*;
use diem_types::{
    account_address::AccountAddress,
    ledger_info::{LedgerInfo, LedgerInfoWithSignatures},
    validator_verifier::ValidatorVerifier,
};

use crate::{
    experimental::{
        buffer_item::BufferItem,
        execution_phase::{ExecutionRequest, ExecutionResponse},
        linkedlist::{find_elem, get_elem, get_next, link_eq, set_elem, take_elem, Link, List},
        persisting_phase::PersistingRequest,
        signing_phase::{SigningRequest, SigningResponse},
    },
    network::NetworkSender,
    network_interface::ConsensusMsg,
    round_manager::VerifiedEvent,
    state_replication::StateComputerCommitCallBackType,
};
use diem_crypto::HashValue;
use futures::channel::mpsc::unbounded;

pub const BUFFER_MANAGER_RETRY_INTERVAL: u64 = 1000;

pub type ResetAck = ();

pub fn sync_ack_new() -> ResetAck {}

pub struct ResetRequest {
    pub tx: oneshot::Sender<ResetAck>,
    pub reconfig: bool,
}

pub struct OrderedBlocks {
    pub ordered_blocks: Vec<ExecutedBlock>,
    pub ordered_proof: LedgerInfoWithSignatures,
    pub callback: StateComputerCommitCallBackType,
}

pub type BufferItemRootType = Link<BufferItem>;
pub type Sender<T> = UnboundedSender<T>;
pub type Receiver<T> = UnboundedReceiver<T>;

pub fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    unbounded::<T>()
}

/// BufferManager handles the states of ordered blocks and
/// interacts with the execution phase, the signing phase, and
/// the persisting phase.
pub struct BufferManager {
    author: Author,

    buffer: List<BufferItem>,

    // the roots point to the first *unprocessed* item.
    // None means no items ready to be processed (either all processed or no item finishes previous stage)
    execution_root: BufferItemRootType,
    execution_phase_tx: Sender<ExecutionRequest>,
    execution_phase_rx: Receiver<ExecutionResponse>,

    signing_root: BufferItemRootType,
    signing_phase_tx: Sender<SigningRequest>,
    signing_phase_rx: Receiver<SigningResponse>,

    commit_msg_tx: NetworkSender,
    commit_msg_rx: channel::diem_channel::Receiver<AccountAddress, VerifiedEvent>,

    // we don't hear back from the persisting phase
    persisting_phase_tx: Sender<PersistingRequest>,

    block_rx: UnboundedReceiver<OrderedBlocks>,
    reset_rx: UnboundedReceiver<ResetRequest>,
    epoch_ends: bool,

    verifier: ValidatorVerifier,
}

impl BufferManager {
    pub fn new(
        author: Author,
        execution_phase_tx: Sender<ExecutionRequest>,
        execution_phase_rx: Receiver<ExecutionResponse>,
        signing_phase_tx: Sender<SigningRequest>,
        signing_phase_rx: Receiver<SigningResponse>,
        commit_msg_tx: NetworkSender,
        commit_msg_rx: channel::diem_channel::Receiver<AccountAddress, VerifiedEvent>,
        persisting_phase_tx: Sender<PersistingRequest>,
        block_rx: UnboundedReceiver<OrderedBlocks>,
        sync_rx: UnboundedReceiver<ResetRequest>,
        verifier: ValidatorVerifier,
    ) -> Self {
        let buffer = List::<BufferItem>::new();

        Self {
            author,

            buffer,

            execution_root: None,
            execution_phase_tx,
            execution_phase_rx,

            signing_root: None,
            signing_phase_tx,
            signing_phase_rx,

            commit_msg_tx,
            commit_msg_rx,

            persisting_phase_tx,

            block_rx,
            reset_rx: sync_rx,
            epoch_ends: false,

            verifier,
        }
    }

    /// process incoming ordered blocks
    /// push them into the buffer and update the roots if they are none.
    fn process_ordered_blocks(&mut self, ordered_blocks: OrderedBlocks) {
        let OrderedBlocks {
            ordered_blocks,
            ordered_proof,
            callback,
        } = ordered_blocks;

        let item = BufferItem::new_ordered(ordered_blocks, ordered_proof, callback);
        self.buffer.push_back(item);
    }

    /// Set the execution root to the first not executed item (Ordered) and send execution request
    /// Set to None if not exist
    async fn advance_execution_root(&mut self) {
        let cursor = self
            .execution_root
            .clone()
            .or_else(|| self.buffer.head.clone());
        self.execution_root = find_elem(cursor, |item| item.is_ordered());
        if self.execution_root.is_some() {
            let ordered_blocks = get_elem(&self.execution_root).get_blocks().clone();
            self.execution_phase_tx
                .send(ExecutionRequest { ordered_blocks })
                .await
                .expect("Failed to send execution request")
        }
    }

    /// Set the signing root to the first not signed item (Executed) and send execution request
    /// Set to None if not exist
    async fn advance_signing_root(&mut self) {
        let cursor = self
            .signing_root
            .clone()
            .or_else(|| self.buffer.head.clone());
        self.signing_root = find_elem(cursor, |item| item.is_executed());
        if self.signing_root.is_some() {
            let item = get_elem(&self.signing_root);
            let executed_item = item.unwrap_executed_ref();
            let commit_ledger_info = LedgerInfo::new(
                executed_item.executed_blocks.last().unwrap().block_info(),
                executed_item
                    .ordered_proof
                    .ledger_info()
                    .consensus_data_hash(),
            );
            self.signing_phase_tx
                .send(SigningRequest {
                    ordered_ledger_info: executed_item.ordered_proof.clone(),
                    commit_ledger_info,
                })
                .await
                .expect("Failed to send signing request");
        }
    }

    /// Pop the prefix of buffer items until (including) target_block_id
    /// Send persist request.
    async fn advance_head(&mut self, target_block_id: HashValue) {
        let mut blocks_to_persist: Vec<Arc<ExecutedBlock>> = vec![];
        // reset if signing root is part of the aggregated prefix, this is not efficient we probably should revisit it later
        let reset_signing = find_elem(self.signing_root.clone(), |item| {
            item.block_id() == target_block_id
        })
        .is_some();
        if reset_signing {
            self.signing_root = None;
        }

        while let Some(item) = self.buffer.pop_front() {
            blocks_to_persist.extend(
                item.get_blocks()
                    .iter()
                    .map(|eb| Arc::new(eb.clone()))
                    .collect::<Vec<Arc<ExecutedBlock>>>(),
            );
            if item.block_id() == target_block_id {
                let aggregated_item = item.unwrap_aggregated();
                self.persisting_phase_tx
                    .send(PersistingRequest {
                        blocks: blocks_to_persist,
                        commit_ledger_info: aggregated_item.commit_proof,
                        // we use the last callback
                        // this is okay because the callback function (from BlockStore::commit)
                        // takes in the actual blocks and ledger info from the state computer
                        // the encoded values are references to the block_tree, storage, and a commit root
                        // the block_tree and storage are the same for all the callbacks in the current epoch
                        // the commit root is used in logging only.
                        callback: aggregated_item.callback,
                    })
                    .await
                    .expect("Failed to send persist request");
                if reset_signing {
                    self.advance_signing_root().await;
                }
                return;
            }
        }
        unreachable!("Aggregated item not found in the list");
    }

    /// It pops everything in the buffer and if reconfig flag is set, it stops the main loop
    fn process_reset_request(&mut self, request: ResetRequest) {
        let ResetRequest { tx, reconfig } = request;

        self.epoch_ends = reconfig;
        self.buffer = List::new();
        self.execution_root = None;
        self.signing_root = None;

        tx.send(sync_ack_new()).unwrap();
    }

    /// If the response is successful, advance the item to Executed, otherwise panic (TODO fix).
    async fn process_execution_response(&mut self, response: ExecutionResponse) {
        let ExecutionResponse { inner } = response;
        let executed_blocks = inner.expect("Execution failure");
        let block_id = executed_blocks.last().unwrap().id();

        // find the corresponding item, may not exist if a reset or aggregated happened
        let current_cursor = find_elem(self.execution_root.clone(), |item| {
            item.block_id() == block_id
        });

        if current_cursor.is_some() {
            let item = take_elem(&current_cursor);
            let new_item = item.advance_to_executed_or_aggregated(executed_blocks, &self.verifier);
            let aggregated = new_item.is_aggregated();
            set_elem(&current_cursor, new_item);
            if aggregated {
                self.advance_head(block_id).await;
            }
        }
    }

    /// If the signing response is successful, advance the item to Signed and broadcast commit votes.
    async fn process_signing_response(&mut self, response: SigningResponse) {
        let SigningResponse {
            signature_result,
            commit_ledger_info,
        } = response;
        let signature = match signature_result {
            Ok(sig) => sig,
            Err(e) => {
                error!("Signing failed {:?}", e);
                return;
            }
        };
        // find the corresponding item, may not exist if a reset or aggregated happened
        let current_cursor = find_elem(self.signing_root.clone(), |item| {
            item.block_id() == commit_ledger_info.commit_info().id()
        });
        if current_cursor.is_some() {
            let item = take_elem(&current_cursor);
            // it is possible that we already signed this buffer item (double check after the final integration)
            if item.is_executed() {
                // we have found the buffer item
                let signed_item = item.advance_to_signed(self.author, signature);
                let commit_vote = signed_item.unwrap_signed_ref().commit_vote.clone();

                set_elem(&current_cursor, signed_item);

                self.commit_msg_tx
                    .broadcast(ConsensusMsg::CommitVoteMsg(Box::new(commit_vote)))
                    .await;
            }
        }
    }

    /// process the commit vote messages
    /// it scans the whole buffer for a matching blockinfo
    /// if found, try advancing the item to be aggregated
    fn process_commit_message(&mut self, commit_msg: VerifiedEvent) -> Option<HashValue> {
        match commit_msg {
            VerifiedEvent::CommitVote(vote) => {
                // find the corresponding item
                let target_block_id = vote.commit_info().id();
                let current_cursor = find_elem(self.buffer.head.clone(), |item| {
                    item.block_id() == target_block_id
                });
                if current_cursor.is_some() {
                    let mut item = take_elem(&current_cursor);
                    let new_item = match item.add_signature_if_matched(*vote) {
                        Ok(()) => item.try_advance_to_aggregated(&self.verifier),
                        Err(e) => {
                            error!("Failed to add commit vote {:?}", e);
                            item
                        }
                    };
                    set_elem(&current_cursor, new_item);
                    if get_elem(&current_cursor).is_aggregated() {
                        return Some(target_block_id);
                    }
                }
            }
            VerifiedEvent::CommitDecision(commit_proof) => {
                let target_block_id = commit_proof.ledger_info().commit_info().id();
                let cursor = find_elem(self.buffer.head.clone(), |item| {
                    item.block_id() == target_block_id
                });
                if cursor.is_some() {
                    let item = take_elem(&cursor);
                    let new_item = item.try_advance_to_aggregated_with_ledger_info(
                        commit_proof.ledger_info().clone(),
                    );
                    let aggregated = new_item.is_aggregated();
                    set_elem(&cursor, new_item);
                    if aggregated {
                        return Some(target_block_id);
                    }
                }
            }
            _ => {
                unreachable!();
            }
        }
        None
    }

    /// this function retries all the items until the signing root
    /// note that there might be other signed items after the signing root
    async fn retry_broadcasting_commit_votes(&mut self) {
        let mut cursor = self.buffer.head.clone();
        while cursor.is_some() && !link_eq(&cursor, &self.signing_root) {
            {
                let item = get_elem(&cursor);
                let signed_item = item.unwrap_signed_ref();
                self.commit_msg_tx
                    .broadcast(ConsensusMsg::CommitVoteMsg(Box::new(
                        signed_item.commit_vote.clone(),
                    )))
                    .await;
            }
            cursor = get_next(&cursor);
        }
    }

    pub async fn start(mut self) {
        info!("Buffer manager starts.");
        let mut interval =
            tokio::time::interval(Duration::from_millis(BUFFER_MANAGER_RETRY_INTERVAL));
        while !self.epoch_ends {
            // advancing the root will trigger sending requests to the pipeline
            tokio::select! {
                Some(blocks) = self.block_rx.next() => {
                    self.process_ordered_blocks(blocks);
                    if self.execution_root.is_none() {
                        self.advance_execution_root().await;
                    }
                }
                Some(reset_event) = self.reset_rx.next() => {
                    self.process_reset_request(reset_event);
                }
                Some(response) = self.execution_phase_rx.next() => {
                    self.process_execution_response(response).await;
                    self.advance_execution_root().await;
                    if self.signing_root.is_none() {
                        self.advance_signing_root().await;
                    }
                }
                Some(response) = self.signing_phase_rx.next() => {
                    self.process_signing_response(response).await;
                    self.advance_signing_root().await;
                }
                Some(commit_msg) = self.commit_msg_rx.next() => {
                    if let Some(aggregated_block_id) = self.process_commit_message(commit_msg) {
                        self.advance_head(aggregated_block_id).await;
                    }
                }
                _ = interval.tick() => {
                    self.retry_broadcasting_commit_votes().await;
                }
                // no else branch here because interval.tick will always be available
            }
        }
    }
}
