// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared RDATA presentation helpers for executor management APIs.

use std::net::IpAddr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::proto::rdata::{
    self, CAA, DNSKEY, DS, NSEC, NSEC3, NSEC3PARAM, RRSIG, SOA, SSHFP, SVCB, TLSA, TXT, URI,
};
use crate::proto::{Name, RData, RecordType};

#[derive(Debug, Clone, Copy)]
pub(crate) enum RDataPayloadMode {
    Cache,
    Recorder,
}

impl RDataPayloadMode {
    #[inline]
    fn is_recorder(self) -> bool {
        matches!(self, Self::Recorder)
    }
}

type Payload = (String, String, Value);

pub(crate) fn rdata_payload(rdata: &RData, mode: RDataPayloadMode) -> (String, String, Value) {
    if let Some(payload) = shared_payload(rdata, mode) {
        return payload;
    }
    if mode.is_recorder() {
        if let Some(payload) = recorder_name_payload(rdata) {
            return payload;
        }
        if let Some(payload) = recorder_metadata_payload(rdata) {
            return payload;
        }
        if let Some(payload) = recorder_dnssec_payload(rdata) {
            return payload;
        }
    }

    fallback_payload(rdata)
}

fn shared_payload(rdata: &RData, mode: RDataPayloadMode) -> Option<Payload> {
    match rdata {
        RData::A(value) => ip_payload("A", IpAddr::V4(value.0)),
        RData::AAAA(value) => ip_payload("AAAA", IpAddr::V6(value.0)),
        RData::CNAME(value) => name_payload("CNAME", "target", &value.0),
        RData::NS(value) => name_payload("NS", "target", &value.0),
        RData::PTR(value) => name_payload("PTR", "target", &value.0),
        RData::DNAME(value) => name_payload("DNAME", "target", &value.0),
        RData::MX(value) => (
            "MX".to_string(),
            format!("{} {}", value.preference(), value.exchange().to_fqdn()),
            json!({
                "preference": value.preference(),
                "exchange": value.exchange().to_fqdn(),
            }),
        ),
        RData::SRV(value) => (
            "SRV".to_string(),
            format!(
                "{} {} {} {}",
                value.priority(),
                value.weight(),
                value.port(),
                value.target().to_fqdn()
            ),
            json!({
                "priority": value.priority(),
                "weight": value.weight(),
                "port": value.port(),
                "target": value.target().to_fqdn(),
            }),
        ),
        RData::SOA(value) => soa_payload(value),
        RData::TXT(value) => txt_payload("TXT", value),
        RData::SVCB(value) => svcb_payload("SVCB", value, mode),
        RData::HTTPS(value) => svcb_payload("HTTPS", &value.0, mode),
        RData::NULL(value) => (
            "NULL".to_string(),
            "NULL".to_string(),
            json!({ "data_base64": STANDARD.encode(value.data()) }),
        ),
        RData::OPT(_) if matches!(mode, RDataPayloadMode::Recorder) => {
            ("OPT".to_string(), "OPT".to_string(), json!({}))
        }
        RData::Unknown { rr_type, data } => (
            format!("TYPE{rr_type}"),
            format!("TYPE{rr_type}"),
            json!({
                "unknown_rr_type": rr_type,
                "data_base64": STANDARD.encode(data),
            }),
        ),
        _ => return None,
    }
    .into()
}

fn recorder_name_payload(rdata: &RData) -> Option<Payload> {
    match rdata {
        RData::MD(value) => name_payload("MD", "target", &value.0),
        RData::MF(value) => name_payload("MF", "target", &value.0),
        RData::MB(value) => name_payload("MB", "target", &value.0),
        RData::MG(value) => name_payload("MG", "target", &value.0),
        RData::MR(value) => name_payload("MR", "target", &value.0),
        RData::ANAME(value) => name_payload("ANAME", "target", &value.0),
        RData::NSAPPTR(value) => name_payload("NSAPPTR", "target", &value.0),
        _ => return None,
    }
    .into()
}

