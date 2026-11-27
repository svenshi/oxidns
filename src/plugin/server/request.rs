// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared DNS request lifecycle for server plugins.

use std::net::SocketAddr;
use std::sync::Arc;

use tracing::{Level, debug, event_enabled, warn};

use super::metrics::ServerMetrics;
pub use crate::core::context::RequestMeta;
use crate::core::context::{DnsContext, OriginalQuery};
use crate::infra::network::ip::normalize_ipv4_mapped_socket_addr;
use crate::plugin::executor::{ExecStep, Executor};
use crate::proto::{Edns, Message, MessageType, Opcode, Question, Rcode};

#[derive(Debug)]
pub struct RequestHandle {
    pub entry_executor: Arc<dyn Executor>,
    /// Shared server metrics. `None` for internal/test handles that should not
    /// emit server-level metrics.
    pub(crate) metrics: Option<Arc<ServerMetrics>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RequestExit {
    Completed,
    Controlled,
    Failed,
}

#[derive(Debug)]
#[allow(unused)]
pub struct RequestResult {
    pub request: Message,
    pub response: Message,
    pub exit: RequestExit,
}

impl RequestHandle {
    /// Build an internal request handler without server-level metrics.
    pub fn new(entry_executor: Arc<dyn Executor>) -> Self {
        Self {
            entry_executor,
            metrics: None,
        }
    }

    #[hotpath::measure]
    pub async fn handle_request(
        &self,
        msg: Message,
        src_addr: SocketAddr,
        meta: RequestMeta,
    ) -> RequestResult {
        let metrics_start = self.metrics.as_ref().map(|m| m.on_request_start());

        let mut context = DnsContext::new(normalize_ipv4_mapped_socket_addr(src_addr), msg);
        self.apply_request_meta(&mut context, meta);

        // Invalid ingress must not execute the pipeline. An unsupported opcode
        // needs NOTIMP (RFC 8906 3.1.4), while unsupported EDNS versions need
        // BADVERS (RFC 6891 6.1.3).
        let ingress_error = if context.request.message_type() != MessageType::Query {
            Some(Rcode::Refused)
        } else if context.request.opcode() != Opcode::Query {
            Some(Rcode::NotImp)
        } else if context
            .request
            .edns()
            .as_ref()
            .is_some_and(|edns| edns.version() > 0)
        {
            Some(Rcode::BADVERS)
        } else {
            None
        };

        if event_enabled!(Level::DEBUG) {
            debug!(
                "DNS request from {}, queries: {:?}, id: {}, edns: {:?}, nameservers: {:?}",
                &src_addr,
                context.request.questions(),
                context.request.id(),
                context.request.edns(),
                context.request.authorities()
            );
        }

        let (mut response, exit) = if let Some(rcode) = ingress_error {
            (
                self.build_base_response(&context, rcode),
                RequestExit::Controlled,
            )
        } else {
            match self
                .entry_executor
                .execute_with_next(&mut context, None)
                .await
            {
                Ok(step) => {
                    let exit = match step {
                        ExecStep::Next => RequestExit::Completed,
                        ExecStep::Stop | ExecStep::Return => RequestExit::Controlled,
                    };
                    let response = context
                        .take_response()
                        .unwrap_or_else(|| self.build_empty_response(&context));
                    (response, exit)
                }
                Err(e) => {
                    warn!(
                        "Entry executor '{}' failed for source {} id {}: {}",
                        self.entry_executor.tag(),
                        src_addr,
                        context.request.id(),
                        e
                    );
                    (self.build_servfail_response(&context), RequestExit::Failed)
                }
            }
        };

        Self::finalize_response(context.original_query(), &mut response);

        if event_enabled!(Level::DEBUG) {
            debug!(
                "Sending response to {}, exit: {:?}, queries: {:?}, id: {}, edns: {:?}, answers: {:?}",
                &src_addr,
                exit,
                context.request.questions(),
                response.id(),
                response.edns(),
                response.answers()
            );
        }

        if let (Some(metrics), Some(start_ms)) = (self.metrics.as_ref(), metrics_start) {
            metrics.on_request_finish(start_ms, exit);
        }

        RequestResult {
            request: context.request,
            response,
            exit,
        }
    }

