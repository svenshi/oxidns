// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! DNS query construction and answer selection for resolver nameservers.

use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;

use crate::infra::error::{DnsError, Result};
use crate::proto::{DNSClass, Message, MessageType, Name, Opcode, Question, Record, RecordType};

#[derive(Clone, Debug)]
pub(super) struct ResolveQuery {
    message: Message,
    query_name: Name,
}

impl ResolveQuery {
    pub(super) fn new(domain: &str, ip_version: Option<u8>) -> Result<Self> {
        let query_name = Name::from_str(domain).map_err(|err| {
            DnsError::plugin(format!(
                "invalid resolver target domain '{}': {}",
                domain, err
            ))
        })?;

        let mut message = Message::new();
        message.set_message_type(MessageType::Query);
        message.set_opcode(Opcode::Query);
        message.set_recursion_desired(true);
        message.add_question(Question::new(
            query_name.clone(),
            match ip_version {
                Some(6) => RecordType::AAAA,
                _ => RecordType::A,
            },
            DNSClass::IN,
        ));

        Ok(Self {
            message,
            query_name,
        })
    }

    pub(super) fn message_template(&self) -> Message {
        self.message.clone()
    }

    pub(super) fn query_name(&self) -> Name {
        self.query_name.clone()
    }

    #[cfg(test)]
    fn first_question(&self) -> Option<&Question> {
        self.message.first_question()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ResolvedAnswer {
    pub(super) ip: IpAddr,
    pub(super) ttl_seconds: u32,
    pub(super) record_type: RecordType,
}

pub(super) fn select_answer(
    answers: &[Record],
    query_name: &Name,
    expected_type: RecordType,
) -> Option<ResolvedAnswer> {
    let mut accepted_names = HashMap::new();
    accepted_names.insert(query_name.clone(), u32::MAX);

    loop {
        let mut changed = false;
        for answer in answers {
            let Some(target) = answer.cname_target() else {
                continue;
            };
            let Some(owner_ttl) = accepted_names.get(answer.name()).copied() else {
                continue;
            };
            let ttl = owner_ttl.min(answer.ttl());
            match accepted_names.get(target).copied() {
                Some(existing_ttl) if existing_ttl <= ttl => {}
                _ => {
                    accepted_names.insert(target.clone(), ttl);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    for answer in answers {
        if answer.rr_type() != expected_type {
            continue;
        }
        let Some(owner_ttl) = accepted_names.get(answer.name()).copied() else {
            continue;
        };
        let Some(ip) = answer.ip_addr() else {
            continue;
        };
        return Some(ResolvedAnswer {
            ip,
            ttl_seconds: owner_ttl.min(answer.ttl()),
            record_type: answer.rr_type(),
        });
    }

    None
}

pub(super) fn validate_response_id(response: &Message, query_id: u16) -> Result<()> {
    if response.id() == query_id {
        Ok(())
    } else {
        Err(DnsError::protocol(format!(
            "nameserver DNS response ID mismatch: expected {}, got {}",
            query_id,
            response.id()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;
    use crate::proto::RData;
    use crate::proto::rdata::{A, AAAA, CNAME};

    fn answer_response(name: &str, ttl: u32, ip: IpAddr) -> Message {
        let name = Name::from_ascii(name).expect("answer name should parse");
        let mut message = Message::new();
        let record = match ip {
            IpAddr::V4(ip) => Record::from_rdata(name, ttl, RData::A(A(ip))),
            IpAddr::V6(ip) => Record::from_rdata(name, ttl, RData::AAAA(AAAA(ip))),
        };
        message.add_answer(record);
        message
    }

    #[test]
    fn test_query_builds_ipv4_by_default() {
        let query = ResolveQuery::new("example.com.", None).expect("query should build");
        let question = query.first_question().expect("question should exist");

        assert_eq!(question.qtype(), RecordType::A);
        assert_eq!(question.name().to_fqdn(), "example.com.");
    }

    #[test]
    fn test_query_builds_ipv6_when_requested() {
        let query = ResolveQuery::new("example.com.", Some(6)).expect("query should build");
        let question = query.first_question().expect("question should exist");

        assert_eq!(question.qtype(), RecordType::AAAA);
    }

    #[test]
    fn test_query_rejects_invalid_target_domain() {
        let result = ResolveQuery::new("example..com.", None);

        assert!(result.is_err());
    }

    #[test]
    fn test_select_answer_follows_cname_and_rejects_unrelated_a() {
        let query_name = Name::from_ascii("example.com.").expect("name should parse");
        let alias_name = Name::from_ascii("alias.example.net.").expect("name should parse");
        let unrelated_name = Name::from_ascii("unrelated.example.").expect("name should parse");
        let unrelated = Record::from_rdata(
            unrelated_name,
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        );
        let cname = Record::from_rdata(
            query_name.clone(),
            30,
            RData::CNAME(CNAME(alias_name.clone())),
        );
        let target =
            Record::from_rdata(alias_name, 300, RData::A(A(Ipv4Addr::new(203, 0, 113, 53))));

        let selected = select_answer(&[unrelated, cname, target], &query_name, RecordType::A)
            .expect("answer should match");

        assert_eq!(selected.ip, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 53)));
        assert_eq!(selected.ttl_seconds, 30);
        assert_eq!(selected.record_type, RecordType::A);
    }

    #[test]
    fn test_select_answer_rejects_unrelated_a_records() {
        let query_name = Name::from_ascii("example.com.").expect("name should parse");
        let unrelated_name = Name::from_ascii("unrelated.example.").expect("name should parse");
        let unrelated = Record::from_rdata(
            unrelated_name,
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        );

        assert!(select_answer(&[unrelated], &query_name, RecordType::A).is_none());
    }

    #[test]
    fn test_select_answer_filters_unexpected_ip_family() {
        let query_name = Name::from_ascii("example.com.").expect("name should parse");
        let v4 = Record::from_rdata(
            query_name.clone(),
            300,
            RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
        );
        let v6 = Record::from_rdata(
            query_name.clone(),
            300,
            RData::AAAA(AAAA(Ipv6Addr::LOCALHOST)),
        );

        let selected =
            select_answer(&[v4, v6], &query_name, RecordType::AAAA).expect("AAAA should match");

        assert_eq!(selected.ip, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(selected.record_type, RecordType::AAAA);
    }

    #[test]
    fn test_validate_response_id_rejects_mismatch() {
        let mut response = Message::new();
        response.set_id(2);

        let err = validate_response_id(&response, 1).expect_err("ID should mismatch");

        assert!(err.to_string().contains("DNS response ID mismatch"));
    }

    #[test]
    fn test_answer_response_supports_ipv6() {
        let response = answer_response("example.com.", 60, IpAddr::V6(Ipv6Addr::LOCALHOST));

        assert_eq!(response.answers().len(), 1);
    }
}
