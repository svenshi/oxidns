// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! TCP nameserver client.

use async_trait::async_trait;

use super::super::endpoint::NameserverConfig;
use super::super::query::validate_response_id;
use super::{NameserverClient, effective_deadline};
use crate::infra::error::Result;
use crate::infra::network::deadline::{DeadlineOutcome, QueryDeadline};
use crate::infra::network::dial::SocketOptions;
use crate::infra::network::proxy::connect_tcp as proxy_connect_tcp;
use crate::infra::network::transport::tcp::{TcpTransportReader, TcpTransportWriter};
use crate::proto::Message;

#[derive(Debug)]
pub(super) struct TcpNameserverClient {
    config: NameserverConfig,
}

impl TcpNameserverClient {
    pub(super) fn new(config: NameserverConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl NameserverClient for TcpNameserverClient {
    async fn query(&self, request: Message, deadline: QueryDeadline) -> Result<Message> {
        query_tcp_config(
            &self.config,
            request,
            effective_deadline(deadline, self.config.timeout),
        )
        .await
    }

    fn label(&self) -> &str {
        self.config.label.as_str()
    }
}

pub(super) async fn query_tcp_config(
    config: &NameserverConfig,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message> {
    let stream = match deadline
        .run(proxy_connect_tcp(
            config.target(),
            SocketOptions::default(),
            config.socks5.clone(),
        ))
        .await
    {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    let (reader, writer) = stream.into_split();
    query_framed_tcp(
        TcpTransportReader::new(reader),
        TcpTransportWriter::new(writer),
        request,
        deadline,
    )
    .await
}

pub(super) async fn query_framed_tcp<R, W>(
    mut reader: TcpTransportReader<R>,
    mut writer: TcpTransportWriter<W>,
    request: Message,
    deadline: QueryDeadline,
) -> Result<Message>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let query_id = request.id();
    match deadline.run(writer.write_message(&request)).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    }
    let response = match deadline.run(reader.read_message()).await {
        DeadlineOutcome::Completed(result) => result?,
        DeadlineOutcome::Expired => return Err(deadline.timeout_error()),
    };
    validate_response_id(&response, query_id)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncWriteExt, duplex, split};

    use super::*;
    use crate::proto::{DNSClass, Name, Question, RecordType};

    fn make_query(id: u16) -> Message {
        let mut message = Message::new();
        message.set_id(id);
        message.add_question(Question::new(
            Name::from_ascii("example.com.").expect("query name should parse"),
            RecordType::A,
            DNSClass::IN,
        ));
        message
    }

    fn encode_frame(message: &Message) -> Vec<u8> {
        let body = message
            .to_bytes()
            .expect("message should serialize successfully");
        let mut frame = Vec::with_capacity(2 + body.len());
        frame.extend_from_slice(&(body.len() as u16).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    #[tokio::test]
    async fn test_query_framed_tcp_rejects_response_id_mismatch() {
        let (client, mut server) = duplex(1024);
        let (reader, writer) = split(client);
        let mut response = Message::new();
        response.set_id(8);
        let response_frame = encode_frame(&response);

        tokio::spawn(async move {
            server
                .write_all(&response_frame)
                .await
                .expect("server side should write response");
        });

        let err = query_framed_tcp(
            TcpTransportReader::new(reader),
            TcpTransportWriter::new(writer),
            make_query(7),
            QueryDeadline::new(Duration::from_secs(1)),
        )
        .await
        .expect_err("mismatched response ID should fail");

        assert!(err.to_string().contains("DNS response ID mismatch"));
    }
}
