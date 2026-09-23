// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, Ipv6Addr};
use std::pin::Pin;
use std::task::{Context, Poll};

use serde_yaml_ng::from_str;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};
use tokio::sync::Semaphore;
use tokio::time::Duration;
use tokio_util::task::AbortOnDropHandle;

use super::*;
use crate::core::context::DnsContext;
use crate::plugin::executor::{ExecStep, Executor};
use crate::plugin::test_utils::plugin_config;
use crate::proto::{DNSClass, Name, Question, Rcode, RecordType};

#[test]
fn test_tcp_factory_requires_args() {
    let factory = TcpServerFactory {};
    let cfg = plugin_config("tcp", "tcp_server", None);
    assert!(crate::plugin::test_utils::create_plugin_for_test(&factory, &cfg).is_err());
}

#[tokio::test]
async fn test_build_tcp_listener_accepts_port_only_shorthand() {
    let listener = build_tcp_listener(parse_listen_addr(":0").unwrap(), Duration::from_secs(5))
        .expect("port-only shorthand should bind");
    let addr = listener
        .local_addr()
        .expect("listener should expose local address");

    assert_eq!(addr.ip(), IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    assert_ne!(addr.port(), 0);
}

#[test]
fn test_tcp_factory_reports_entry_dependency() {
    let factory = TcpServerFactory {};
    let args = from_str(
        r#"
entry: forward_main
listen: 127.0.0.1:53
"#,
    )
    .expect("yaml should parse");
    let cfg = plugin_config("tcp", "tcp_server", Some(args));

    let deps = factory.get_dependency_specs(&cfg);

    assert_eq!(
        deps,
        vec![DependencySpec::executor("args.entry", "forward_main")]
    );
}

#[tokio::test]
async fn test_tcp_writer_exits_when_response_channel_closes() {
    let (_client, server) = tokio::io::duplex(64);
    let writer = TcpTransportWriter::new(server);
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    drop(sender);

    tokio::time::timeout(
        Duration::from_millis(100),
        write_tcp_responses(writer, receiver),
    )
    .await
    .expect("writer should exit when all response senders are dropped")
    .expect("closed response queue is a normal exit");
}

#[derive(Debug)]
struct GatedExecutor {
    started: Semaphore,
    release: Semaphore,
    finished: Semaphore,
}

impl GatedExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Semaphore::new(0),
            release: Semaphore::new(0),
            finished: Semaphore::new(0),
        })
    }
}

#[async_trait]
impl Plugin for GatedExecutor {
    fn tag(&self) -> &str {
        "gated"
    }

