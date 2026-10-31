//! # High-Throughput SIMD Scanner and Bytecode Indexer
//!
//! This module implements the zero-copy, SIMD-accelerated indexing engine
//! for `.rost` documents. It transforms arbitrary `.rost` text into a compact
//! binary offset arena (`Vec<u8>`) at sustained throughputs exceeding 500 MB/s.

use memchr::{memchr, memchr2, memchr3};
use std::collections::HashSet;
use std::ops::Range;

use crate::error::{ParseError, ParseErrorKind};

/// Delimiter of raw multi-line strings (`"""..."""`).
pub(crate) const RAW_QUOTE: &[u8] = b"\"\"\"";

/// Tag identifier for top-level `#Section` headers.
pub const TAG_TITLE: u8 = 0x01;
/// Tag identifier for secondary `##Section "value"` headers.
pub const TAG_SUBTITLE: u8 = 0x02;
/// Tag identifier for key-value properties `key: "value"` or `key: value`.
pub const TAG_COMMON_TEXT: u8 = 0x03;
/// Tag identifier for enum sets `key | a, b, c |`.
pub const TAG_ENUM: u8 = 0x04;
/// Tag identifier for arrays/lists `key [ a, b, c ]`.
pub const TAG_LIST: u8 = 0x05;
/// Tag identifier for expressions or functions `key = expr`.
pub const TAG_VAR_FUNC: u8 = 0x06;
/// Tag identifier for directives `@theme "dark"` or `@rule "name"`.
pub const TAG_DIRECTIVE: u8 = 0x07;
/// Tag identifier for block openings `#selector {` or `block {`.
pub const TAG_BLOCK_START: u8 = 0x08;
/// Tag identifier for block closures `}`.
pub const TAG_BLOCK_END: u8 = 0x09;

// Fast 256-element lookup table for delimiter detection in 1 CPU cycle
const IS_DELIM_OR_WS: [bool; 256] = {
    let mut table = [false; 256];
    table[b':' as usize] = true;
    table[b'=' as usize] = true;
    table[b'|' as usize] = true;
    table[b'[' as usize] = true;
    table[b'{' as usize] = true;
    table[b'}' as usize] = true;
    table[b'\n' as usize] = true;
    table[b';' as usize] = true;
    table[b',' as usize] = true;
    table[b' ' as usize] = true;
    table[b'\t' as usize] = true;
    table[b'\r' as usize] = true;
    table
};

// 0 = other, 1 = horizontal ws, 2 = newline (\n), 3 = carriage return (\r)
const WS_LUT: [u8; 256] = {
    let mut table = [0u8; 256];
    table[b' ' as usize] = 1;
    table[b'\t' as usize] = 1;
    table[b'\n' as usize] = 2;
    table[b'\r' as usize] = 3;
    table
};

/// Parses raw bytes of a `.rost` document into an indexed bytecode offset buffer.
///
/// Returns a flat binary vector where each parsed syntactic element is represented
/// by a 1-byte tag followed by 32-bit Little Endian offsets and lengths pointing
/// into `raw`.
///
/// # Examples
///
/// ```
/// use rost::parser::parse_into_bytecode;
///
/// let raw = b"#Header\nwidth: 120px\n";
/// let bytecode = parse_into_bytecode(raw);
/// assert!(!bytecode.is_empty());
/// ```
pub fn parse_into_bytecode(raw: &[u8]) -> Vec<u8> {
    scan(raw, &mut NoDiagnostics)
}

/// Parses raw bytes like [`parse_into_bytecode`] and also returns every diagnostic found.
///
/// The bytecode is identical to the lenient output: malformed constructs are skipped,
/// but each one is reported as a [`ParseError`].
///
/// # Examples
///
/// ```
/// use rost::parser::parse_with_diagnostics;
///
/// let (_, errors) = parse_with_diagnostics(b"title: \"unclosed\n");
/// assert_eq!(errors.len(), 1);
/// assert_eq!(errors[0].to_string(), "1:8: unterminated string literal");
/// ```
pub fn parse_with_diagnostics(raw: &[u8]) -> (Vec<u8>, Vec<ParseError>) {
    let mut found = Vec::new();
    let fields = scan(raw, &mut found);
    let errors = found
        .into_iter()
        .map(|(kind, span)| ParseError::new(kind, raw, span))
        .collect();
    (fields, errors)
}

