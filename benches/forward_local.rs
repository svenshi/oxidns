// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native-only loopback forwarding diagnostic. Socket scheduling is
//! intentionally excluded from deterministic Callgrind comparisons; no public
//! DNS is contacted.

use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use hotpath::criterion::{Criterion, criterion_group, criterion_main};
use oxidns::config::types::Config;
use oxidns::core::context::DnsContext;
use oxidns::plugin::init as init_plugins;
use oxidns::proto::{A, DNSClass, Message, Name, Question, RData, Rcode, Record, RecordType};

struct Upstream {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    queries: Arc<AtomicUsize>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Upstream {
    fn start() -> Self {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let address = socket.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let queries = Arc::new(AtomicUsize::new(0));
        let worker = {
            let stop = stop.clone();
            let queries = queries.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while !stop.load(Ordering::Relaxed) {
                    let (length, peer) = match socket.recv_from(&mut buffer) {
                        Ok(packet) => packet,
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            continue;
                        }
                        Err(error) => panic!("fixture upstream receive failed: {error}"),
                    };
                    let query = Message::from_bytes(&buffer[..length]).unwrap();
                    let question = query.first_question().unwrap();
                    let mut response = query.response(Rcode::NoError);
                    response.add_answer(Record::from_rdata(
                        question.name().clone(),
                        60,
                        RData::A(A(Ipv4Addr::new(192, 0, 2, 10))),
                    ));
                    queries.fetch_add(1, Ordering::Relaxed);
                    socket.send_to(&response.to_bytes().unwrap(), peer).unwrap();
                }
            })
        };
        Self {
            address,
            stop,
            queries,
            worker: Some(worker),
        }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .unwrap()
            .join()
            .expect("fixture upstream should stop");
    }
}

fn bench_forward(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let upstream = Upstream::start();
    let config: Config = serde_yaml_ng::from_str(&format!(
        r#"
plugins:
  - tag: forward
    type: forward
    args:
      upstreams:
        - addr: "udp://{}"
  - tag: bench
    type: sequence
    args:
      - exec: $forward
"#,
        upstream.address
    ))
    .unwrap();
    let registry = rt.block_on(init_plugins(config)).unwrap();
    let executor = registry.get_plugin("bench").unwrap().to_executor();
    let mut request = Message::new();
    request.set_id(1234);
    request.add_question(Question::new(
        Name::from_ascii("bench.test.").unwrap(),
        RecordType::A,
        DNSClass::IN,
    ));
    let wire = request.to_bytes().unwrap();
    let run = || {
        let mut context = DnsContext::new(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 5300)),
            Message::from_bytes(black_box(&wire)).unwrap(),
        );
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), executor.execute(&mut context)).await
        })
        .expect("local forwarding timed out")
        .expect("local forwarding failed");
        black_box(context.response().unwrap().to_bytes().unwrap());
        context
    };
    // Warm the connection pool, not a DNS cache. Every timed request must reach
    // the fixture upstream; the response ID and original question must survive.
    let probe = run();
    let response = probe.response().unwrap();
    assert_eq!(response.id(), request.id());
    assert_eq!(response.first_question(), request.first_question());
    assert_eq!(response.answers().len(), 1);
    assert_eq!(response.answers()[0].ttl(), 60);
    let before = upstream.queries.load(Ordering::Relaxed);
    c.bench_function("forward_local/udp_reused", |b| {
        b.iter(|| {
            black_box(run());
        })
    });
    if let Ok(iterations) = std::env::var("HOTPATH_CRITERION_ITERATIONS") {
        assert_eq!(
            upstream.queries.load(Ordering::Relaxed) - before,
            iterations.parse::<usize>().unwrap()
        );
    }
    rt.block_on(registry.destroy());
}

criterion_group!(forward_local, bench_forward);
criterion_main!(allocator = none; forward_local);
