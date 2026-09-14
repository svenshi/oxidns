// SPDX-FileCopyrightText: 2025 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded native ancillary-message encoding and checked decoding.

use std::io;
use std::mem::size_of;

#[cfg(unix)]
use libc::cmsghdr as Header;
#[cfg(windows)]
use windows::Win32::Networking::WinSock::CMSGHDR as Header;

use super::invalid_data;

#[cfg(unix)]
fn header_len() -> usize {
    // SAFETY: zero is a valid ancillary payload size.
    unsafe { libc::CMSG_LEN(0) as usize }
}

#[cfg(unix)]
fn space(len: usize) -> usize {
    // All callers first bound len by the 128-byte control buffer.
    unsafe { libc::CMSG_SPACE(len as _) as usize }
}

#[cfg(windows)]
fn header_len() -> usize {
    size_of::<Header>()
}

#[cfg(windows)]
fn space(len: usize) -> usize {
    let alignment = size_of::<usize>();
    header_len() + len.div_ceil(alignment) * alignment
}

#[repr(C)]
pub(super) struct Control {
    alignment: [Header; 0],
    pub bytes: [u8; 128],
    pub len: usize,
}

impl Control {
    pub fn new() -> Self {
        Self {
            alignment: [],
            bytes: [0; 128],
            len: 0,
        }
    }

    /// Append a native, initialized, plain-data packet information structure.
    ///
    /// # Safety
    /// Every byte of T must be initialized, including when T is moved. In
    /// particular, T must not contain padding: copying a padded value can make
    /// part of the control buffer uninitialized despite its initial zero fill.
    pub unsafe fn push<T: Copy>(&mut self, level: i32, kind: i32, value: T) {
        assert!(size_of::<T>() <= self.bytes.len());
        let occupied = space(size_of::<T>());
        assert!(self.len + occupied <= self.bytes.len());
        let header = Header {
            cmsg_len: (header_len() + size_of::<T>()) as _,
            cmsg_level: level,
            cmsg_type: kind,
        };
        // SAFETY: both writes fit in the checked buffer. Header has only
        // integer fields without padding; the caller guarantees all
        // bytes of T are initialized. Unaligned writes also support
        // BSD's four-byte ancillary alignment on 64-bit targets.
        unsafe {
            let ptr = self.bytes.as_mut_ptr().add(self.len);
            ptr.cast::<Header>().write_unaligned(header);
            ptr.add(header_len()).cast::<T>().write_unaligned(value);
        }
        self.len += occupied;
    }

    pub fn messages(&self, len: usize, truncated: bool) -> io::Result<Messages<'_>> {
        if truncated || len > self.bytes.len() {
            return Err(invalid_data("Truncated UDP packet information"));
        }
        Ok(Messages {
            remaining: &self.bytes[..len],
        })
    }
}

pub(super) struct Messages<'a> {
    remaining: &'a [u8],
}

impl<'a> Iterator for Messages<'a> {
    type Item = io::Result<(i32, i32, &'a [u8])>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        if self.remaining.len() < header_len() {
            self.remaining = &[];
            return Some(Err(invalid_data("Invalid UDP control message header")));
        }
        // SAFETY: Header consists of integers and its full size was checked.
        let header = unsafe { self.remaining.as_ptr().cast::<Header>().read_unaligned() };
        // BSD uses socklen_t; Linux and Winsock use size_t.
        #[allow(clippy::unnecessary_cast)]
        let len = header.cmsg_len as usize;
        if len < header_len() || len > self.remaining.len() {
            self.remaining = &[];
            return Some(Err(invalid_data("Invalid UDP control message length")));
        }
        let payload = &self.remaining[header_len()..len];
        let next = space(payload.len()).min(self.remaining.len());
        self.remaining = &self.remaining[next..];
        Some(Ok((header.cmsg_level, header.cmsg_type, payload)))
    }
}

/// # Safety
/// T must be a native plain-data structure valid for every bit pattern.
pub(super) unsafe fn decode<T: Copy>(bytes: &[u8]) -> io::Result<T> {
    if bytes.len() != size_of::<T>() {
        return Err(invalid_data("Invalid UDP packet information length"));
    }
    // SAFETY: the caller guarantees bit validity; exact size was checked.
    Ok(unsafe { bytes.as_ptr().cast::<T>().read_unaligned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_messages_validate_lengths_and_truncation() {
        let mut control = Control::new();
        // SAFETY: integers and byte arrays have no padding or uninitialized
        // bytes.
        unsafe {
            control.push(1, 2, 123u32);
            control.push(3, 4, [5u8; 20]);
        }
        let messages: Vec<_> = control
            .messages(control.len, false)
            .unwrap()
            .collect::<io::Result<_>>()
            .unwrap();
        assert_eq!((messages[0].0, messages[0].1), (1, 2));
        assert_eq!(unsafe { decode::<u32>(messages[0].2) }.unwrap(), 123);
        assert_eq!(messages[1].2, &[5u8; 20]);
        assert!(control.messages(control.len, true).is_err());
        assert!(control.messages(129, false).is_err());
        assert!(control.messages(1, false).unwrap().next().unwrap().is_err());
        assert!(unsafe { decode::<u64>(messages[0].2) }.is_err());
        // Zero-length records must terminate with an error, never loop.
        control.bytes[..size_of::<Header>()].fill(0);
        let mut malformed = control.messages(control.len, false).unwrap();
        assert!(malformed.next().unwrap().is_err());
        assert!(malformed.next().is_none());
    }
}
