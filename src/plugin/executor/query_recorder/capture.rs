// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use super::model::{
    EdnsJson, EdnsOptionJson, PendingRecord, QuestionJson, RecordJson, RecordRow, StepJson,
};
use crate::core::context::{ExecutionPath, ExecutionPathEvent};
use crate::plugin::executor::rdata_json::{RDataPayloadMode, rdata_payload};
use crate::proto::rdata::{ClientSubnet, Edns, EdnsCode, EdnsExtendedDnsError, EdnsOption};
use crate::proto::{DNSClass, Message, Opcode, Question, Rcode, Record, RecordType};

impl PendingRecord {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        request: Message,
        response: Option<Message>,
        created_at_ms: i64,
        elapsed_ms: u64,
        exec_path: ExecutionPath,
        step_start_index: usize,
        client_ip: SocketAddr,
        error: Option<String>,
    ) -> Self {
        Self {
            request,
            response,
            created_at_ms,
            elapsed_ms,
            exec_path,
            step_start_index,
            client_ip,
            error,
        }
    }

    pub(super) fn take_to_record(self) -> (RecordRow, Vec<StepJson>) {
        let PendingRecord {
            request,
            response,
            created_at_ms,
            elapsed_ms,
            exec_path,
            step_start_index,
            client_ip,
            error,
        } = self;

        let questions_json = request
            .questions()
            .iter()
            .map(question_json)
            .collect::<Vec<_>>();
        let req_edns_json = request.edns().as_ref().map(edns_json);
        let steps = exec_path
            .events_from(step_start_index)
            .iter()
            .enumerate()
            .map(step_json)
            .collect::<Vec<_>>();

        let no_err = error.is_none();

        let mut record = RecordRow {
            id: 0,
            created_at_ms,
            elapsed_ms,
            request_id: request.id(),
            client_ip: client_ip.ip().to_string(),
            questions_json,
            req_rd: request.recursion_desired(),
            req_cd: request.checking_disabled(),
            req_ad: request.authentic_data(),
            req_opcode: opcode_name(request.opcode()),
            req_edns_json,
            error,
            has_response: false,
            rcode: None,
            resp_aa: None,
            resp_tc: None,
            resp_ra: None,
            resp_ad: None,
            resp_cd: None,
            answer_count: 0,
            authority_count: 0,
            additional_count: 0,
            answers_json: Vec::new(),
            authorities_json: Vec::new(),
            additionals_json: Vec::new(),
            signature_json: Vec::new(),
            resp_edns_json: None,
        };

        if no_err && let Some(response) = response {
            record.has_response = true;
            record.rcode = Some(rcode_name(response.rcode()));
            record.resp_aa = Some(response.authoritative());
            record.resp_tc = Some(response.truncated());
            record.resp_ra = Some(response.recursion_available());
            record.resp_ad = Some(response.authentic_data());
            record.resp_cd = Some(response.checking_disabled());
            record.answer_count = response.answers().len() as u32;
            record.authority_count = response.authorities().len() as u32;
            record.additional_count = response.additionals().len() as u32;
            record.answers_json = response
                .answers()
                .iter()
                .map(record_json)
                .collect::<Vec<_>>();
            record.authorities_json = response
                .authorities()
                .iter()
                .map(record_json)
                .collect::<Vec<_>>();
            record.additionals_json = response
                .additionals()
                .iter()
                .map(record_json)
                .collect::<Vec<_>>();
            record.signature_json = response
                .signature()
                .iter()
                .map(record_json)
                .collect::<Vec<_>>();
            record.resp_edns_json = response.edns().as_ref().map(edns_json);
        }

        (record, steps)
    }
}

fn question_json(question: &Question) -> QuestionJson {
    QuestionJson {
        name: question.name().to_fqdn(),
        qtype: record_type_name(question.qtype()),
        qclass: dns_class_name(question.qclass()),
    }
}

fn step_json((event_index, event): (usize, &Arc<ExecutionPathEvent>)) -> StepJson {
    StepJson {
        event_index,
        sequence_tag: event.sequence_tag.clone(),
        node_index: event.node_index,
        kind: event.kind.clone(),
        tag: event.tag.clone(),
        outcome: event.outcome.clone(),
    }
}

fn record_json(record: &Record) -> RecordJson {
    let (payload_kind, payload_text, payload) =
        rdata_payload(record.data(), RDataPayloadMode::Recorder);
    RecordJson {
        name: record.name().to_fqdn(),
        class: dns_class_name(record.class()),
        ttl: record.ttl(),
        rr_type: record_type_name(record.rr_type()),
        payload_kind,
        payload_text,
        payload,
    }
}

fn edns_json(edns: &Edns) -> EdnsJson {
    EdnsJson {
        udp_payload_size: edns.udp_payload_size(),
        ext_rcode: edns.ext_rcode(),
        version: edns.version(),
        dnssec_ok: edns.flags().dnssec_ok,
        z: edns.flags().z,
        options: edns.options().iter().map(edns_option_json).collect(),
    }
}

