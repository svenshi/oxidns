// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;

use tokio::net::UdpSocket;

use super::UdpReplyTarget;

#[derive(Debug)]
pub(super) struct State;

impl State {
    pub fn new(_: &UdpSocket) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "UDP packet information is not supported on this platform",
        ))
    }

    pub fn recv(&self, _: &UdpSocket, _: &mut [u8]) -> io::Result<(usize, UdpReplyTarget)> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub fn send(&self, _: &UdpSocket, _: &[u8], _: UdpReplyTarget) -> io::Result<usize> {
        Err(io::ErrorKind::Unsupported.into())
    }
}