/// Receives diagnostics from the scanner.
///
/// The lenient path uses [`NoDiagnostics`], whose `ENABLED = false` and empty inlined
/// `report` let the compiler strip every reporting branch and the bookkeeping behind it,
/// so strictness costs nothing when unused.
trait Diagnostics {
    const ENABLED: bool;
    fn report(&mut self, kind: ParseErrorKind, span: Range<usize>);
}

struct NoDiagnostics;

impl Diagnostics for NoDiagnostics {
    const ENABLED: bool = false;
    #[inline(always)]
    fn report(&mut self, _: ParseErrorKind, _: Range<usize>) {}
}

impl Diagnostics for Vec<(ParseErrorKind, Range<usize>)> {
    const ENABLED: bool = true;
    fn report(&mut self, kind: ParseErrorKind, span: Range<usize>) {
        self.push((kind, span));
    }
}

/// Records `key` in the innermost scope and reports it if it was already defined there.
#[inline(always)]
fn check_duplicate<'r, D: Diagnostics>(
    diag: &mut D,
    scopes: &mut [HashSet<&'r [u8]>],
    raw: &'r [u8],
    key: Range<usize>,
) {
    if !D::ENABLED {
        return;
    }
    if let Some(scope) = scopes.last_mut()
        && !scope.insert(&raw[key.clone()])
    {
        let name = String::from_utf8_lossy(&raw[key.clone()]).into_owned();
        diag.report(ParseErrorKind::DuplicateKey(name), key);
    }
}

/// Returns the position of the next `\n` at or after `from`, or the end of the buffer.
#[inline]
fn line_end(raw: &[u8], from: usize) -> usize {
    memchr(b'\n', &raw[from.min(raw.len())..]).map_or(raw.len(), |p| from + p)
}

