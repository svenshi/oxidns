// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! `has_cname` matcher plugin.
//!
//! Configured with `type: has_cname` and no arguments, or with the
//! parameterless `has_cname` quick-setup expression. Returns true when the
//! current response's answer section contains a CNAME, regardless of request
//! qtype or response rcode. It has no plugin dependencies or lifecycle
//! resources; matching short-circuits without allocations, response mutation,
//! or external side effects.

use std::fmt::Debug;

use async_trait::async_trait;

use crate::config::types::PluginConfig;
use crate::core::context::DnsContext;
use crate::infra::error::{DnsError, Result as DnsResult};
use crate::plugin::matcher::Matcher;
use crate::plugin::{Plugin, PluginFactory, UninitializedPlugin};
use crate::plugin_factory;
use crate::proto::RecordType;

#[derive(Debug, Clone)]
#[plugin_factory("has_cname")]
pub struct HasCnameFactory {}

impl PluginFactory for HasCnameFactory {
    fn create(
        &self,
        plugin_config: &PluginConfig,
        _init_context: &crate::plugin::PluginInitContext<'_>,
    ) -> DnsResult<UninitializedPlugin> {
        Ok(UninitializedPlugin::Matcher(Box::new(HasCnameMatcher {
            tag: plugin_config.tag.clone(),
        })))
    }

    fn quick_setup(&self, tag: &str, param: Option<String>) -> DnsResult<UninitializedPlugin> {
        if let Some(param) = param
            && !param.trim().is_empty()
        {
            return Err(DnsError::plugin("has_cname does not accept parameters"));
        }
        Ok(UninitializedPlugin::Matcher(Box::new(HasCnameMatcher {
            tag: tag.to_string(),
        })))
    }
}

#[derive(Debug)]
struct HasCnameMatcher {
    tag: String,
}

#[async_trait]
impl Plugin for HasCnameMatcher {
    fn tag(&self) -> &str {
        &self.tag
    }

    async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> DnsResult<()> {
        Ok(())
    }

    async fn destroy(&self) -> DnsResult<()> {
        Ok(())
    }
}

impl Matcher for HasCnameMatcher {
    #[hotpath::measure]
    fn is_match(&self, context: &mut DnsContext) -> bool {
        context
            .response()
            .is_some_and(|response| response.has_answer_type(RecordType::CNAME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::test_utils::test_context;
    use crate::proto::rdata::{A, CNAME};
    use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record};

    fn cname(owner: &str, target: &str) -> Record {
        Record::from_rdata(
            Name::from_ascii(owner).unwrap(),
            60,
            RData::CNAME(CNAME(Name::from_ascii(target).unwrap())),
        )
    }

    fn address() -> Record {
        Record::from_rdata(
            Name::from_ascii("target.example.com.").unwrap(),
            60,
            RData::A(A::new(192, 0, 2, 1)),
        )
    }

    #[test]
    fn test_has_cname_quick_setup_parameters() {
        let factory = HasCnameFactory {};
        for param in [None, Some(String::new()), Some(" \t".to_string())] {
            assert!(factory.quick_setup("has_cname", param).is_ok());
        }
        let err = factory
            .quick_setup("has_cname", Some("unexpected".to_string()))
            .err()
            .expect("has_cname quick setup should reject parameters");
        assert!(
            err.to_string()
                .contains("has_cname does not accept parameters")
        );
    }

    #[test]
    fn test_has_cname_missing_or_non_cname_response() {
        let matcher = HasCnameMatcher {
            tag: "has_cname".to_string(),
        };
        let mut ctx = test_context();
        assert!(!matcher.is_match(&mut ctx));

        ctx.set_response(Message::new());
        assert!(!matcher.is_match(&mut ctx));

        let mut response = Message::new();
        response.add_answer(address());
        ctx.set_response(response);
        assert!(!matcher.is_match(&mut ctx));
    }

    #[test]
    fn test_has_cname_matches_single_chain_and_mixed_answers() {
        let matcher = HasCnameMatcher {
            tag: "has_cname".to_string(),
        };
        let first = cname("example.com.", "alias.example.com.");
        let second = cname("alias.example.com.", "target.example.com.");
        for answers in [
            vec![first.clone()],
            vec![first.clone(), second.clone()],
            vec![address(), first, second],
        ] {
            let mut ctx = test_context();
            let mut response = Message::new();
            for answer in answers {
                response.add_answer(answer);
            }
            ctx.set_response(response);
            assert!(matcher.is_match(&mut ctx));
        }
    }

    #[test]
    fn test_has_cname_ignores_authority_and_additional_records() {
        let matcher = HasCnameMatcher {
            tag: "has_cname".to_string(),
        };
        for in_authority in [true, false] {
            let mut ctx = test_context();
            let mut response = Message::new();
            response.add_answer(address());
            let record = cname("example.com.", "target.example.com.");
            if in_authority {
                response.add_authority(record);
            } else {
                response.add_additional(record);
            }
            ctx.set_response(response);
            assert!(!matcher.is_match(&mut ctx));
        }
    }

    #[test]
    fn test_has_cname_does_not_depend_on_qtype_or_rcode() {
        let matcher = HasCnameMatcher {
            tag: "has_cname".to_string(),
        };
        for qtype in [RecordType::A, RecordType::AAAA, RecordType::CNAME] {
            for rcode in [Rcode::NoError, Rcode::NXDomain, Rcode::ServFail] {
                let mut ctx = test_context();
                ctx.request.add_question(Question::new(
                    Name::from_ascii("example.com.").unwrap(),
                    qtype,
                    DNSClass::IN,
                ));
                assert!(!matcher.is_match(&mut ctx));

                let mut response = Message::new();
                response.set_rcode(rcode);
                response.add_answer(cname("example.com.", "target.example.com."));
                ctx.set_response(response);
                assert!(matcher.is_match(&mut ctx));
            }
        }
    }
}
