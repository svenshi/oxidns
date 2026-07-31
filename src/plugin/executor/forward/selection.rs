// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use serde::Deserialize;
use tokio::task::{JoinError, JoinSet};
use tracing::warn;

use super::is_timeout_error;
use crate::core::response::{
    NegativeResponseKind, ResponseDisposition, classify_response as classify_dns_response,
};
use crate::infra::error::{DnsError, Result};
use crate::proto::{Message, Question};

const BALANCED_NEGATIVE_GRACE: Duration = Duration::from_millis(100);
const CONSENSUS_NEGATIVE_VOTES: usize = 2;
const NEGATIVE_RESPONSE_RANK: u8 = 2;

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
pub(super) struct SelectedResponse {
    pub(super) message: Message,
    pub(super) disposition: Option<ResponseDisposition>,
    pub(super) upstream_index: usize,
}

#[derive(Debug)]
pub(super) struct UpstreamAttempt {
    pub(super) upstream_index: usize,
    pub(super) result: Result<Message>,
}

#[derive(Debug)]
struct SelectionState<'a> {
    question: Option<&'a Question>,
    completed: usize,
    last_error: Option<String>,
    last_timeout: bool,
    best_response: Option<(usize, Message)>,
    best_response_rank: Option<u8>,
    best_response_disposition: Option<ResponseDisposition>,
    best_negative_response: Option<(usize, Message)>,
    best_negative_disposition: Option<ResponseDisposition>,
    negative_votes: usize,
    nxdomain_votes: usize,
    nodata_votes: usize,
}

impl<'a> SelectionState<'a> {
    fn new(question: Option<&'a Question>) -> Self {
        Self {
            question,
            completed: 0,
            last_error: None,
            last_timeout: false,
            best_response: None,
            best_response_rank: None,
            best_response_disposition: None,
            best_negative_response: None,
            best_negative_disposition: None,
            negative_votes: 0,
            nxdomain_votes: 0,
            nodata_votes: 0,
        }
    }

    fn record_response(&mut self, upstream_index: usize, response: Message) -> ResponseClass {
        let disposition = classify_dns_response(&response, self.question);
        let class = response_class(disposition);
        if class == ResponseClass::Negative {
            self.negative_votes += 1;
            if let Some(key) = negative_response_key(disposition) {
                self.record_negative_vote(key);
            }
            self.best_negative_response = Some((upstream_index, response));
            self.best_negative_disposition = Some(disposition);
            return class;
        }
        let response_rank = response_rank(disposition);
        if self
            .best_response_rank
            .is_none_or(|best_rank| response_rank >= best_rank)
        {
            self.best_response = Some((upstream_index, response));
            self.best_response_rank = Some(response_rank);
            self.best_response_disposition = Some(disposition);
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

    fn take_selected_response(&mut self) -> Option<SelectedResponse> {
        if self
            .best_response_rank
            .is_none_or(|rank| rank < NEGATIVE_RESPONSE_RANK)
            && let Some(selected) = self.take_negative_response()
        {
            return Some(selected);
        }

        self.best_response
            .take()
            .map(|(upstream_index, message)| SelectedResponse {
                message,
                disposition: self.best_response_disposition.take(),
                upstream_index,
            })
    }

    fn take_negative_response(&mut self) -> Option<SelectedResponse> {
        self.best_negative_response
            .take()
            .map(|(upstream_index, message)| SelectedResponse {
                message,
                disposition: self.best_negative_disposition.take(),
                upstream_index,
            })
    }

    fn finish(mut self) -> (Option<SelectedResponse>, Option<String>, bool) {
        let selected = self.take_selected_response();
        (selected, self.last_error, self.last_timeout)
    }

    fn finish_success(mut self) -> (Option<SelectedResponse>, Option<String>, bool) {
        (self.take_selected_response(), None, false)
    }

    fn finish_negative_success(mut self) -> (Option<SelectedResponse>, Option<String>, bool) {
        (self.take_negative_response(), None, false)
    }

    fn record_negative_vote(&mut self, key: NegativeResponseKey) {
        match key {
            NegativeResponseKey::NxDomain => self.nxdomain_votes += 1,
            NegativeResponseKey::NoData => self.nodata_votes += 1,
        }
    }

    fn has_negative_consensus(&self, required_votes: usize) -> bool {
        self.nxdomain_votes >= required_votes || self.nodata_votes >= required_votes
    }
}

pub(super) async fn select_response(
    join_set: &mut JoinSet<UpstreamAttempt>,
    active_concurrent: usize,
    question: Option<&Question>,
    mode: ResponseSelectionMode,
) -> (Option<SelectedResponse>, Option<String>, bool) {
    match mode {
        ResponseSelectionMode::Fastest => select_fastest(join_set).await,
        ResponseSelectionMode::Balanced => {
            select_balanced(join_set, active_concurrent, question).await
        }
        ResponseSelectionMode::PreferPositive => {
            select_prefer_positive(join_set, active_concurrent, question).await
        }
        ResponseSelectionMode::Consensus => {
            select_consensus(join_set, active_concurrent, question).await
        }
    }
}

async fn select_fastest(
    join_set: &mut JoinSet<UpstreamAttempt>,
) -> (Option<SelectedResponse>, Option<String>, bool) {
    let mut state = SelectionState::new(None);
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(UpstreamAttempt {
                upstream_index,
                result: Ok(response),
            }) => {
                join_set.abort_all();
                return (
                    Some(SelectedResponse {
                        message: response,
                        disposition: None,
                        upstream_index,
                    }),
                    None,
                    false,
                );
            }
            Ok(UpstreamAttempt {
                result: Err(err), ..
            }) => state.record_error(err),
            Err(err) => state.record_join_error(err),
        }
    }
    state.finish()
}