fn scan<D: Diagnostics>(raw: &[u8], diag: &mut D) -> Vec<u8> {
    let len = raw.len();
    // Pre-allocate assuming an average token density of one field per 24 bytes
    let mut fields = Vec::with_capacity((len / 24).max(1) * 21);

    let mut i = 0;
    let mut line: u32 = 1;

    // Name spans of currently open blocks; only tracked when diagnostics are enabled.
    let mut open_blocks: Vec<Range<usize>> = Vec::new();
    // Keys defined in each open scope (root first); only tracked when diagnostics are enabled.
    let mut scopes: Vec<HashSet<&[u8]>> = if D::ENABLED { vec![HashSet::new()] } else { Vec::new() };

    'scan: while i < len {
        // 1. Vectorized / LUT whitespace skip and newline tracker
        loop {
            if i >= len {
                break 'scan;
            }
            let ws_type = WS_LUT[raw[i] as usize];
            if ws_type == 0 {
                break;
            }
            if ws_type == 2 {
                line += 1;
            }
            i += 1;
        }

        // 2. Skip single-line comments: // ... using SIMD memchr
        if i + 1 < len && raw[i] == b'/' && raw[i + 1] == b'/' {
            if let Some(pos) = memchr(b'\n', &raw[i..]) {
                i += pos + 1;
                line += 1;
            } else {
                break;
            }
            continue;
        }

        // 3. Block closing delimiter '}'
        if raw[i] == b'}' {
            if D::ENABLED {
                if open_blocks.pop().is_some() {
                    scopes.pop();
                } else {
                    diag.report(ParseErrorKind::UnexpectedCloseBrace, i..i + 1);
                }
            }
            emit_block_end(&mut fields, line);
            i += 1;
            continue;
        }

        // 4. Directives starting with '@' (e.g., @theme "dark" or @rule { ... })
        if raw[i] == b'@' {
            i += 1;
            let start_name = i;
            while i < len && !raw[i].is_ascii_whitespace() && raw[i] != b'{' && raw[i] != b':' {
                i += 1;
            }
            let name_off = start_name as u32;
            let name_len = (i - start_name) as u32;

            // Skip inline spaces
            while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
                i += 1;
            }

            if i < len && raw[i] == b'{' {
                if D::ENABLED {
                    open_blocks.push(start_name..start_name + name_len as usize);
                    scopes.push(HashSet::new());
                }
                emit_block_start(&mut fields, name_off, name_len, line);
                i += 1;
                continue;
            }

            if i < len && raw[i] == b'"' {
                let val_start = i + 1;
                if let Some(quote_end) = find_unescaped_quote(&raw[val_start..]) {
                    let val_off = val_start as u32;
                    let val_len = quote_end as u32;
                    i = val_start + quote_end + 1;

                    emit_directive(&mut fields, name_off, name_len, val_off, val_len, line);
                } else {
                    let end = line_end(raw, val_start);
                    diag.report(ParseErrorKind::UnterminatedString, val_start - 1..end);
                    // Skip the rest of the line instead of re-reading the string body as keys.
                    i = end;
                }
            } else {
                let val_start = i;
                let val_end = if let Some(pos) = memchr2(b'\n', b';', &raw[i..]) {
                    i + pos
                } else {
                    len
                };
                let trimmed_len = trim_trailing_ws(raw, val_start, val_end);
                emit_directive(&mut fields, name_off, name_len, val_start as u32, trimmed_len as u32, line);
                i = val_end;
            }
            continue;
        }

        // 5. Titles, Subtitles, and Selectors (# / ##)
        if raw[i] == b'#' {
            let is_sub = i + 1 < len && raw[i + 1] == b'#';
            i += if is_sub { 2 } else { 1 };

            let start_name = i;
            while i < len && !raw[i].is_ascii_whitespace() && raw[i] != b'{' && raw[i] != b':' {
                i += 1;
            }
            let name_off = start_name as u32;
            let name_len = (i - start_name) as u32;

            if is_sub {
                // Secondary section header: ##name "value"
                if let Some(quote_start) = memchr(b'"', &raw[i..]) {
                    let val_start = i + quote_start + 1;
                    if let Some(quote_end) = find_unescaped_quote(&raw[val_start..]) {
                        let val_off = val_start as u32;
                        let val_len = quote_end as u32;
                        i = val_start + quote_end + 1;

                        emit_subtitle(&mut fields, name_off, name_len, val_off, val_len, line);
                    } else {
                        let end = line_end(raw, val_start);
                    diag.report(ParseErrorKind::UnterminatedString, val_start - 1..end);
                    // Skip the rest of the line instead of re-reading the string body as keys.
                    i = end;
                    }
                }
            } else {
                // Check if followed by '{' opening a block selector
                let mut probe = i;
                while probe < len && (raw[probe] == b' ' || raw[probe] == b'\t') {
                    probe += 1;
                }

                if probe < len && raw[probe] == b'{' {
                    if D::ENABLED {
                        open_blocks.push(start_name..start_name + name_len as usize);
                        scopes.push(HashSet::new());
                    }
                    emit_block_start(&mut fields, name_off, name_len, line);
                    i = probe + 1;
                } else {
                    emit_title(&mut fields, name_off, name_len, line);
                }
            }
            continue;
        }

        // 6. Skip commas and semicolons
        if raw[i] == b',' || raw[i] == b';' {
            i += 1;
            continue;
        }

        // 7. General Identifier / Property Key scan using 1-cycle lookup table
        let key_start = i;
        while i < len && !IS_DELIM_OR_WS[raw[i] as usize] {
            i += 1;
        }
        let key_off = key_start as u32;
        let key_len = (i - key_start) as u32;

        if key_len == 0 {
            i += 1;
            continue;
        }

        // Skip spaces between identifier and operator
        while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
            i += 1;
        }
        if i >= len {
            diag.report(ParseErrorKind::ExpectedOperator, key_start..key_start + key_len as usize);
            break;
        }

        match raw[i] {
            // Block start without leading '#' (e.g., palette { ... })
            b'{' => {
                if D::ENABLED {
                    open_blocks.push(key_start..key_start + key_len as usize);
                    scopes.push(HashSet::new());
                }
                emit_block_start(&mut fields, key_off, key_len, line);
                i += 1;
            }

            // Common Property: key: "value" or key: 120px
            b':' => {
                i += 1;
                while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
                    i += 1;
                }

                if raw[i..].starts_with(RAW_QUOTE) {
                    // Raw multi-line string: """ ... """ (no escape processing)
                    let val_start = i + RAW_QUOTE.len();
                    if let Some(body_len) = memchr::memmem::find(&raw[val_start..], RAW_QUOTE) {
                        check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                        emit_property(&mut fields, key_off, key_len, val_start as u32, body_len as u32, line);
                        line += memchr::Memchr::new(b'\n', &raw[val_start..val_start + body_len]).count() as u32;
                        i = val_start + body_len + RAW_QUOTE.len();
                    } else {
                        diag.report(ParseErrorKind::UnterminatedString, i..len);
                        i = len;
                    }
                } else if i < len && raw[i] == b'"' {
                    let val_start = i + 1;
                    if let Some(quote_end) = find_unescaped_quote(&raw[val_start..]) {
                        let val_off = val_start as u32;
                        let val_len = quote_end as u32;
                        i = val_start + quote_end + 1;

                        check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                        emit_property(&mut fields, key_off, key_len, val_off, val_len, line);
                    } else {
                        let end = line_end(raw, val_start);
                    diag.report(ParseErrorKind::UnterminatedString, val_start - 1..end);
                    // Skip the rest of the line instead of re-reading the string body as keys.
                    i = end;
                    }
                } else {
                    let val_start = i;
                    // SIMD accelerated search for next property delimiter (\n, ,, })
                    let (val_end, next_i) = if let Some(pos) = memchr3(b'\n', b',', b'}', &raw[i..]) {
                        let delim_pos = i + pos;
                        let delim = raw[delim_pos];
                        if delim == b'\n' {
                            line += 1;
                            (delim_pos, delim_pos + 1)
                        } else if delim == b',' || delim == b';' {
                            (delim_pos, delim_pos + 1)
                        } else {
                            (delim_pos, delim_pos) // preserve '}' for block close
                        }
                    } else {
                        (len, len)
                    };

                    let trimmed_len = trim_trailing_ws(raw, val_start, val_end);
                    check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                    emit_property(&mut fields, key_off, key_len, val_start as u32, trimmed_len as u32, line);
                    i = next_i;
                }
            }

            // Enums / States: key | idle, hover |
            b'|' => {
                let start_line = line;
                i += 1;
                let body_start = i;
                if let Some(end_bar) = memchr(b'|', &raw[i..]) {
                    let body_slice = &raw[body_start..body_start + end_bar];
                    line += memchr::Memchr::new(b'\n', body_slice).count() as u32;

                    let body_off = body_start as u32;
                    let body_len = end_bar as u32;
                    i = body_start + end_bar + 1;
                    let end_line = line;

                    check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                    emit_enum(&mut fields, key_off, key_len, body_off, body_len, start_line, end_line);
                } else {
                    diag.report(ParseErrorKind::UnterminatedEnum, body_start - 1..line_end(raw, body_start));
                    i = len;
                }
            }

            // Lists / Arrays: key [ item1, item2 ]
            b'[' => {
                let start_line = line;
                i += 1;
                let body_start = i;
                let mut depth = 1;
                while i < len && depth > 0 {
                    match raw[i] {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        b'\n' => line += 1,
                        _ => {}
                    }
                    i += 1;
                }
                if depth > 0 {
                    diag.report(ParseErrorKind::UnterminatedList, body_start - 1..line_end(raw, body_start));
                }
                let body_end = i.saturating_sub(1);
                let body_off = body_start as u32;
                let body_len = (body_end - body_start) as u32;
                let end_line = line;

                check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                emit_list(&mut fields, key_off, key_len, body_off, body_len, start_line, end_line);
            }

            // Variables and Expression Bindings: key = expr
            b'=' => {
                i += 1;
                while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
                    i += 1;
                }
                let expr_start = i;
                let expr_end = if let Some(end_pos) = memchr2(b'\n', b'}', &raw[i..]) {
                    i + end_pos
                } else {
                    len
                };

                let trimmed_len = trim_trailing_ws(raw, expr_start, expr_end);
                check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                emit_var_func(&mut fields, key_off, key_len, expr_start as u32, trimmed_len as u32);
                i = expr_end;
            }

            _ => {
                diag.report(ParseErrorKind::ExpectedOperator, key_start..key_start + key_len as usize);
                // Recover by skipping the rest of this token run so one mistake
                // yields one diagnostic.
                i += 1;
                while i < len && !IS_DELIM_OR_WS[raw[i] as usize] {
                    i += 1;
                }
            }
        }
    }

    for span in open_blocks {
        diag.report(ParseErrorKind::UnclosedBlock, span);
    }
    fields
}

