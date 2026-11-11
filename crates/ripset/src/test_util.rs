// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test-only helpers for parsing netlink message buffers that the crate
//! emits. These let us assert on the on-the-wire layout without going
//! through an actual netlink socket.
//!
//! Intentionally minimal: only the readers we need to verify ADD/DEL/GET
//! requests and to round-trip attribute encodings touched by the
//! protocol fixes in #122 / #127.

#![cfg(test)]
#![allow(dead_code)]

use crate::netlink::{NLA_F_NESTED, NLA_F_NET_BYTEORDER, NfGenMsg, NlAttr, NlMsgHdr, nla_align};

/// A single decoded netlink attribute. `attr_type` already has the
/// nested / net-byteorder flags stripped; callers can re-check them
/// via `nested` / `net_byteorder` when relevant.
#[derive(Clone, Debug)]
pub struct Attr<'a> {
    pub attr_type: u16,
    pub nested: bool,
    pub net_byteorder: bool,
    pub payload: &'a [u8],
}

/// Walk a flat sequence of nlattr TLVs and return them in order. The
/// input slice must start at the first attribute (callers strip the
/// fixed-size headers up front).
pub fn walk_attrs(mut data: &[u8]) -> Vec<Attr<'_>> {
    let mut out = Vec::new();
    while data.len() >= NlAttr::SIZE {
        let raw_len = u16::from_ne_bytes([data[0], data[1]]) as usize;
        let raw_type = u16::from_ne_bytes([data[2], data[3]]);
        if raw_len < NlAttr::SIZE || raw_len > data.len() {
            break;
        }
        out.push(Attr {
            attr_type: raw_type & !(NLA_F_NESTED | NLA_F_NET_BYTEORDER),
            nested: raw_type & NLA_F_NESTED != 0,
            net_byteorder: raw_type & NLA_F_NET_BYTEORDER != 0,
            payload: &data[NlAttr::SIZE..raw_len],
        });
        let aligned = nla_align(raw_len);
        if aligned > data.len() {
            break;
        }
        data = &data[aligned..];
    }
    out
}

/// Find the first attribute with a given type. Useful when callers
/// want to assert on a specific field's presence and payload.
pub fn find_attr<'a>(attrs: &'a [Attr<'a>], attr_type: u16) -> Option<&'a Attr<'a>> {
    attrs.iter().find(|a| a.attr_type == attr_type)
}

/// Split a `MsgBuffer` `as_slice()` blob produced by `nftset_operate`
/// or similar into the sequence of top-level netlink messages it
/// contains. Each returned tuple is `(header, attrs_payload)` where
/// `attrs_payload` already has the `nfgen_msgmsg` stripped, leaving just
/// the trailing attribute TLVs.
pub fn split_messages(buf: &[u8]) -> Vec<(NlMsgHdr, NfGenMsg, &[u8])> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + NlMsgHdr::SIZE <= buf.len() {
        let mut hdr = NlMsgHdr::default();
        let hdr_bytes = &buf[offset..offset + NlMsgHdr::SIZE];
        // SAFETY: NlMsgHdr is repr(C) with the same size as the slice; we
        // copy the bytes into a freshly default-initialised struct.
        unsafe {
            std::ptr::copy_nonoverlapping(
                hdr_bytes.as_ptr(),
                &mut hdr as *mut NlMsgHdr as *mut u8,
                NlMsgHdr::SIZE,
            );
        }
        let msg_len = hdr.nlmsg_len as usize;
        if msg_len < NlMsgHdr::SIZE + NfGenMsg::SIZE || offset + msg_len > buf.len() {
            break;
        }
        let mut gen_msg = NfGenMsg::default();
        let gen_msg_bytes = &buf[offset + NlMsgHdr::SIZE..offset + NlMsgHdr::SIZE + NfGenMsg::SIZE];
        unsafe {
            std::ptr::copy_nonoverlapping(
                gen_msg_bytes.as_ptr(),
                &mut gen_msg as *mut NfGenMsg as *mut u8,
                NfGenMsg::SIZE,
            );
        }
        let attrs = &buf[offset + NlMsgHdr::SIZE + NfGenMsg::SIZE..offset + msg_len];
        out.push((hdr, gen_msg, attrs));
        offset += crate::netlink::nlmsg_align(msg_len);
    }
    out
}
