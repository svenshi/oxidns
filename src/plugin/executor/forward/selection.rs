// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::IpAddr;
use std::time::Duration;

use serde::Deserialize;
use tokio::task::{JoinError, JoinSet};
use tracing::{Level, debug, event_enabled, warn};

use super::bogus::BogusAnswerFilter;
use super::is_timeout_error;
use super::metrics::ForwardMetrics;
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
}

/// Result of one concurrent selection round.
#[derive(Debug)]
pub(super) struct SelectionOutcome {
    pub(super) selected: Option<SelectedResponse>,
    pub(super) last_error: Option<String>,
    pub(super) timed_out: bool,
    /// Responses discarded because an answer address matched
    /// `bogus_answer_ips`.
    pub(super) bogus_dropped: usize,
    /// First matching address, used to report which address was rejected.
    pub(super) bogus_answer_ip: Option<IpAddr>,
}

#[derive(Debug)]
struct SelectionState<'a> {
    question: Option<&'a Question>,
    filter: &'a BogusAnswerFilter,
    metrics: &'a ForwardMetrics,
    completed: usize,
    last_error: Option<String>,
    last_timeout: bool,
    best_response: Option<Message>,
    best_response_rank: Option<u8>,
    best_response_disposition: Option<ResponseDisposition>,
    best_negative_response: Option<Message>,
    best_negative_disposition: Option<ResponseDisposition>,
    negative_votes: usize,
    nxdomain_votes: usize,
    nodata_votes: usize,
    bogus_dropped: usize,
    bogus_answer_ip: Option<IpAddr>,
}

impl<'a> SelectionState<'a> {
    fn new(
        question: Option<&'a Question>,
        filter: &'a BogusAnswerFilter,
        metrics: &'a ForwardMetrics,
    ) -> Self {
        Self {
            question,
            filter,
            metrics,
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
            bogus_dropped: 0,
            bogus_answer_ip: None,
        }
    }

    /// Drop the response before classification when it is bogus.
    ///
    /// Bogus answers never become candidates, votes, or ranking entries, and
    /// they do not start the negative grace window.
    fn discard_if_bogus(&mut self, idx: usize, response: &Message) -> bool {
        let Some(ip) = self.filter.bogus_answer_ip(response) else {
            return false;
        };

        self.bogus_dropped += 1;
        self.metrics.record_upstream_bogus(idx);
        if self.bogus_answer_ip.is_none() {
            self.bogus_answer_ip = Some(ip);
        }
        // Per-drop logging is debug-only. Avoid building qname/qtype strings
        // when the level is disabled: bogus answers can be a high-frequency
        // path, and the two allocations below are otherwise paid for nothing.
        if event_enabled!(Level::DEBUG) {
            let (qname, qtype) = match self.question {
                Some(question) => (
                    question.name().normalized().to_string(),
                    format!("{:?}", question.qtype()),
                ),
                None => ("<none>".to_string(), "<none>".to_string()),
            };
            debug!(
                "forward plugin '{}' discarded bogus answer from upstream[{}] for qname {} qtype {} answer {}",
                self.metrics.tag, idx, qname, qtype, ip
            );
        }
        true
    }

    fn record_response(&mut self, idx: usize, response: Message) -> Option<ResponseClass> {
        if self.discard_if_bogus(idx, &response) {
            return None;
        }
        let disposition = classify_dns_response(&response, self.question);
        let class = response_class(disposition);
        if class == ResponseClass::Negative {
            self.negative_votes += 1;
            if let Some(key) = negative_response_key(disposition) {
                self.record_negative_vote(key);
            }
            self.best_negative_response = Some(response);
            self.best_negative_disposition = Some(disposition);
            return Some(class);
        }
        let response_rank = response_rank(disposition);
        if self
            .best_response_rank
            .is_none_or(|best_rank| response_rank >= best_rank)
        {
            self.best_response = Some(response);
            self.best_response_rank = Some(response_rank);
            self.best_response_disposition = Some(disposition);
        }
        Some(class)
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

        self.best_response.take().map(|message| SelectedResponse {
            message,
            disposition: self.best_response_disposition.take(),
        })
    }

    fn take_negative_response(&mut self) -> Option<SelectedResponse> {
        self.best_negative_response
            .take()
            .map(|message| SelectedResponse {
                message,
                disposition: self.best_negative_disposition.take(),
            })
    }