async fn select_prefer_positive(
    join_set: &mut JoinSet<UpstreamAttempt>,
    active_concurrent: usize,
    question: Option<&Question>,
) -> (Option<SelectedResponse>, Option<String>, bool) {
    let mut state = SelectionState::new(question);
    while let Some(class) = next_response_class(join_set, &mut state).await {
        if class == ResponseClass::Positive {
            join_set.abort_all();
            return state.finish_success();
        }
        if state.completed >= active_concurrent {
            break;
        }
    }
    state.finish()
}

async fn select_balanced(
    join_set: &mut JoinSet<UpstreamAttempt>,
    active_concurrent: usize,
    question: Option<&Question>,
) -> (Option<SelectedResponse>, Option<String>, bool) {
    let mut state = SelectionState::new(question);
    let negative_grace = tokio::time::sleep(BALANCED_NEGATIVE_GRACE);
    tokio::pin!(negative_grace);

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
                        return state.finish_success();
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
                return state.finish_success();
            }
        }
    }
}

async fn select_consensus(
    join_set: &mut JoinSet<UpstreamAttempt>,
    active_concurrent: usize,
    question: Option<&Question>,
) -> (Option<SelectedResponse>, Option<String>, bool) {
    if active_concurrent < CONSENSUS_NEGATIVE_VOTES {
        return select_prefer_positive(join_set, active_concurrent, question).await;
    }

    let mut state = SelectionState::new(question);
    while let Some(class) = next_response_class(join_set, &mut state).await {
        match class {
            ResponseClass::Positive => {
                join_set.abort_all();
                return state.finish_success();
            }
            ResponseClass::Negative if state.has_negative_consensus(CONSENSUS_NEGATIVE_VOTES) => {
                join_set.abort_all();
                return state.finish_negative_success();
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
    join_set: &mut JoinSet<UpstreamAttempt>,
    state: &mut SelectionState<'_>,
) -> Option<ResponseClass> {
    loop {
        let joined = join_set.join_next().await?;
        if let Some(class) = handle_joined_response(joined, state) {
            return Some(class);
        }
    }
}

fn handle_joined_response(
    joined: std::result::Result<UpstreamAttempt, JoinError>,
    state: &mut SelectionState<'_>,
) -> Option<ResponseClass> {
    state.completed += 1;
    match joined {
        Ok(UpstreamAttempt {
            upstream_index,
            result: Ok(response),
        }) => Some(state.record_response(upstream_index, response)),
        Ok(UpstreamAttempt {
            result: Err(err), ..
        }) => {
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
fn response_class(disposition: ResponseDisposition) -> ResponseClass {
    match disposition {
        ResponseDisposition::CompletePositive => ResponseClass::Positive,
        ResponseDisposition::DefinitiveNegative(_) => ResponseClass::Negative,
        ResponseDisposition::IncompleteAlias | ResponseDisposition::Other => ResponseClass::Other,
    }
}

fn negative_response_key(disposition: ResponseDisposition) -> Option<NegativeResponseKey> {
    match disposition.negative_kind() {
        Some(NegativeResponseKind::NxDomain) => Some(NegativeResponseKey::NxDomain),
        Some(NegativeResponseKind::NoData) => Some(NegativeResponseKey::NoData),
        None => None,
    }
}

fn response_rank(disposition: ResponseDisposition) -> u8 {
    match disposition {
        ResponseDisposition::CompletePositive => 4,
        ResponseDisposition::IncompleteAlias => 3,
        ResponseDisposition::DefinitiveNegative(_) => NEGATIVE_RESPONSE_RANK,
        ResponseDisposition::Other => 1,
    }
}