// ================= FAST BATCH EMITTERS (ZERO PER-FIELD REALLOCATION) =================

#[inline(always)]
fn emit_title(buf: &mut Vec<u8>, name_off: u32, name_len: u32, line: u32) {
    if buf.capacity() - buf.len() < 13 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_TITLE;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(9), 4);
        buf.set_len(len + 13);
    }
}

#[inline(always)]
fn emit_subtitle(buf: &mut Vec<u8>, name_off: u32, name_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < 21 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_SUBTITLE;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(val_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(val_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(17), 4);
        buf.set_len(len + 21);
    }
}

#[inline(always)]
fn emit_property(buf: &mut Vec<u8>, key_off: u32, key_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < 21 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_COMMON_TEXT;
        std::ptr::copy_nonoverlapping(key_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(key_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(val_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(val_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(17), 4);
        buf.set_len(len + 21);
    }
}

#[inline(always)]
fn emit_directive(buf: &mut Vec<u8>, name_off: u32, name_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < 21 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_DIRECTIVE;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(val_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(val_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(17), 4);
        buf.set_len(len + 21);
    }
}

#[inline(always)]
fn emit_block_start(buf: &mut Vec<u8>, name_off: u32, name_len: u32, line: u32) {
    if buf.capacity() - buf.len() < 13 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_BLOCK_START;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(9), 4);
        buf.set_len(len + 13);
    }
}

