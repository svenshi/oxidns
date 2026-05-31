// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Response eligibility and answer-section rewrite policy.
//!
//! The probe layer produces optional scores for IP addresses. This module is
//! the only place that interprets those scores against DNS message semantics:
//! it decides whether a response is eligible, preserves non-target records, and
//! keeps upstream ordering as the deterministic fallback.

use std::collections::VecDeque;
use std::net::IpAddr;

use ahash::{AHashMap, AHashSet};

use crate::core::context::DnsContext;
use crate::proto::{Message, RData, Rcode, Record, RecordType};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ScoreSource {
    Probe,
    Cache,
}

/// Comparable score used by response policy.
///
/// Only successful observations become scores. Failed or missing probes are
/// treated as "no score" so the original upstream ordering remains a stable
/// fallback.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct IpScore {
    pub(super) latency_ms: u64,
    pub(super) source: ScoreSource,
}

/// Address RR candidate extracted from the answer section.
///
/// `original_index` is kept so sorting is stable when latencies are equal or a
/// subset of records has no score. `record` is cloned at extraction time
/// because response rewriting later consumes selected records.
#[derive(Debug, Clone)]
pub(super) struct CandidateRecord {
    pub(super) original_index: usize,
    pub(super) ip: IpAddr,
    pub(super) record: Record,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum SelectionSource {
    Probe,
    Cache,
    Fallback,
}

pub(super) fn eligible_qtype(context: &DnsContext) -> Option<RecordType> {
    // Keep the plugin intentionally narrow. Multi-question packets and
    // non-address queries are left untouched so the executor composes cleanly
    // with other response processors.
    if context.request.question_count() != 1 {
        return None;
    }
    let qtype = context.request.first_qtype()?;
    if qtype != RecordType::A && qtype != RecordType::AAAA {
        return None;
    }
    let response = context.response()?;
    if response.rcode() != Rcode::NoError {
        return None;
    }
    Some(qtype)
}

pub(super) fn candidate_records(
    response: &Message,
    qtype: RecordType,
) -> Option<Vec<CandidateRecord>> {
    // Only answer-section records matching the query address family participate
    // in selection. CNAMEs, authority records, additionals, and unrelated RRsets
    // are preserved exactly as received.
    let candidates: Vec<CandidateRecord> = response
        .answers()
        .iter()
        .enumerate()
        .filter_map(|(idx, record)| {
            if record.rr_type() != qtype {
                return None;
            }
            Some(CandidateRecord {
                original_index: idx,
                ip: record.ip_addr()?,
                record: record.clone(),
            })
        })
        .collect();

    (!candidates.is_empty()).then_some(candidates)
}

pub(super) fn unique_candidate_ips(candidates: &[CandidateRecord]) -> Vec<IpAddr> {
    // Preserve first-seen ordering so probe scheduling remains stable across
    // duplicate records for the same IP.
    let mut seen = AHashSet::new();
    let mut ips = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.ip) {
            ips.push(candidate.ip);
        }
    }
    ips
}

pub(super) fn response_requires_dnssec_caution(context: &DnsContext, qtype: RecordType) -> bool {
    // DO bit means the client has asked for DNSSEC material. RRSIG coverage in
    // the response is also enough to treat the RRset as sensitive, even if the
    // request did not explicitly set DO.
    let request_has_do = context
        .request()
        .edns()
        .as_ref()
        .is_some_and(|edns| edns.flags().dnssec_ok);
    request_has_do
        || context
            .response()
            .is_some_and(|response| response_has_rrsig_for(response, qtype))
}

fn response_has_rrsig_for(response: &Message, qtype: RecordType) -> bool {
    let covered = u16::from(qtype);
    // Scan all message sections because signatures can appear outside the
    // answer section depending on upstream behavior and DNSSEC data layout.
    response
        .answers()
        .iter()
        .chain(response.authorities())
        .chain(response.additionals())
        .chain(response.signature())
        .any(|record| match record.data() {
            RData::RRSIG(sig) => sig.type_covered() == covered,
            RData::SIG(sig) => sig.0.type_covered() == covered,
            _ => false,
        })
}

pub(super) fn apply_response_policy(
    response: &mut Message,
    qtype: RecordType,
    scores: &AHashMap<IpAddr, IpScore>,
    top_n: usize,
) -> SelectionSource {
    let Some(mut candidates) = candidate_records(response, qtype) else {
        return SelectionSource::Fallback;
    };
    if candidates.len() <= 1 {
        return SelectionSource::Fallback;
    }

    let has_any_score = candidates
        .iter()
        .any(|candidate| scores.contains_key(&candidate.ip));
    if !has_any_score {
        return SelectionSource::Fallback;
    }

    candidates.sort_by(
        |left, right| match (scores.get(&left.ip), scores.get(&right.ip)) {
            // Scored records are ordered by latency; ties keep upstream order.
            (Some(left_score), Some(right_score)) => left_score
                .latency_ms
                .cmp(&right_score.latency_ms)
                .then_with(|| left.original_index.cmp(&right.original_index)),
            // Prefer known-good candidates over unscored candidates, but never
            // drop all records solely because probes failed.
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left.original_index.cmp(&right.original_index),
        },
    );

    let selected_len = if top_n == 0 {
        // `top_n = 0` means reorder only. This is also forced for DNSSEC
        // reorder-only mode.
        candidates.len()
    } else {
        top_n.min(candidates.len())
    };
    let selected: Vec<Record> = candidates
        .into_iter()
        .take(selected_len)
        .map(|candidate| candidate.record)
        .collect();

    let source = selected
        .first()
        .and_then(|record| record.ip_addr())
        .and_then(|ip| scores.get(&ip).copied())
        .map(|score| match score.source {
            ScoreSource::Probe => SelectionSource::Probe,
            ScoreSource::Cache => SelectionSource::Cache,
        })
        .unwrap_or(SelectionSource::Fallback);

    replace_target_records(response, qtype, selected);
    source
}

fn replace_target_records(response: &mut Message, qtype: RecordType, selected: Vec<Record>) {
    let mut selected = VecDeque::from(selected);
    let old_answers = response.take_answers();
    for record in old_answers {
        if record.rr_type() == qtype && record.ip_addr().is_some() {
            // Replace only target address RRs. Non-address answers, including
            // CNAME chains, remain in their original relative positions.
            if let Some(selected_record) = selected.pop_front() {
                response.add_answer(selected_record);
            }
        } else {
            response.add_answer(record);
        }
    }
}