fn recorder_metadata_payload(rdata: &RData) -> Option<Payload> {
    match rdata {
        RData::KX(value) => (
            "KX".to_string(),
            format!("{} {}", value.preference(), value.exchanger().to_fqdn()),
            json!({
                "preference": value.preference(),
                "exchange": value.exchanger().to_fqdn(),
            }),
        ),
        RData::SPF(value) => txt_payload("SPF", &value.0),
        RData::CAA(value) => caa_payload(value),
        RData::URI(value) => uri_payload(value),
        _ => return None,
    }
    .into()
}

fn recorder_dnssec_payload(rdata: &RData) -> Option<Payload> {
    if let Some(payload) = recorder_signature_payload(rdata) {
        return Some(payload);
    }
    if let Some(payload) = recorder_key_payload(rdata) {
        return Some(payload);
    }
    if let Some(payload) = recorder_digest_payload(rdata) {
        return Some(payload);
    }

    match rdata {
        RData::TLSA(value) => tlsa_payload("TLSA", value),
        RData::SMIMEA(value) => tlsa_payload("SMIMEA", &value.0),
        RData::SSHFP(value) => sshfp_payload(value),
        RData::OPENPGPKEY(value) => (
            "OPENPGPKEY".to_string(),
            "OPENPGPKEY".to_string(),
            json!({ "public_key_base64": STANDARD.encode(&value.0) }),
        ),
        _ => return None,
    }
    .into()
}

fn recorder_signature_payload(rdata: &RData) -> Option<Payload> {
    match rdata {
        RData::RRSIG(value) => rrsig_payload("RRSIG", value),
        RData::SIG(value) => rrsig_payload("SIG", &value.0),
        RData::NSEC(value) => nsec_payload(value),
        RData::NSEC3(value) => nsec3_payload(value),
        RData::NSEC3PARAM(value) => nsec3param_payload(value),
        _ => return None,
    }
    .into()
}

fn recorder_key_payload(rdata: &RData) -> Option<Payload> {
    match rdata {
        RData::DNSKEY(value) => dnskey_payload("DNSKEY", value),
        RData::CDNSKEY(value) => dnskey_payload("CDNSKEY", &value.0),
        _ => return None,
    }
    .into()
}

fn recorder_digest_payload(rdata: &RData) -> Option<Payload> {
    match rdata {
        RData::DS(value) => ds_payload("DS", value),
        RData::CDS(value) => ds_payload("CDS", &value.0),
        RData::DLV(value) => ds_payload("DLV", &value.0),
        RData::TA(value) => ds_payload("TA", &value.0),
        _ => return None,
    }
    .into()
}

fn fallback_payload(rdata: &RData) -> Payload {
    (
        record_type_name(rdata.rr_type()),
        format!("{rdata:?}"),
        json!({ "display": format!("{rdata:?}") }),
    )
}

fn ip_payload(kind: &str, ip: IpAddr) -> (String, String, Value) {
    let ip = ip.to_string();
    (kind.to_string(), ip.clone(), json!({ "ip": ip }))
}

fn name_payload(kind: &str, field: &str, name: &Name) -> (String, String, Value) {
    let target = name.to_fqdn();
    (kind.to_string(), target.clone(), json!({ field: target }))
}

fn soa_payload(value: &SOA) -> (String, String, Value) {
    (
        "SOA".to_string(),
        format!("{} {}", value.mname().to_fqdn(), value.rname().to_fqdn()),
        json!({
            "mname": value.mname().to_fqdn(),
            "rname": value.rname().to_fqdn(),
            "serial": value.serial(),
            "refresh": value.refresh(),
            "retry": value.retry(),
            "expire": value.expire(),
            "minimum": value.minimum(),
        }),
    )
}

fn txt_payload(kind: &str, value: &TXT) -> (String, String, Value) {
    let mut strings = Vec::new();
    let mut parts = Vec::new();
    let mut all_utf8 = true;
    for part in value.txt_data() {
        match std::str::from_utf8(part) {
            Ok(text) => {
                strings.push(text.to_string());
                parts.push(json!({ "text": text }));
            }
            Err(_) => {
                all_utf8 = false;
                let encoded = STANDARD.encode(part);
                parts.push(json!({ "data_base64": encoded }));
            }
        }
    }

    let payload = if all_utf8 {
        json!({ "strings": strings })
    } else {
        json!({ "parts": parts })
    };

    let payload_text = if strings.is_empty() {
        kind.to_string()
    } else {
        strings.join(" ")
    };

    (kind.to_string(), payload_text, payload)
}