    async fn init(&mut self, _: &crate::plugin::PluginInitContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn destroy(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Executor for GatedExecutor {
    async fn execute(&self, context: &mut DnsContext) -> Result<ExecStep> {
        self.started.add_permits(1);
        if context.request().id() == 335 {
            self.release.acquire().await.unwrap().forget();
        }
        context.set_response(context.request().response(Rcode::NoError));
        // Represents post-processing that must survive client disconnection.
        self.finished.add_permits(1);
        Ok(ExecStep::Stop)
    }
}

fn request() -> Message {
    let mut message = Message::new();
    message.set_id(335);
    message.add_question(Question::new(
        Name::from_ascii("example.test.").unwrap(),
        RecordType::A,
        DNSClass::IN,
    ));
    message
}

async fn wait_for(semaphore: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(2), semaphore.acquire())
        .await
        .expect("executor should make progress")
        .unwrap()
        .forget();
}

#[tokio::test]
async fn connection_exit_preserves_accepted_requests_and_releases_writer() {
    // Exercise both peer EOF and cancellation by the server shutdown select.
    for cancel in [false, true] {
        let (mut client, stream) = tokio::io::duplex(1024);
        let executor = GatedExecutor::new();
        let metrics = Arc::new(ServerMetrics::new("disconnected".into(), "tcp"));
        let handler = Arc::new(RequestHandle {
            entry_executor: executor.clone(),
            metrics: Some(metrics.clone()),
        });
        let requests = TaskTracker::new();
        let tracked = requests.clone();
        let connection = AbortOnDropHandle::new(tokio::spawn(async move {
            handle_dns_stream(
                stream,
                "127.0.0.1:335".parse().unwrap(),
                handler,
                None,
                &tracked,
            )
            .await;
        }));
        let mut writer = TcpTransportWriter::new(&mut client);
        writer.write_message(&request()).await.unwrap();
        wait_for(&executor.started).await;
        if cancel {
            connection.abort();
        } else {
            client.shutdown().await.unwrap();
        }
        let result = tokio::time::timeout(Duration::from_secs(2), connection)
            .await
            .expect("connection should exit without waiting for the executor");
        assert_eq!(result.is_err(), cancel);
        // A detached writer would keep its half alive while the request holds
        // a sender. This must reach EOF before that request is released.
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), client.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        requests.close();
        assert_eq!(requests.len(), 1);
        executor.release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(2), requests.wait())
            .await
            .unwrap();
        wait_for(&executor.finished).await;
        assert!(requests.is_empty());
        use crate::infra::observability::metrics::{MetricSample, MetricSink, MetricSource};
        #[derive(Default)]
        struct Samples(Vec<(&'static str, u64)>);
        impl MetricSink for Samples {
            fn emit(&mut self, sample: MetricSample<'_>) {
                self.0.push((sample.name, sample.value));
            }
        }
        let mut samples = Samples::default();
        metrics.collect(&mut samples);
        assert!(samples.0.contains(&("server_inflight", 0)));
        assert!(samples.0.contains(&("server_controlled_total", 1)));
    }
}

struct FailedWriter<R>(R);

impl<R: AsyncRead + Unpin> AsyncRead for FailedWriter<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl<R: Unpin> AsyncWrite for FailedWriter<R> {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn write_failure_closes_connection_while_reader_is_pending() {
    let (mut client, stream) = tokio::io::duplex(1024);
    let executor = GatedExecutor::new();
    let handler = Arc::new(RequestHandle {
        entry_executor: executor.clone(),
        metrics: None,
    });
    let requests = TaskTracker::new();
    let tracked = requests.clone();
    let connection = AbortOnDropHandle::new(tokio::spawn(async move {
        handle_dns_stream(
            FailedWriter(stream),
            "127.0.0.1:335".parse().unwrap(),
            handler,
            None,
            &tracked,
        )
        .await;
    }));
    TcpTransportWriter::new(&mut client)
        .write_message(&request())
        .await
        .unwrap();
    wait_for(&executor.started).await;
    executor.release.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), connection)
        .await
        .expect("write failure must stop the reader too")
        .unwrap();
    requests.close();
    tokio::time::timeout(Duration::from_secs(2), requests.wait())
        .await
        .unwrap();
    let mut byte = [0];
    assert_eq!(client.read(&mut byte).await.unwrap(), 0);
}

#[tokio::test]
async fn writer_stops_on_first_error_and_retains_io_error_kind() {
    let writer = TcpTransportWriter::new(FailedWriter(tokio::io::empty()));
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    sender.send(request()).await.unwrap();
    sender.send(request()).await.unwrap();
    let error = tokio::time::timeout(
        Duration::from_secs(2),
        write_tcp_responses(writer, receiver),
    )
    .await
    .expect("writer must not wait for another response after an I/O failure")
    .unwrap_err();
    assert!(is_peer_disconnect(&error));
    assert!(sender.is_closed());
    assert!(!is_peer_disconnect(&DnsError::Io(
        std::io::ErrorKind::PermissionDenied.into()
    )));
    assert!(!is_peer_disconnect(&DnsError::protocol("broken pipe")));
}

#[tokio::test]
async fn pipelined_requests_can_complete_out_of_order() {
    use crate::infra::network::transport::tcp::TcpTransportReader;

    let (client, stream) = tokio::io::duplex(1024);
    let executor = GatedExecutor::new();
    let handler = Arc::new(RequestHandle {
        entry_executor: executor.clone(),
        metrics: None,
    });
    let requests = TaskTracker::new();
    let tracked = requests.clone();
    let connection = AbortOnDropHandle::new(tokio::spawn(async move {
        handle_dns_stream(
            stream,
            "127.0.0.1:335".parse().unwrap(),
            handler,
            None,
            &tracked,
        )
        .await;
    }));
    let (reader, writer) = tokio::io::split(client);
    let mut reader = TcpTransportReader::new(reader);
    let mut writer = TcpTransportWriter::new(writer);
    writer.write_message(&request()).await.unwrap();
    wait_for(&executor.started).await;
    let mut second = request();
    second.set_id(336);
    writer.write_message(&second).await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), reader.read_message())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.id(), 336);
    executor.release.add_permits(1);
    let response = tokio::time::timeout(Duration::from_secs(2), reader.read_message())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.id(), 335);
    drop(reader);
    drop(writer);
    tokio::time::timeout(Duration::from_secs(2), connection)
        .await
        .unwrap()
        .unwrap();
    requests.close();
    tokio::time::timeout(Duration::from_secs(2), requests.wait())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancellation_releases_a_writer_blocked_mid_frame() {
    let (mut client, stream) = tokio::io::duplex(1);
    let executor = GatedExecutor::new();
    let handler = Arc::new(RequestHandle {
        entry_executor: executor.clone(),
        metrics: None,
    });
    let requests = TaskTracker::new();
    let tracked = requests.clone();
    let connection = AbortOnDropHandle::new(tokio::spawn(async move {
        handle_dns_stream(
            stream,
            "127.0.0.1:335".parse().unwrap(),
            handler,
            None,
            &tracked,
        )
        .await;
    }));
    tokio::time::timeout(
        Duration::from_secs(2),
        TcpTransportWriter::new(&mut client).write_message(&request()),
    )
    .await
    .unwrap()
    .unwrap();
    wait_for(&executor.started).await;
    executor.release.add_permits(1);
    // Reading one byte proves writing has started. The one-byte duplex buffer
    // cannot hold the rest of the frame while the client stops reading.
    let mut byte = [0];
    tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut byte))
        .await
        .unwrap()
        .unwrap();
    connection.abort();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), connection)
            .await
            .unwrap()
            .is_err()
    );
    let mut remaining = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut remaining))
        .await
        .expect("connection cancellation must release a blocked writer")
        .unwrap();
    requests.close();
    tokio::time::timeout(Duration::from_secs(2), requests.wait())
        .await
        .unwrap();
}
