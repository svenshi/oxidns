// SPDX-FileCopyrightText: 2024 erikoest
// SPDX-FileCopyrightText: 2026 Sven Shi
// SPDX-License-Identifier: MIT

mod parser;

pub use crate::parser::{
    ParseOptions, ZoneParseError, parse_file, parse_str, visit_file, visit_str,
};