    fn finish(mut self) -> SelectionOutcome {
        let selected = self.take_selected_response();
        SelectionOutcome {
            selected,
            last_error: self.last_error,
            timed_out: self.last_timeout,
            bogus_dropped: self.bogus_dropped,
            bogus_answer_ip: self.bogus_answer_ip,
        }
    }

    fn finish_success(mut self) -> SelectionOutcome {
        SelectionOutcome {
            selected: self.take_selected_response(),
            last_error: None,
            timed_out: false,
            bogus_dropped: self.bogus_dropped,
            bogus_answer_ip: self.bogus_answer_ip,
        }
    }

    fn finish_negative_success(mut self) -> SelectionOutcome {
        SelectionOutcome {
            selected: self.take_negative_response(),
            last_error: None,
            timed_out: false,
            bogus_dropped: self.bogus_dropped,
            bogus_answer_ip: self.bogus_answer_ip,
        }
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
    join_set: &mut JoinSet<(usize, Result<Message>)>,
    active_concurrent: usize,
    question: Option<&Question>,
    mode: ResponseSelectionMode,
    filter: &BogusAnswerFilter,
    metrics: &ForwardMetrics,
) -> SelectionOutcome {
    match mode {
        ResponseSelectionMode::Fastest => select_fastest(join_set, question, filter, metrics).await,
        ResponseSelectionMode::Balanced => {
            select_balanced(join_set, active_concurrent, question, filter, metrics).await
        }
        ResponseSelectionMode::PreferPositive => {
            select_prefer_positive(join_set, active_concurrent, question, filter, metrics).await
        }
        ResponseSelectionMode::Consensus => {
            select_consensus(join_set, active_concurrent, question, filter, metrics).await
        }
    }
}

async fn select_fastest(
    join_set: &mut JoinSet<(usize, Result<Message>)>,
    question: Option<&Question>,
    filter: &BogusAnswerFilter,
    metrics: &ForwardMetrics,
) -> SelectionOutcome {
    let mut state = SelectionState::new(question, filter, metrics);
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((idx, Ok(response))) => {
                if state.discard_if_bogus(idx, &response) {
                    continue;
                }
                join_set.abort_all();
                return SelectionOutcome {
                    selected: Some(SelectedResponse {
                        message: response,
                        disposition: None,
                    }),
                    last_error: None,
                    timed_out: false,
                    bogus_dropped: state.bogus_dropped,
                    bogus_answer_ip: state.bogus_answer_ip,
                };
            }
            Ok((_, Err(err))) => state.record_error(err),
            Err(err) => state.record_join_error(err),
        }
    }
    state.finish()
}

async fn select_prefer_positive(
    join_set: &mut JoinSet<(usize, Result<Message>)>,
    active_concurrent: usize,
    question: Option<&Question>,
    filter: &BogusAnswerFilter,
    metrics: &ForwardMetrics,
) -> SelectionOutcome {
    let mut state = SelectionState::new(question, filter, metrics);
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
    join_set: &mut JoinSet<(usize, Result<Message>)>,
    active_concurrent: usize,
    question: Option<&Question>,
    filter: &BogusAnswerFilter,
    metrics: &ForwardMetrics,
) -> SelectionOutcome {
    let mut state = SelectionState::new(question, filter, metrics);
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
    join_set: &mut JoinSet<(usize, Result<Message>)>,
    active_concurrent: usize,
    question: Option<&Question>,
    filter: &BogusAnswerFilter,
    metrics: &ForwardMetrics,
) -> SelectionOutcome {
    if active_concurrent < CONSENSUS_NEGATIVE_VOTES {
        return select_prefer_positive(join_set, active_concurrent, question, filter, metrics)
            .await;
    }

    let mut state = SelectionState::new(question, filter, metrics);
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
    join_set: &mut JoinSet<(usize, Result<Message>)>,
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
    joined: std::result::Result<(usize, Result<Message>), JoinError>,
    state: &mut SelectionState<'_>,
) -> Option<ResponseClass> {
    state.completed += 1;
    match joined {
        Ok((idx, Ok(response))) => state.record_response(idx, response),
        Ok((_, Err(err))) => {
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
