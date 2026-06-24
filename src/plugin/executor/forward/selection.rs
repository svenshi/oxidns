// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use serde::Deserialize;
use tokio::task::{JoinError, JoinSet};
use tracing::warn;

use super::is_timeout_error;
use crate::infra::error::{DnsError, Result};
use crate::proto::{Message, Rcode};

const BALANCED_NEGATIVE_GRACE: Duration = Duration::from_millis(100);
const CONSENSUS_NEGATIVE_VOTES: usize = 2;

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseSelectionMode {
    /// First DNS response wins. Transport errors never win.
    Fastest,
    /// Positive answers win immediately; negative answers wait briefly.
    #[default]
    Balanced,
    /// Positive answers win immediately; negative answers wait for all
    /// attempts.
    PreferPositive,
    /// Positive answers win immediately; negative answers need two
    /// confirmations.
    Consensus,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ResponseClass {
    Positive,
    Negative,
    Other,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum NegativeResponseKey {
    NxDomain,
    NoData,
}

#[derive(Debug)]
struct NegativeVote {
    key: NegativeResponseKey,
    count: usize,
}

#[derive(Debug)]
struct SelectionState {
    completed: usize,
    last_error: Option<String>,
    last_timeout: bool,
    best_response: Option<Message>,
    negative_votes: usize,
    negative_vote_buckets: Vec<NegativeVote>,
}

impl SelectionState {
    fn new() -> Self {
        Self {
            completed: 0,
            last_error: None,
            last_timeout: false,
            best_response: None,
            negative_votes: 0,
            negative_vote_buckets: Vec::new(),
        }
    }

    fn record_response(&mut self, response: Message) -> ResponseClass {
        let class = classify_response(&response);
        if class == ResponseClass::Negative {
            self.negative_votes += 1;
            if let Some(key) = negative_response_key(&response) {
                self.record_negative_vote(key);
            }
        }
        if should_replace_best(self.best_response.as_ref(), &response) {
            self.best_response = Some(response);
        }
        class
    }

    fn record_error(&mut self, err: DnsError) {
        warn!("DNS query failed: {}", err);
        self.last_timeout |= is_timeout_error(&err);
        self.last_error = Some(err.to_string());
    }

    fn record_join_error(&mut self, err: JoinError) {
        self.last_error = Some(format!("forward subtask join failed: {}", err));
    }

    fn finish(self) -> (Option<Message>, Option<String>, bool) {
        (self.best_response, self.last_error, self.last_timeout)
    }

    fn record_negative_vote(&mut self, key: NegativeResponseKey) {
        if let Some(bucket) = self
            .negative_vote_buckets
            .iter_mut()
            .find(|bucket| bucket.key == key)
        {
            bucket.count += 1;
            return;
        }
        self.negative_vote_buckets
            .push(NegativeVote { key, count: 1 });
    }

    fn has_negative_consensus(&self, required_votes: usize) -> bool {
        self.negative_vote_buckets
            .iter()
            .any(|bucket| bucket.count >= required_votes)
    }
}

pub(super) async fn select_response(
    join_set: &mut JoinSet<Result<Message>>,
    active_concurrent: usize,
    mode: ResponseSelectionMode,
) -> (Option<Message>, Option<String>, bool) {
    match mode {
        ResponseSelectionMode::Fastest => select_fastest(join_set).await,
        ResponseSelectionMode::Balanced => select_balanced(join_set, active_concurrent).await,
        ResponseSelectionMode::PreferPositive => {
            select_prefer_positive(join_set, active_concurrent).await
        }
        ResponseSelectionMode::Consensus => select_consensus(join_set, active_concurrent).await,
    }
}

async fn select_fastest(
    join_set: &mut JoinSet<Result<Message>>,
) -> (Option<Message>, Option<String>, bool) {
    let mut state = SelectionState::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok(response)) => {
                join_set.abort_all();
                return (Some(response), None, false);
            }
            Ok(Err(err)) => state.record_error(err),
            Err(err) => state.record_join_error(err),
        }
    }
    state.finish()
}