fn caa_payload(value: &CAA) -> (String, String, Value) {
    let tag = bytes_to_text_or_base64(value.tag());
    let caa_value = bytes_to_text_or_base64(value.value());
    (
        "CAA".to_string(),
        format!("{} {}", tag.text, caa_value.text),
        json!({
            "flag": value.flag(),
            "tag": tag.text_value,
            "tag_base64": tag.base64_value,
            "value": caa_value.text_value,
            "value_base64": caa_value.base64_value,
        }),
    )
}

fn uri_payload(value: &URI) -> (String, String, Value) {
    let target = bytes_to_text_or_base64(value.target());
    (
        "URI".to_string(),
        target.text.clone(),
        json!({
            "priority": value.priority(),
            "weight": value.weight(),
            "target": target.text_value,
            "target_base64": target.base64_value,
        }),
    )
}

fn svcb_payload(kind: &str, value: &SVCB, mode: RDataPayloadMode) -> (String, String, Value) {
    let params = match mode {
        RDataPayloadMode::Cache => json!(value.params().len()),
        RDataPayloadMode::Recorder => json!(
            value
                .params()
                .iter()
                .map(|param| {
                    json!({
                        "key": param.key(),
                        "name": svcb_param_name(param.key()),
                        "value_base64": STANDARD.encode(param.value()),
                        "parsed": svcb_param_value_json(param.parsed()),
                    })
                })
                .collect::<Vec<_>>()
        ),
    };

    (
        kind.to_string(),
        value.target().to_fqdn(),
        json!({
            "priority": value.priority(),
            "target": value.target().to_fqdn(),
            "params": params,
        }),
    )
}

fn rrsig_payload(kind: &str, value: &RRSIG) -> (String, String, Value) {
    (
        kind.to_string(),
        value.signer_name().to_fqdn(),
        json!({
            "type_covered": format_record_type_from_u16(value.type_covered()),
            "algorithm": value.algorithm(),
            "labels": value.labels(),
            "orig_ttl": value.orig_ttl(),
            "expiration": value.expiration(),
            "inception": value.inception(),
            "key_tag": value.key_tag(),
            "signer_name": value.signer_name().to_fqdn(),
            "signature_base64": STANDARD.encode(value.signature()),
        }),
    )
}

fn nsec_payload(value: &NSEC) -> (String, String, Value) {
    (
        "NSEC".to_string(),
        value.next_domain().to_fqdn(),
        json!({
            "next_domain": value.next_domain().to_fqdn(),
            "type_bitmap": value.type_bitmap_types().iter().map(|ty| record_type_name(*ty)).collect::<Vec<_>>(),
            "type_bitmap_base64": STANDARD.encode(value.type_bitmap()),
        }),
    )
}

fn nsec3_payload(value: &NSEC3) -> (String, String, Value) {
    (
        "NSEC3".to_string(),
        "NSEC3".to_string(),
        json!({
            "hash": value.hash(),
            "flags": value.flags(),
            "iterations": value.iterations(),
            "salt_base64": STANDARD.encode(value.salt()),
            "next_domain_base64": STANDARD.encode(value.next_domain()),
            "type_bitmap": value.type_bitmap_types().iter().map(|ty| record_type_name(*ty)).collect::<Vec<_>>(),
            "type_bitmap_base64": STANDARD.encode(value.type_bitmap()),
        }),
    )
}

fn nsec3param_payload(value: &NSEC3PARAM) -> (String, String, Value) {
    (
        "NSEC3PARAM".to_string(),
        "NSEC3PARAM".to_string(),
        json!({
            "hash": value.hash(),
            "flags": value.flags(),
            "iterations": value.iterations(),
            "salt_base64": STANDARD.encode(value.salt()),
        }),
    )
}

