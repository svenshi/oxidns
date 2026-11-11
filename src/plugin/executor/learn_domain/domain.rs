// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: GPL-3.0-or-later

/// Borrow the registrable domain from a canonical, lowercased request qname.
pub(super) fn registrable_domain(qname: &str) -> Option<&str> {
    // PSL helpers split presentation strings at dots. Escaped labels may
    // contain literal dots or arbitrary octets, so do not reinterpret them as
    // hostnames. Reverse lookup namespaces also have no registrable site
    // boundary and must never be collapsed into a network-wide suffix rule.
    if qname.contains('\\')
        || qname == "in-addr.arpa"
        || qname.ends_with(".in-addr.arpa")
        || qname == "ip6.arpa"
        || qname.ends_with(".ip6.arpa")
    {
        return None;
    }

    let domain = psl::domain(qname.as_bytes())?;
    // The PSL prevailing wildcard also returns domains for unknown suffixes.
    // Learning them would guess at a boundary for local or future namespaces.
    if !domain.suffix().is_known() {
        return None;
    }
    std::str::from_utf8(domain.as_bytes()).ok()
}