async fn select_prefer_positive(
    join_set: &mut JoinSet<Result<Message>>,
    active_concurrent: usize,
) -> (Option<Message>, Option<String>, bool) {
    let mut state = SelectionState::new();
    while let Some(class) = next_response_class(join_set, &mut state).await {
        if class == ResponseClass::Positive {
            join_set.abort_all();
            return (state.best_response, None, false);
        }
        if state.completed >= active_concurrent {
            break;
        }
    }
    state.finish()
}

async fn select_balanced(
    join_set: &mut JoinSet<Result<Message>>,
    active_concurrent: usize,
) -> (Option<Message>, Option<String>, bool) {
    let mut state = SelectionState::new();
    let mut negative_grace =
        std::pin::Pin::from(Box::new(tokio::time::sleep(BALANCED_NEGATIVE_GRACE)));

    loop {
        tokio::select! {
            joined = join_set.join_next() => {
                let Some(joined) = joined else {
                    return state.finish();
                };
                let Some(class) = handle_joined_response(joined, &mut state) else {
                    if state.completed >= active_concurrent {
                        return state.finish();
                    }
                    continue;
                };
                match class {
                    ResponseClass::Positive => {
                        join_set.abort_all();
                        return (state.best_response, None, false);
                    }
                    ResponseClass::Negative => {
                        if state.completed >= active_concurrent {
                            return state.finish();
                        }
                        if state.negative_votes == 1 {
                            negative_grace.as_mut().reset(tokio::time::Instant::now() + BALANCED_NEGATIVE_GRACE);
                        }
                    }
                    ResponseClass::Other => {
                        if state.completed >= active_concurrent {
                            return state.finish();
                        }
                    }
                }
            }
            _ = &mut negative_grace, if state.negative_votes > 0 => {
                join_set.abort_all();
                return (state.best_response, None, false);
            }
        }
    }
}

async fn select_consensus(
    join_set: &mut JoinSet<Result<Message>>,
    active_concurrent: usize,
) -> (Option<Message>, Option<String>, bool) {
    if active_concurrent < CONSENSUS_NEGATIVE_VOTES {
        return select_prefer_positive(join_set, active_concurrent).await;
    }

    let mut state = SelectionState::new();
    while let Some(class) = next_response_class(join_set, &mut state).await {
        match class {
            ResponseClass::Positive => {
                join_set.abort_all();
                return (state.best_response, None, false);
            }
            ResponseClass::Negative if state.has_negative_consensus(CONSENSUS_NEGATIVE_VOTES) => {
                join_set.abort_all();
                return (state.best_response, None, false);
            }
            ResponseClass::Negative | ResponseClass::Other => {
                if state.completed >= active_concurrent {
                    break;
                }
            }
        }
    }
    state.finish()
}

async fn next_response_class(
    join_set: &mut JoinSet<Result<Message>>,
    state: &mut SelectionState,
) -> Option<ResponseClass> {
    loop {
        let joined = join_set.join_next().await?;
        if let Some(class) = handle_joined_response(joined, state) {
            return Some(class);
        }
    }
}

fn handle_joined_response(
    joined: std::result::Result<Result<Message>, JoinError>,
    state: &mut SelectionState,
) -> Option<ResponseClass> {
    state.completed += 1;
    match joined {
        Ok(Ok(response)) => Some(state.record_response(response)),
        Ok(Err(err)) => {
            state.record_error(err);
            None
        }
        Err(err) => {
            state.record_join_error(err);
            None
        }
    }
}

#[inline]
fn classify_response(response: &Message) -> ResponseClass {
    match response.rcode() {
        Rcode::NoError if !response.answers().is_empty() => ResponseClass::Positive,
        Rcode::NoError | Rcode::NXDomain => ResponseClass::Negative,
        _ => ResponseClass::Other,
    }
}

fn negative_response_key(response: &Message) -> Option<NegativeResponseKey> {
    match response.rcode() {
        Rcode::NXDomain => Some(NegativeResponseKey::NxDomain),
        Rcode::NoError if response.answers().is_empty() => Some(NegativeResponseKey::NoData),
        _ => None,
    }
}

fn should_replace_best(current: Option<&Message>, candidate: &Message) -> bool {
    let Some(current) = current else {
        return true;
    };
    response_rank(candidate) >= response_rank(current)
}

fn response_rank(response: &Message) -> u8 {
    match classify_response(response) {
        ResponseClass::Positive => 3,
        ResponseClass::Negative => 2,
        ResponseClass::Other => 1,
    }
}