    #[inline]
    fn apply_request_meta(&self, context: &mut DnsContext, meta: RequestMeta) {
        context.set_request_meta(RequestMeta {
            server_name: meta.server_name.filter(|value| !value.is_empty()),
            url_path: meta.url_path.filter(|value| !value.is_empty()),
        });
    }

    #[inline]
    fn build_servfail_response(&self, context: &DnsContext) -> Message {
        self.build_base_response(context, Rcode::ServFail)
    }

    #[inline]
    fn build_empty_response(&self, context: &DnsContext) -> Message {
        self.build_base_response(context, Rcode::NoError)
    }

    #[inline]
    fn build_base_response(&self, context: &DnsContext, rcode: Rcode) -> Message {
        // Base responses must be built from the ingress snapshot: the live
        // request may have been rewritten or replaced downstream, and a
        // mismatched id, opcode, or question would make clients drop the
        // response.
        let original = context.original_query();
        let (recursion_desired, checking_disabled) = if original.opcode() == Opcode::Query {
            (original.recursion_desired(), original.checking_disabled())
        } else {
            (false, false)
        };
        let mut response = Message::new();
        response.set_id(original.id());
        response.set_message_type(MessageType::Response);
        response.set_opcode(original.opcode());
        response.set_recursion_desired(recursion_desired);
        response.set_checking_disabled(checking_disabled);
        response.set_rcode(rcode);
        response
            .questions_mut()
            .extend(original.questions().iter().cloned());
        response
    }

    /// Apply server-level RFC fixes to every outbound response.
    ///
    /// `original` is the ingress snapshot (`context.original_query`), not the
    /// live `context.request`, which plugins may legally rewrite or replace
    /// mid-pipeline. Question echo and the EDNS DO-bit copy must both reflect
    /// what the client actually sent.
    fn finalize_response(original: &OriginalQuery, response: &mut Message) {
        response.set_recursion_available(true);

        if let Some(dnssec_ok) = original.edns_do()
            && response.edns().is_none()
        {
            let mut edns = Edns::new();
            edns.flags_mut().dnssec_ok = dnssec_ok;
            response.set_edns(edns);
        }

        Self::restore_question_echo(original.questions(), response);
    }