fn edns_option_json(option: &EdnsOption) -> EdnsOptionJson {
    let code = EdnsCode::from(option);
    let (payload_kind, payload) = match option {
        EdnsOption::Llq(value) => (
            "Llq".to_string(),
            json!({
                "version": value.version(),
                "opcode": value.opcode(),
                "error": value.error(),
                "id": value.id(),
                "lease_life": value.lease_life(),
            }),
        ),
        EdnsOption::UpdateLease(value) => (
            "UpdateLease".to_string(),
            json!({
                "lease": value.lease(),
                "key_lease": value.key_lease(),
            }),
        ),
        EdnsOption::Nsid(value) => (
            "Nsid".to_string(),
            json!({ "nsid_base64": STANDARD.encode(value.nsid()) }),
        ),
        EdnsOption::Esu(value) => (
            "Esu".to_string(),
            utf8_or_base64_payload("uri", value.uri()),
        ),
        EdnsOption::Dau(value) => (
            "Dau".to_string(),
            json!({ "algorithms": value.algorithms() }),
        ),
        EdnsOption::Dhu(value) => (
            "Dhu".to_string(),
            json!({ "algorithms": value.algorithms() }),
        ),
        EdnsOption::N3u(value) => (
            "N3u".to_string(),
            json!({ "algorithms": value.algorithms() }),
        ),
        EdnsOption::Subnet(value) => ("Subnet".to_string(), client_subnet_json(value)),
        EdnsOption::Expire(value) => (
            "Expire".to_string(),
            json!({
                "empty": value.is_empty(),
                "expire": (!value.is_empty()).then_some(value.expire()),
            }),
        ),
        EdnsOption::Cookie(value) => (
            "Cookie".to_string(),
            json!({ "cookie_base64": STANDARD.encode(value.cookie()) }),
        ),
        EdnsOption::TcpKeepalive(value) => (
            "TcpKeepalive".to_string(),
            json!({ "timeout": value.timeout() }),
        ),
        EdnsOption::Padding(value) => (
            "Padding".to_string(),
            json!({ "padding_base64": STANDARD.encode(value.padding()) }),
        ),
        EdnsOption::ExtendedDnsError(value) => (
            "ExtendedDnsError".to_string(),
            extended_dns_error_json(value),
        ),
        EdnsOption::ReportChannel(value) => (
            "ReportChannel".to_string(),
            json!({ "agent_domain": value.agent_domain().to_fqdn() }),
        ),
        EdnsOption::ZoneVersion(value) => (
            "ZoneVersion".to_string(),
            json!({
                "label_count": value.label_count(),
                "version_type": value.version_type(),
                "version_base64": STANDARD.encode(value.version()),
            }),
        ),
        EdnsOption::Local(value) => (
            "Local".to_string(),
            json!({
                "code": value.code(),
                "data_base64": STANDARD.encode(value.data()),
            }),
        ),
    };

    EdnsOptionJson {
        code: u16::from(code),
        name: edns_code_name(code),
        payload_kind,
        payload,
    }
}

fn client_subnet_json(value: &ClientSubnet) -> Value {
    json!({
        "addr": value.addr().to_string(),
        "source_prefix": value.source_prefix(),
        "scope_prefix": value.scope_prefix(),
    })
}

fn extended_dns_error_json(value: &EdnsExtendedDnsError) -> Value {
    let text = std::str::from_utf8(value.extra_text())
        .ok()
        .map(str::to_string);
    json!({
        "info_code": value.info_code(),
        "extra_text": text,
        "extra_text_base64": text.is_none().then(|| STANDARD.encode(value.extra_text())),
    })
}

fn utf8_or_base64_payload(field: &str, bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => json!({ field: text }),
        Err(_) => {
            let mut map = serde_json::Map::new();
            map.insert(
                format!("{field}_base64"),
                Value::String(STANDARD.encode(bytes)),
            );
            Value::Object(map)
        }
    }
}

fn opcode_name(opcode: Opcode) -> String {
    opcode.to_string()
}

fn rcode_name(rcode: Rcode) -> String {
    match rcode {
        Rcode::Unknown(code) => format!("RCODE{code}"),
        _ => rcode.to_string(),
    }
}

fn dns_class_name(class: DNSClass) -> String {
    match class {
        DNSClass::Unknown(value) => format!("CLASS{value}"),
        DNSClass::OPT(value) => format!("OPT({value})"),
        _ => class.to_string(),
    }
}

fn record_type_name(record_type: RecordType) -> String {
    match record_type {
        RecordType::Unknown(value) => format!("TYPE{value}"),
        _ => record_type.to_string(),
    }
}

fn edns_code_name(code: EdnsCode) -> String {
    match code {
        EdnsCode::Reserved => "Reserved".to_string(),
        EdnsCode::Llq => "Llq".to_string(),
        EdnsCode::UpdateLease => "UpdateLease".to_string(),
        EdnsCode::Nsid => "Nsid".to_string(),
        EdnsCode::Esu => "Esu".to_string(),
        EdnsCode::Dau => "Dau".to_string(),
        EdnsCode::Dhu => "Dhu".to_string(),
        EdnsCode::N3u => "N3u".to_string(),
        EdnsCode::Subnet => "Subnet".to_string(),
        EdnsCode::Expire => "Expire".to_string(),
        EdnsCode::Cookie => "Cookie".to_string(),
        EdnsCode::TcpKeepalive => "TcpKeepalive".to_string(),
        EdnsCode::Padding => "Padding".to_string(),
        EdnsCode::Chain => "Chain".to_string(),
        EdnsCode::KeyTag => "KeyTag".to_string(),
        EdnsCode::ExtendedDnsError => "ExtendedDnsError".to_string(),
        EdnsCode::ClientTag => "ClientTag".to_string(),
        EdnsCode::ServerTag => "ServerTag".to_string(),
        EdnsCode::ReportChannel => "ReportChannel".to_string(),
        EdnsCode::ZoneVersion => "ZoneVersion".to_string(),
        EdnsCode::Unknown(value) => format!("Unknown({value})"),
    }
}
