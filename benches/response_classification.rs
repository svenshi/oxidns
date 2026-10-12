// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::hint::black_box;
use std::net::Ipv4Addr;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxidns::core::response::classify_response;
use oxidns::proto::rdata::{A, CNAME, SOA};
use oxidns::proto::{DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

fn question(name: &str) -> Question {
    Question::new(
        Name::from_ascii(name).expect("benchmark qname should parse"),
        RecordType::A,
        DNSClass::IN,
    )
}

fn response_for(question: &Question, rcode: Rcode) -> Message {
    let mut response = Message::new();
    response.set_rcode(rcode);
    response.add_question(question.clone());
    response
}

fn add_cname(response: &mut Message, owner: &str, target: &str) {
    response.add_answer(Record::from_rdata(
        Name::from_ascii(owner).expect("benchmark CNAME owner should parse"),
        60,
        RData::CNAME(CNAME(
            Name::from_ascii(target).expect("benchmark CNAME target should parse"),
        )),
    ));
}

fn add_a(response: &mut Message, owner: &str) {
    response.add_answer(Record::from_rdata(
        Name::from_ascii(owner).expect("benchmark A owner should parse"),
        60,
        RData::A(A(Ipv4Addr::new(192, 0, 2, 1))),
    ));
}

fn direct_a_fixture() -> (Question, Message) {
    let question = question("www.example.com.");
    let mut response = response_for(&question, Rcode::NoError);
    add_a(&mut response, "www.example.com.");
    (question, response)
}

fn cname_fixture(hops: usize, terminal_answer: bool) -> (Question, Message) {
    let question = question("www.example.com.");
    let mut response = response_for(&question, Rcode::NoError);
    let mut owner = "www.example.com.".to_string();
    for hop in 0..hops {
        let target = format!("hop-{hop}.example.com.");
        add_cname(&mut response, &owner, &target);
        owner = target;
    }
    if terminal_answer {
        add_a(&mut response, &owner);
    }
    (question, response)
}

fn cname_nodata_fixture() -> (Question, Message) {
    let (question, mut response) = cname_fixture(1, false);
    response.add_authority(Record::from_rdata(
        Name::from_ascii("example.com.").expect("benchmark SOA owner should parse"),
        120,
        RData::SOA(SOA::new(
            Name::from_ascii("ns1.example.com.").expect("benchmark MNAME should parse"),
            Name::from_ascii("hostmaster.example.com.").expect("benchmark RNAME should parse"),
            1,
            3600,
            600,
            86400,
            30,
        )),
    ));
    (question, response)
}

fn nxdomain_fixture() -> (Question, Message) {
    let question = question("www.example.com.");
    let response = response_for(&question, Rcode::NXDomain);
    (question, response)
}

fn bench_response_classification(c: &mut Criterion) {
    let fixtures = [
        ("direct_a", direct_a_fixture()),
        ("cname_1_hop_a", cname_fixture(1, true)),
        ("cname_4_hop_a", cname_fixture(4, true)),
        ("cname_16_hop_a", cname_fixture(16, true)),
        ("cname_only", cname_fixture(1, false)),
        ("cname_nodata", cname_nodata_fixture()),
        ("nxdomain", nxdomain_fixture()),
    ];
    let mut group = c.benchmark_group("response_classification");

    for (label, (question, response)) in &fixtures {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter(|| {
                let disposition = classify_response(black_box(response), Some(black_box(question)));
                black_box(disposition);
            });
        });
    }

    group.finish();
}

criterion_group!(response_classification, bench_response_classification);
criterion_main!(response_classification);
