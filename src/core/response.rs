// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Query-aware DNS response classification shared by response consumers.
//!
//! A response is only positive when the requested RR type belongs to the
//! original QNAME or the terminal name of its CNAME chain.  This keeps an
//! alias-only response from satisfying an address-query cache key while still
//! preserving it as a useful fallback when no complete response is available.

use crate::proto::{DNSClass, Message, Name, Question, Rcode, RecordType};

/// Maximum CNAME hops inspected in one response.
///
/// The bound prevents a malformed answer section from turning response
/// classification into an unbounded hot-path operation.  A chain reaching
/// this limit is treated as malformed rather than complete.
const MAX_CNAME_HOPS: usize = 16;

/// A definitive negative response kind.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NegativeResponseKind {
    NxDomain,
    NoData,
}

/// How well an upstream response answers its original question.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResponseDisposition {
    /// The requested RR type exists at the QNAME or the CNAME-chain terminal.
    CompletePositive,
    /// The response definitively denies the requested name or type.
    DefinitiveNegative(NegativeResponseKind),
    /// The response proves an alias exists but does not prove a final answer.
    IncompleteAlias,
    /// A malformed, irrelevant, or otherwise non-terminal response.
    Other,
}

impl ResponseDisposition {
    #[inline]
    pub fn is_complete_positive(self) -> bool {
        matches!(self, Self::CompletePositive)
    }

    #[inline]
    pub fn negative_kind(self) -> Option<NegativeResponseKind> {
        match self {
            Self::DefinitiveNegative(kind) => Some(kind),
            _ => None,
        }
    }
}

/// Classify a response against its original DNS question.
///
/// When there is no question, retain the historic conservative behavior:
/// non-empty `NOERROR` responses are usable positives and empty `NOERROR`
/// responses are NODATA.  Normal request processing always supplies a
/// question.
#[inline]
#[hotpath::measure]
pub fn classify_response(response: &Message, question: Option<&Question>) -> ResponseDisposition {
    if let Some(question) = question
        && response.first_question().is_some_and(|response_question| {
            !std::ptr::eq(response_question, question) && response_question != question
        })
    {
        return ResponseDisposition::Other;
    }

    match response.rcode() {
        Rcode::NXDomain => {
            return ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NxDomain);
        }
        Rcode::NoError => {}
        _ => return ResponseDisposition::Other,
    }

    let Some(question) = question else {
        return if response.answers().is_empty() {
            ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NoData)
        } else {
            ResponseDisposition::CompletePositive
        };
    };

    let qtype = question.qtype();
    if qtype == RecordType::ANY {
        return if has_any_answer_at_name(response, question.name(), question.qclass()) {
            ResponseDisposition::CompletePositive
        } else if response.answers().is_empty()
            || has_negative_soa_for_class(response, question.qclass())
        {
            ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NoData)
        } else {
            ResponseDisposition::Other
        };
    }

    if qtype == RecordType::CNAME {
        return if response.answers().iter().any(|record| {
            record.name() == question.name()
                && record.class() == question.qclass()
                && record.rr_type() == RecordType::CNAME
        }) {
            ResponseDisposition::CompletePositive
        } else if response.answers().is_empty()
            || has_negative_soa_for_class(response, question.qclass())
        {
            ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NoData)
        } else {
            ResponseDisposition::Other
        };
    }

    let mut current = question.name();
    let mut saw_alias = false;

    for hop in 0..=MAX_CNAME_HOPS {
        let target = match inspect_answers_at_name(response, current, qtype, question.qclass()) {
            OwnerAnswer::Requested => return ResponseDisposition::CompletePositive,
            OwnerAnswer::Alias(target) => target,
            OwnerAnswer::ConflictingAlias => return ResponseDisposition::Other,
            OwnerAnswer::None => break,
        };
        if hop == MAX_CNAME_HOPS || target == current {
            return ResponseDisposition::Other;
        }

        saw_alias = true;
        current = target;
    }

    if response.answers().is_empty() || has_negative_soa_for_class(response, question.qclass()) {
        ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NoData)
    } else if saw_alias {
        ResponseDisposition::IncompleteAlias
    } else {
        ResponseDisposition::Other
    }
}

enum OwnerAnswer<'a> {
    Requested,
    Alias(&'a Name),
    ConflictingAlias,
    None,
}

/// Inspect one CNAME-chain owner with a single pass over the answer section.
///
/// A requested RR takes precedence over conflicting CNAME records at the same
/// owner, matching the classifier's historic positive-answer behavior.
#[inline]
fn inspect_answers_at_name<'a>(
    response: &'a Message,
    name: &Name,
    record_type: RecordType,
    dns_class: DNSClass,
) -> OwnerAnswer<'a> {
    let mut cname_target = None;
    let mut conflicting_alias = false;

    for record in response.answers() {
        if record.name() != name || record.class() != dns_class {
            continue;
        }
        if record.rr_type() == record_type {
            return OwnerAnswer::Requested;
        }

        let Some(candidate) = record.cname_target() else {
            continue;
        };
        if let Some(existing) = cname_target {
            conflicting_alias |= existing != candidate;
        } else {
            cname_target = Some(candidate);
        }
    }

    if conflicting_alias {
        OwnerAnswer::ConflictingAlias
    } else if let Some(target) = cname_target {
        OwnerAnswer::Alias(target)
    } else {
        OwnerAnswer::None
    }
}