fn dnskey_payload(kind: &str, value: &DNSKEY) -> (String, String, Value) {
    (
        kind.to_string(),
        kind.to_string(),
        json!({
            "flags": value.flags(),
            "protocol": value.protocol(),
            "algorithm": value.algorithm(),
            "public_key_base64": STANDARD.encode(value.public_key()),
        }),
    )
}

fn ds_payload(kind: &str, value: &DS) -> (String, String, Value) {
    (
        kind.to_string(),
        kind.to_string(),
        json!({
            "key_tag": value.key_tag(),
            "algorithm": value.algorithm(),
            "digest_type": value.digest_type(),
            "digest_base64": STANDARD.encode(value.digest()),
        }),
    )
}

fn tlsa_payload(kind: &str, value: &TLSA) -> (String, String, Value) {
    (
        kind.to_string(),
        kind.to_string(),
        json!({
            "usage": value.usage(),
            "selector": value.selector(),
            "matching_type": value.matching_type(),
            "certificate_base64": STANDARD.encode(value.certificate()),
        }),
    )
}

fn sshfp_payload(value: &SSHFP) -> (String, String, Value) {
    (
        "SSHFP".to_string(),
        "SSHFP".to_string(),
        json!({
            "algorithm": value.algorithm(),
            "fp_type": value.fp_type(),
            "fingerprint_base64": STANDARD.encode(value.fingerprint()),
        }),
    )
}

#[derive(Debug)]
struct TextOrBase64 {
    text: String,
    text_value: Option<String>,
    base64_value: Option<String>,
}

fn bytes_to_text_or_base64(bytes: &[u8]) -> TextOrBase64 {
    match std::str::from_utf8(bytes) {
        Ok(text) => TextOrBase64 {
            text: text.to_string(),
            text_value: Some(text.to_string()),
            base64_value: None,
        },
        Err(_) => {
            let encoded = STANDARD.encode(bytes);
            TextOrBase64 {
                text: encoded.clone(),
                text_value: None,
                base64_value: Some(encoded),
            }
        }
    }
}

fn svcb_param_value_json(value: &rdata::SvcParamValue) -> Value {
    match value {
        rdata::SvcParamValue::Mandatory(values) => json!({ "mandatory": values }),
        rdata::SvcParamValue::Alpn(values) => json!({
            "alpn": values
                .iter()
                .map(|value| std::str::from_utf8(value).ok().map(str::to_string).unwrap_or_else(|| STANDARD.encode(value)))
                .collect::<Vec<_>>()
        }),
        rdata::SvcParamValue::NoDefaultAlpn => json!({ "no_default_alpn": true }),
        rdata::SvcParamValue::Port(port) => json!({ "port": port }),
        rdata::SvcParamValue::Ipv4Hint(values) => json!({
            "ipv4_hint": values.iter().map(ToString::to_string).collect::<Vec<_>>()
        }),
        rdata::SvcParamValue::Ech(value) => json!({ "ech_base64": STANDARD.encode(value) }),
        rdata::SvcParamValue::Ipv6Hint(values) => json!({
            "ipv6_hint": values.iter().map(ToString::to_string).collect::<Vec<_>>()
        }),
        rdata::SvcParamValue::DohPath(value) => match std::str::from_utf8(value) {
            Ok(text) => json!({ "doh_path": text }),
            Err(_) => json!({ "doh_path_base64": STANDARD.encode(value) }),
        },
        rdata::SvcParamValue::Ohttp => json!({ "ohttp": true }),
        rdata::SvcParamValue::Unknown => json!({ "unknown": true }),
    }
}

fn format_record_type_from_u16(value: u16) -> String {
    record_type_name(RecordType::from(value))
}

fn record_type_name(record_type: RecordType) -> String {
    match record_type {
        RecordType::Unknown(value) => format!("TYPE{value}"),
        _ => record_type.to_string(),
    }
}

fn svcb_param_name(key: u16) -> &'static str {
    match key {
        0 => "mandatory",
        1 => "alpn",
        2 => "no-default-alpn",
        3 => "port",
        4 => "ipv4hint",
        5 => "ech",
        6 => "ipv6hint",
        7 => "dohpath",
        8 => "ohttp",
        _ => "unknown",
    }
}