    /// Restore the request's original question spelling in the response.
    ///
    /// DNS names compare case-insensitively, and the cache key is normalized to
    /// lowercase, so an upstream or cached response may echo a differently
    /// cased (or missing) question. RFC 1035 4.1.2 carries the query's question
    /// section back in every response, and clients that verify 0x20 case
    /// randomization (RFC 7626) depend on the echoed case, so align it before
    /// the response leaves the server. The common case only swaps `Name`
    /// handles; a length mismatch rebuilds the question section from the
    /// request.
    fn restore_question_echo(original_questions: &[Question], response: &mut Message) {
        let response_questions = response.questions_mut();

        if response_questions.len() != original_questions.len() {
            response_questions.clear();
            response_questions.extend(original_questions.iter().cloned());
            return;
        }

        for (response_question, request_question) in
            response_questions.iter_mut().zip(original_questions)
        {
            // A question that differs from the request even case-insensitively
            // (e.g. a broken upstream answering a different name) is left
            // as-is on purpose: rewriting it would hide the mismatch while the
            // answers still cannot satisfy the query, and clients drop such a
            // response anyway.
            if response_question.name() == request_question.name()
                && response_question.qtype() == request_question.qtype()
                && response_question.qclass() == request_question.qclass()
            {
                response_question.set_name(request_question.name().clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::continue_next;
    use crate::infra::error::Result;
    use crate::plugin::Plugin;
    use crate::plugin::test_utils::encoded_question_fqdn;
    use crate::proto::rdata::A;
    use crate::proto::{Name, Question, RData, Record, RecordType};

    fn make_request(id: u16, qname: &str) -> Message {
        let mut request = Message::new();
        request.set_id(id);
        request.add_question(Question::new(
            Name::from_ascii(qname).expect("query name should be valid"),
            RecordType::A,
            crate::proto::DNSClass::IN,
        ));
        request
    }

    fn make_request_handle(executor: Arc<dyn Executor>) -> RequestHandle {
        RequestHandle::new(executor)
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct ObservedMeta {
        server_name: Option<String>,
        url_path: Option<String>,
    }

    #[derive(Debug)]
    struct CaptureMetaExecutor {
        observed: Arc<Mutex<Option<ObservedMeta>>>,
    }

    #[async_trait]
    impl Plugin for CaptureMetaExecutor {
        fn tag(&self) -> &str {
            "capture_meta"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for CaptureMetaExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            let observed = ObservedMeta {
                server_name: context.server_name().map(str::to_string),
                url_path: context.url_path().map(str::to_string),
            };
            self.observed
                .lock()
                .expect("meta capture lock should not be poisoned")
                .replace(observed);
            Ok(ExecStep::Next)
        }
    }

    #[derive(Debug)]
    struct PostResponseExecutor;

    #[async_trait]
    impl Plugin for PostResponseExecutor {
        fn tag(&self) -> &str {
            "post_response"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for PostResponseExecutor {
        fn with_next(&self) -> bool {
            true
        }

        async fn execute(&self, _context: &mut DnsContext) -> Result<ExecStep> {
            Ok(ExecStep::Next)
        }

        async fn execute_with_next(
            &self,
            context: &mut DnsContext,
            next: Option<crate::plugin::executor::ExecutorNext>,
        ) -> Result<ExecStep> {
            let step = continue_next!(next, context)?;
            context.set_response(context.request.response(Rcode::NXDomain));
            Ok(step)
        }
    }

    /// Simulates a downstream plugin that rewrites the live request question
    /// and never restores it, like an uncareful `replace_request` user, and
    /// an upstream that echoes the lowercased spelling back.
    #[derive(Debug)]
    struct RequestPollutingExecutor;

    #[async_trait]
    impl Plugin for RequestPollutingExecutor {
        fn tag(&self) -> &str {
            "request_polluting"
        }

        async fn init(&mut self, _context: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
            Ok(())
        }

        async fn destroy(&self) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Executor for RequestPollutingExecutor {
        async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
            if let Some(question) = context.request_mut().first_question_mut() {
                question.set_name(
                    Name::from_ascii(question.name().normalized())
                        .expect("normalized name should parse"),
                );
            }
            let mut response = context.request.response(Rcode::NoError);
            response.add_answer(Record::from_rdata(
                Name::from_ascii("www.example.com.").expect("answer name should parse"),
                60,
                RData::A(A(Ipv4Addr::new(192, 0, 2, 2))),
            ));
            context.set_response(response);
            Ok(ExecStep::Return)
        }
    }

    #[tokio::test]
    async fn test_handle_request_refuses_non_query_ingress() {
        let observed = Arc::new(Mutex::new(None));
        let request_handle = make_request_handle(Arc::new(CaptureMetaExecutor {
            observed: observed.clone(),
        }));
        let mut request = make_request(21, "example.com.");
        request.set_message_type(crate::proto::MessageType::Response);

        let result = request_handle
            .handle_request(
                request,
                SocketAddr::from(([127, 0, 0, 1], 5303)),
                RequestMeta {
                    server_name: None,
                    url_path: None,
                },
            )
            .await;

        assert_eq!(result.response.rcode(), Rcode::Refused);
        assert_eq!(result.exit, RequestExit::Controlled);
        assert!(
            observed
                .lock()
                .expect("meta capture lock should not be poisoned")
                .is_none(),
            "pipeline must not run for non-query ingress"
        );
    }

    #[tokio::test]
    async fn test_handle_request_answers_notimp_without_running_pipeline() {
        let observed = Arc::new(Mutex::new(None));
        let request_handle = make_request_handle(Arc::new(CaptureMetaExecutor {
            observed: observed.clone(),
        }));
        for opcode in [
            Opcode::Status,
            Opcode::Notify,
            Opcode::Update,
            Opcode::Unknown(15),
        ] {
            let mut request = make_request(23, "example.com.");
            request.set_opcode(opcode);
            let result = request_handle
                .handle_request(
                    request,
                    SocketAddr::from(([127, 0, 0, 1], 5303)),
                    RequestMeta {
                        server_name: None,
                        url_path: None,
                    },
                )
                .await;
            let response = Message::from_bytes(&result.response.to_bytes().unwrap()).unwrap();
            assert_eq!(response.rcode(), Rcode::NotImp);
            assert_eq!(response.opcode(), opcode);
            assert_eq!(response.id(), 23);
            assert_eq!(response.message_type(), MessageType::Response);
            assert_eq!(response.questions(), result.request.questions());
            assert_eq!(result.exit, RequestExit::Controlled);
        }
        assert!(
            observed.lock().unwrap().is_none(),
            "pipeline must not run for unsupported opcodes"
        );
    }

    #[tokio::test]
    async fn test_handle_request_answers_badvers_for_unsupported_edns_version() {
        let observed = Arc::new(Mutex::new(None));
        let request_handle = make_request_handle(Arc::new(CaptureMetaExecutor {
            observed: observed.clone(),
        }));
        let mut request = make_request(22, "example.com.");
        request.set_edns(crate::proto::rdata::Edns::new_with_param(
            1232,
            0,
            1,
            Default::default(),
            Vec::new(),
        ));

        let result = request_handle
            .handle_request(
                request,
                SocketAddr::from(([127, 0, 0, 1], 5303)),
                RequestMeta {
                    server_name: None,
                    url_path: None,
                },
            )
            .await;

        assert_eq!(result.response.rcode(), Rcode::BADVERS);
        assert!(result.response.edns().is_some(), "BADVERS requires an OPT");
        let response = Message::from_bytes(&result.response.to_bytes().unwrap()).unwrap();
        assert_eq!(response.rcode(), Rcode::BADVERS);
        assert_eq!(response.edns().as_ref().unwrap().version(), 0);
        assert!(
            observed
                .lock()
                .expect("meta capture lock should not be poisoned")
                .is_none(),
            "pipeline must not run for unsupported EDNS versions"
        );
    }

    #[tokio::test]
    async fn test_handle_request_with_meta_applies_server_name_and_url_path() {
        let observed = Arc::new(Mutex::new(None));
        let request_handle = make_request_handle(Arc::new(CaptureMetaExecutor {
            observed: observed.clone(),
        }));
        let request = make_request(13, "example.com.");

        let _result = request_handle
            .handle_request(
                request,
                SocketAddr::from(([127, 0, 0, 1], 5303)),
                RequestMeta {
                    server_name: Some(Arc::from("dns.example.test")),
                    url_path: Some(Arc::from("/dns-query")),
                },
            )
            .await;

        assert_eq!(
            observed
                .lock()
                .expect("meta capture lock should not be poisoned")
                .clone(),
            Some(ObservedMeta {
                server_name: Some("dns.example.test".to_string()),
                url_path: Some("/dns-query".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn test_handle_request_supports_with_next_entry_executor() {
        let request_handle = make_request_handle(Arc::new(PostResponseExecutor));
        let request = make_request(21, "example.com.");

        let result = request_handle
            .handle_request(
                request,
                SocketAddr::from(([127, 0, 0, 1], 5303)),
                RequestMeta::default(),
            )
            .await;

        assert_eq!(result.response.rcode(), Rcode::NXDomain);
        assert_eq!(result.exit, RequestExit::Completed);
    }

    #[test]
    fn finalize_response_rebuilds_missing_question_section() {
        let request = make_request(33, "WWW.Example.COM.");

        let mut response = Message::new();
        response.set_id(33);
        RequestHandle::finalize_response(&OriginalQuery::capture(&request), &mut response);

        assert!(response.recursion_available());
        assert_eq!(encoded_question_fqdn(&response), "WWW.Example.COM.");
    }

    #[tokio::test]
    async fn test_handle_request_echoes_case_after_downstream_request_rewrite() {
        let request_handle = make_request_handle(Arc::new(RequestPollutingExecutor));
        let request = make_request(35, "WWW.Example.COM.");

        let result = request_handle
            .handle_request(
                request,
                SocketAddr::from(([127, 0, 0, 1], 5304)),
                RequestMeta::default(),
            )
            .await;

        assert_eq!(
            result.response.answer_ips(),
            vec![IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2))]
        );
        assert_eq!(
            encoded_question_fqdn(&result.response),
            "WWW.Example.COM.",
            "the echo must come from the ingress snapshot, not the polluted live request"
        );
    }
}