#[inline]
fn has_any_answer_at_name(response: &Message, name: &Name, dns_class: DNSClass) -> bool {
    response
        .answers()
        .iter()
        .any(|record| record.name() == name && record.class() == dns_class)
}

#[inline]
fn has_negative_soa_for_class(response: &Message, dns_class: DNSClass) -> bool {
    response
        .authorities()
        .iter()
        .any(|record| record.class() == dns_class && record.rr_type() == RecordType::SOA)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::proto::rdata::{A, CNAME, SOA};
    use crate::proto::{DNSClass, RData, Record};

    fn question(name: &str, qtype: RecordType) -> Question {
        Question::new(Name::from_ascii(name).unwrap(), qtype, DNSClass::IN)
    }

    fn response_with_question(question: Question) -> Message {
        let mut response = Message::new();
        response.set_rcode(Rcode::NoError);
        response.add_question(question);
        response
    }

    fn add_cname(response: &mut Message, owner: &str, target: &str) {
        response.add_answer(Record::from_rdata(
            Name::from_ascii(owner).unwrap(),
            60,
            RData::CNAME(CNAME(Name::from_ascii(target).unwrap())),
        ));
    }

    #[test]
    fn follows_cname_chain_to_requested_address_type() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge.example.com.");
        add_cname(&mut response, "edge.example.com.", "origin.example.com.");
        response.add_answer(Record::from_rdata(
            Name::from_ascii("origin.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::CompletePositive
        );
    }

    #[test]
    fn rejects_unrelated_requested_type_in_answer_section() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge.example.com.");
        response.add_answer(Record::from_rdata(
            Name::from_ascii("unrelated.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::IncompleteAlias
        );
    }

    #[test]
    fn recognizes_alias_nodata_from_soa() {
        let request = question("www.example.com.", RecordType::AAAA);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge.example.com.");
        response.add_authority(Record::from_rdata(
            Name::from_ascii("example.com.").unwrap(),
            120,
            RData::SOA(SOA::new(
                Name::from_ascii("ns1.example.com.").unwrap(),
                Name::from_ascii("hostmaster.example.com.").unwrap(),
                1,
                2,
                3,
                4,
                60,
            )),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::DefinitiveNegative(NegativeResponseKind::NoData)
        );
    }

    #[test]
    fn marks_alias_loop_as_other() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge.example.com.");
        add_cname(&mut response, "edge.example.com.", "www.example.com.");

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::Other
        );
    }

    #[test]
    fn marks_conflicting_alias_targets_without_requested_rr_as_other() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge-a.example.com.");
        add_cname(&mut response, "www.example.com.", "edge-b.example.com.");

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::Other
        );
    }

    #[test]
    fn requested_rr_wins_over_conflicting_alias_targets_at_same_owner() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        add_cname(&mut response, "www.example.com.", "edge-a.example.com.");
        add_cname(&mut response, "www.example.com.", "edge-b.example.com.");
        response.add_answer(Record::from_rdata(
            Name::from_ascii("www.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::CompletePositive
        );
    }

    #[test]
    fn keeps_any_and_cname_queries_complete() {
        let any_request = question("www.example.com.", RecordType::ANY);
        let cname_request = question("www.example.com.", RecordType::CNAME);
        let mut any_response = response_with_question(any_request.clone());
        add_cname(&mut any_response, "www.example.com.", "edge.example.com.");
        let mut cname_response = response_with_question(cname_request.clone());
        add_cname(&mut cname_response, "www.example.com.", "edge.example.com.");

        assert_eq!(
            classify_response(&any_response, Some(&any_request)),
            ResponseDisposition::CompletePositive
        );
        assert_eq!(
            classify_response(&cname_response, Some(&cname_request)),
            ResponseDisposition::CompletePositive
        );
    }

    #[test]
    fn rejects_mismatched_echoed_question() {
        let request = question("www.example.com.", RecordType::A);
        let response_question = question("other.example.com.", RecordType::A);
        let mut response = response_with_question(response_question);
        response.set_rcode(Rcode::NXDomain);
        response.add_answer(Record::from_rdata(
            Name::from_ascii("www.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::Other
        );
    }

    #[test]
    fn rejects_requested_type_from_another_dns_class() {
        let request = question("www.example.com.", RecordType::A);
        let mut response = response_with_question(request.clone());
        response.add_answer(Record::from_rdata_with_class(
            Name::from_ascii("www.example.com.").unwrap(),
            60,
            DNSClass::CH,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::Other
        );
    }

    #[test]
    fn rejects_unrelated_any_answer() {
        let request = question("www.example.com.", RecordType::ANY);
        let mut response = response_with_question(request.clone());
        response.add_answer(Record::from_rdata(
            Name::from_ascii("unrelated.example.com.").unwrap(),
            60,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
        ));

        assert_eq!(
            classify_response(&response, Some(&request)),
            ResponseDisposition::Other
        );
    }
}
