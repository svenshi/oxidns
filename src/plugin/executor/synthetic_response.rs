// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for executor-owned synthetic DNS responses.

use std::sync::Arc;

use crate::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, SOA};

pub(crate) const DEFAULT_FAKE_SOA_TTL: u32 = 300;
const DEFAULT_FAKE_SOA_MINIMUM: u32 = 86_400;

lazy_static::lazy_static! {
    static ref DEFAULT_FAKE_SOA_RDATA: Arc<RData> =
        fake_soa_rdata(DEFAULT_FAKE_SOA_MINIMUM);
}

pub(crate) fn fake_soa_rdata(minimum: u32) -> Arc<RData> {
    Arc::new(RData::SOA(SOA::new(
        Name::from_ascii("fake-ns.oxidns.fake.root.").expect("fake SOA mname should parse"),
        Name::from_ascii("fake-mbox.oxidns.fake.root.").expect("fake SOA rname should parse"),
        2021110400,
        1800,
        900,
        604800,
        minimum,
    )))
}

pub(crate) fn default_nxdomain_response(request: &Message, question: &Question) -> Message {
    let mut response = request.response(Rcode::NXDomain);
    add_default_fake_soa_authority(&mut response, question);
    response
}

pub(crate) fn default_nodata_response(request: &Message, question: &Question) -> Message {
    let mut response = request.response(Rcode::NoError);
    add_default_fake_soa_authority(&mut response, question);
    response
}

pub(crate) fn add_default_fake_soa_authority(response: &mut Message, question: &Question) {
    add_fake_soa_authority(
        response,
        question,
        DEFAULT_FAKE_SOA_TTL,
        DNSClass::IN,
        DEFAULT_FAKE_SOA_RDATA.clone(),
    );
}

pub(crate) fn add_fake_soa_authority(
    response: &mut Message,
    question: &Question,
    ttl: u32,
    class: DNSClass,
    rdata: Arc<RData>,
) {
    response.add_authority(Record::from_arc_rdata_with_class(
        question.name().clone(),
        ttl,
        class,
        rdata,
    ));
}