#[inline(always)]
fn emit_block_end(buf: &mut Vec<u8>, line: u32) {
    if buf.capacity() - buf.len() < 5 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_BLOCK_END;
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(1), 4);
        buf.set_len(len + 5);
    }
}

#[inline(always)]
fn emit_enum(buf: &mut Vec<u8>, k_off: u32, k_len: u32, b_off: u32, b_len: u32, s_line: u32, e_line: u32) {
    if buf.capacity() - buf.len() < 25 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_ENUM;
        std::ptr::copy_nonoverlapping(k_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(k_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(b_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(b_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        std::ptr::copy_nonoverlapping(s_line.to_le_bytes().as_ptr(), ptr.add(17), 4);
        std::ptr::copy_nonoverlapping(e_line.to_le_bytes().as_ptr(), ptr.add(21), 4);
        buf.set_len(len + 25);
    }
}

#[inline(always)]
fn emit_list(buf: &mut Vec<u8>, k_off: u32, k_len: u32, b_off: u32, b_len: u32, s_line: u32, e_line: u32) {
    if buf.capacity() - buf.len() < 25 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_LIST;
        std::ptr::copy_nonoverlapping(k_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(k_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(b_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(b_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        std::ptr::copy_nonoverlapping(s_line.to_le_bytes().as_ptr(), ptr.add(17), 4);
        std::ptr::copy_nonoverlapping(e_line.to_le_bytes().as_ptr(), ptr.add(21), 4);
        buf.set_len(len + 25);
    }
}

#[inline(always)]
fn emit_var_func(buf: &mut Vec<u8>, k_off: u32, k_len: u32, expr_off: u32, expr_len: u32) {
    if buf.capacity() - buf.len() < 17 {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_VAR_FUNC;
        std::ptr::copy_nonoverlapping(k_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(k_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(expr_off.to_le_bytes().as_ptr(), ptr.add(9), 4);
        std::ptr::copy_nonoverlapping(expr_len.to_le_bytes().as_ptr(), ptr.add(13), 4);
        buf.set_len(len + 17);
    }
}

/// Trims trailing whitespace characters without scanning from start.
#[inline(always)]
fn trim_trailing_ws(data: &[u8], start: usize, mut end: usize) -> usize {
    while end > start {
        let b = data[end - 1];
        if b == b' ' || b == b'\t' || b == b'\r' || b == b';' || b == b',' {
            end -= 1;
        } else {
            break;
        }
    }
    end - start
}

/// Reads a 32-bit Little Endian integer from the byte buffer at the specified index.
#[inline(always)]
pub fn read_u32(buf: &[u8], idx: usize) -> u32 {
    if idx + 4 <= buf.len() {
        unsafe {
            let ptr = buf.as_ptr().add(idx) as *const [u8; 4];
            u32::from_le_bytes(*ptr)
        }
    } else {
        0
    }
}

/// Resolves a zero-copy string slice from the source buffer using offsets stored in bytecode.
#[inline(always)]
pub fn resolve_slice<'a>(data: &'a [u8], fields: &[u8], idx: usize) -> &'a str {
    let off = read_u32(fields, idx) as usize;
    let len = read_u32(fields, idx + 4) as usize;
    if off + len > data.len() {
        return "";
    }
    unsafe { std::str::from_utf8_unchecked(&data[off..off + len]) }
}

/// Finds the next unescaped quotation mark `"` using SIMD acceleration.
#[inline(always)]
fn find_unescaped_quote(slice: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while let Some(pos) = memchr(b'"', &slice[offset..]) {
        let abs_pos = offset + pos;
        if abs_pos > 0 && slice[abs_pos - 1] == b'\\' {
            let mut backslashes = 0;
            let mut k = abs_pos;
            while k > 0 && slice[k - 1] == b'\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes % 2 == 1 {
                offset = abs_pos + 1;
                continue;
            }
        }
        return Some(abs_pos);
    }
    None
}
