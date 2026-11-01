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

/// Byte length of a [`TAG_TITLE`] / [`TAG_BLOCK_START`] record: tag + name (offset, len) + line.
pub const REC_NAMED: usize = 13;
/// Byte length of a [`TAG_COMMON_TEXT`] / [`TAG_SUBTITLE`] / [`TAG_DIRECTIVE`] record:
/// tag + name (offset, len) + value (offset, len) + line.
pub const REC_PAIR: usize = 21;
/// Byte length of a [`TAG_ENUM`] / [`TAG_LIST`] record:
/// tag + name (offset, len) + body (offset, len) + start line + end line.
pub const REC_RANGE: usize = 25;
/// Byte length of a [`TAG_VAR_FUNC`] record: tag + name (offset, len) + expression (offset, len).
pub const REC_BINDING: usize = 17;
/// Byte length of a [`TAG_BLOCK_END`] record: tag + line.
pub const REC_BLOCK_END: usize = 5;

/// Returns the total byte length of the bytecode record starting with `tag`.
///
/// Unknown tags report a length of `1` so scanners always make progress.
#[inline(always)]
pub const fn record_len(tag: u8) -> usize {
    match tag {
        TAG_TITLE | TAG_BLOCK_START => REC_NAMED,
        TAG_COMMON_TEXT | TAG_SUBTITLE | TAG_DIRECTIVE => REC_PAIR,
        TAG_ENUM | TAG_LIST => REC_RANGE,
        TAG_VAR_FUNC => REC_BINDING,
        TAG_BLOCK_END => REC_BLOCK_END,
        _ => 1,
    }
}

// Fast 256-element lookup table for delimiter detection in 1 CPU cycle
pub(crate) const IS_DELIM_OR_WS: [bool; 256] = {
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

/// Keys defined in each open scope, used by strict mode to detect duplicates.
///
/// All scopes share one key stack, so opening a block allocates nothing; small scopes
/// (the common case) are checked with a linear scan, and a scope switches to a hash set
/// only once it grows past [`ScopeKeys::LINEAR_MAX`] keys.
struct ScopeKeys<'r> {
    keys: Vec<&'r [u8]>,
    scopes: Vec<Scope<'r>>,
}

struct Scope<'r> {
    start: usize,
    set: Option<HashSet<&'r [u8]>>,
}

impl<'r> ScopeKeys<'r> {
    const LINEAR_MAX: usize = 16;

    /// Creates the tracker without allocating; the root scope is added on first use.
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            scopes: Vec::new(),
        }
    }

    fn root(&mut self) {
        if self.scopes.is_empty() {
            self.scopes.push(Scope { start: 0, set: None });
        }
    }

    fn open(&mut self) {
        self.root();
        self.scopes.push(Scope {
            start: self.keys.len(),
            set: None,
        });
    }

    fn close(&mut self) {
        if self.scopes.len() > 1
            && let Some(scope) = self.scopes.pop()
        {
            self.keys.truncate(scope.start);
        }
    }

    /// Records `key` in the innermost scope; returns `false` if it was already there.
    fn insert(&mut self, key: &'r [u8]) -> bool {
        self.root();
        let Some(scope) = self.scopes.last_mut() else {
            return true;
        };
        if let Some(set) = &mut scope.set {
            return set.insert(key);
        }
        let current = &self.keys[scope.start..];
        if current.contains(&key) {
            return false;
        }
        if current.len() >= Self::LINEAR_MAX {
            let mut set: HashSet<&[u8]> = current.iter().copied().collect();
            set.insert(key);
            self.keys.truncate(scope.start);
            scope.set = Some(set);
        } else {
            self.keys.push(key);
        }
        true
    }
}

/// Records `key` in the innermost scope and reports it if it was already defined there.
#[inline(always)]
fn check_duplicate<'r, D: Diagnostics>(
    diag: &mut D,
    scopes: &mut ScopeKeys<'r>,
    raw: &'r [u8],
    key: Range<usize>,
) {
    if !D::ENABLED {
        return;
    }
    if !scopes.insert(&raw[key.clone()]) {
        let name = String::from_utf8_lossy(&raw[key.clone()]).into_owned();
        diag.report(ParseErrorKind::DuplicateKey(name), key);
    }
}

/// Returns the position of the next `\n` at or after `from`, or the end of the buffer.
#[inline]
fn line_end(raw: &[u8], from: usize) -> usize {
    memchr(b'\n', &raw[from.min(raw.len())..]).map_or(raw.len(), |p| from + p)
}

/// A document wrapped in `{ … }`, as a JSON object is, reads as its contents: returns
/// where the contents start and the position of the closing `}` (the last
/// non-whitespace byte).
pub(crate) fn root_braces(raw: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    loop {
        while i < raw.len() && WS_LUT[raw[i] as usize] != 0 {
            i += 1;
        }
        if raw[i..].starts_with(b"//") {
            i = memchr(b'\n', &raw[i..]).map_or(raw.len(), |p| i + p);
        } else {
            break;
        }
    }
    if raw.get(i) != Some(&b'{') {
        return None;
    }
    let end = raw.iter().rposition(|b| !b.is_ascii_whitespace())?;
    (end > i && raw[end] == b'}').then_some((i + 1, end))
}

fn scan<D: Diagnostics>(raw: &[u8], diag: &mut D) -> Vec<u8> {
    // Truncating at the closing brace keeps every offset absolute.
    let (start, raw) = match root_braces(raw) {
        Some((start, end)) => (start, &raw[..end]),
        None => (0, raw),
    };
    let len = raw.len();
    // Pre-allocate assuming an average token density of one field per 24 bytes
    let mut fields = Vec::with_capacity((len / 24).max(1) * 21);

    let mut i = start;
    let mut line: u32 = 1 + count_newlines(&raw[..start]);

    // Name spans of currently open blocks; only tracked when diagnostics are enabled.
    let mut open_blocks: Vec<Range<usize>> = Vec::new();
    // Keys defined in each open scope (root first); only tracked when diagnostics are enabled.
    let mut scopes = ScopeKeys::new();

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
                    scopes.close();
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
                    scopes.open();
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
                    line += count_newlines(&raw[val_start..val_start + quote_end]);
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
                // Secondary section header: ##name "value". The value is optional and must
                // follow on the same line; a quote further ahead belongs to other entries.
                while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
                    i += 1;
                }
                if i < len && raw[i] == b'"' {
                    let val_start = i + 1;
                    if let Some(quote_end) = find_unescaped_quote(&raw[val_start..]) {
                        let val_off = val_start as u32;
                        let val_len = quote_end as u32;
                        i = val_start + quote_end + 1;

                        emit_subtitle(&mut fields, name_off, name_len, val_off, val_len, line);
                        line += count_newlines(&raw[val_start..val_start + quote_end]);
                    } else {
                        let end = line_end(raw, val_start);
                        diag.report(ParseErrorKind::UnterminatedString, val_start - 1..end);
                        // Skip the rest of the line instead of re-reading the string body as keys.
                        i = end;
                    }
                } else {
                    emit_subtitle(&mut fields, name_off, name_len, i as u32, 0, line);
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
                        scopes.open();
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
        let mut key_start = i;
        while i < len && !KEY_END[raw[i] as usize] {
            i += 1;
        }
        let mut key_end = i;
        let quoted = key_end == key_start && raw[i] == b'"';
        if quoted {
            // Quoted key (`"any key": …`): the record holds the body, and the `"` right
            // after it is how readers tell a quoted key from a bare one.
            let Some(end) = quoted_key_end(raw, i) else {
                let end = line_end(raw, i);
                diag.report(ParseErrorKind::UnterminatedString, i..end);
                i = end;
                continue;
            };
            key_start = i + 1;
            key_end = end;
            line += count_newlines(&raw[key_start..key_end]);
            i = key_end + 1;
        }
        let key_off = key_start as u32;
        let key_len = (key_end - key_start) as u32;

        if key_len == 0 && !quoted {
            // A delimiter (`:`, `=`, `|`, `[`, `{`) where a key should start.
            diag.report(ParseErrorKind::ExpectedKey, i..i + 1);
            // `: value` / `= expr` is a property missing its key: skip the value too, so one
            // mistake yields one diagnostic.
            i = match raw[i] {
                b':' => find_value_end(raw, i + 1).unwrap_or(len),
                b'=' => memchr2(b'\n', b'}', &raw[i..]).map_or(len, |p| i + p),
                _ => i + 1,
            };
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

        // Opens the block of `key {` (at `i`, the brace).
        macro_rules! open_block {
            () => {{
                if D::ENABLED {
                    open_blocks.push(key_start..key_start + key_len as usize);
                    scopes.open();
                }
                emit_block_start(&mut fields, key_off, key_len, line);
                i += 1;
            }};
        }
        // Reads the list of `key [ … ]` (at `i`, the bracket).
        macro_rules! scan_list {
            () => {{
                let start_line = line;
                let body_start = i + 1;
                // A closed list ends before its `]`; an unterminated one runs to EOF.
                let body_end = match find_list_end(raw, body_start) {
                    Some(end) => {
                        i = end + 1;
                        end
                    }
                    None => {
                        diag.report(ParseErrorKind::UnterminatedList, body_start - 1..line_end(raw, body_start));
                        i = len;
                        len
                    }
                };
                line += count_newlines(&raw[body_start..body_end]);
                let body_off = body_start as u32;
                let body_len = (body_end - body_start) as u32;
                let end_line = line;

                check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                emit_list(&mut fields, key_off, key_len, body_off, body_len, start_line, end_line);
            }};
        }

        match raw[i] {
            // Block start without leading '#' (e.g., palette { ... })
            b'{' => open_block!(),

            // Common Property: key: "value" or key: 120px
            b':' => {
                i += 1;
                while i < len && (raw[i] == b' ' || raw[i] == b'\t') {
                    i += 1;
                }

                // `key: { … }` and `key: [ … ]` are the JSON spellings of `key {` / `key [`.
                if i < len && raw[i] == b'{' {
                    open_block!();
                } else if i < len && raw[i] == b'[' {
                    scan_list!();
                } else if i < len && raw[i] == b'"' && !raw[i..].starts_with(RAW_QUOTE) {
                    let val_start = i + 1;
                    if let Some((quote_end, newlines)) = scan_quoted(&raw[val_start..]) {
                        let val_off = val_start as u32;
                        let val_len = quote_end as u32;
                        i = val_start + quote_end + 1;

                        check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                        emit_property(&mut fields, key_off, key_len, val_off, val_len, line);
                        line += newlines;
                    } else {
                        let end = line_end(raw, val_start);
                        diag.report(ParseErrorKind::UnterminatedString, val_start - 1..end);
                        // Skip the rest of the line instead of re-reading the string body as keys.
                        i = end;
                    }
                } else if i < len && raw[i] == b'"' {
                    // Raw multi-line string: """ ... """ (no escape processing)
                    let val_start = i + RAW_QUOTE.len();
                    if let Some(body_len) = memchr::memmem::find(&raw[val_start..], RAW_QUOTE) {
                        check_duplicate(diag, &mut scopes, raw, key_start..key_start + key_len as usize);
                        emit_property(&mut fields, key_off, key_len, val_start as u32, body_len as u32, line);
                        line += count_newlines(&raw[val_start..val_start + body_len]);
                        i = val_start + body_len + RAW_QUOTE.len();
                    } else {
                        diag.report(ParseErrorKind::UnterminatedString, i..len);
                        i = len;
                    }
                } else {
                    let val_start = i;
                    // SIMD accelerated search for next property delimiter (\n, ,, })
                    let (val_end, next_i) = if let Some(delim_pos) = find_value_end(raw, i) {
                        if raw[delim_pos] == b',' {
                            (delim_pos, delim_pos + 1)
                        } else {
                            // Leave '\n' for the whitespace skipper (which counts the line
                            // after this property is emitted) and '}' for the block close.
                            (delim_pos, delim_pos)
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
                    line += count_newlines(body_slice);

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
            b'[' => scan_list!(),

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
    if buf.capacity() - buf.len() < REC_NAMED {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_TITLE;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(9), 4);
        buf.set_len(len + REC_NAMED);
    }
}

#[inline(always)]
fn emit_subtitle(buf: &mut Vec<u8>, name_off: u32, name_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < REC_PAIR {
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
        buf.set_len(len + REC_PAIR);
    }
}

#[inline(always)]
fn emit_property(buf: &mut Vec<u8>, key_off: u32, key_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < REC_PAIR {
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
        buf.set_len(len + REC_PAIR);
    }
}

#[inline(always)]
fn emit_directive(buf: &mut Vec<u8>, name_off: u32, name_len: u32, val_off: u32, val_len: u32, line: u32) {
    if buf.capacity() - buf.len() < REC_PAIR {
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
        buf.set_len(len + REC_PAIR);
    }
}

#[inline(always)]
fn emit_block_start(buf: &mut Vec<u8>, name_off: u32, name_len: u32, line: u32) {
    if buf.capacity() - buf.len() < REC_NAMED {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_BLOCK_START;
        std::ptr::copy_nonoverlapping(name_off.to_le_bytes().as_ptr(), ptr.add(1), 4);
        std::ptr::copy_nonoverlapping(name_len.to_le_bytes().as_ptr(), ptr.add(5), 4);
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(9), 4);
        buf.set_len(len + REC_NAMED);
    }
}

#[inline(always)]
fn emit_block_end(buf: &mut Vec<u8>, line: u32) {
    if buf.capacity() - buf.len() < REC_BLOCK_END {
        buf.reserve(4096);
    }
    let len = buf.len();
    unsafe {
        let ptr = buf.as_mut_ptr().add(len);
        *ptr = TAG_BLOCK_END;
        std::ptr::copy_nonoverlapping(line.to_le_bytes().as_ptr(), ptr.add(1), 4);
        buf.set_len(len + REC_BLOCK_END);
    }
}

#[inline(always)]
fn emit_enum(buf: &mut Vec<u8>, k_off: u32, k_len: u32, b_off: u32, b_len: u32, s_line: u32, e_line: u32) {
    if buf.capacity() - buf.len() < REC_RANGE {
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
        buf.set_len(len + REC_RANGE);
    }
}

#[inline(always)]
fn emit_list(buf: &mut Vec<u8>, k_off: u32, k_len: u32, b_off: u32, b_len: u32, s_line: u32, e_line: u32) {
    if buf.capacity() - buf.len() < REC_RANGE {
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
        buf.set_len(len + REC_RANGE);
    }
}

#[inline(always)]
fn emit_var_func(buf: &mut Vec<u8>, k_off: u32, k_len: u32, expr_off: u32, expr_len: u32) {
    if buf.capacity() - buf.len() < REC_BINDING {
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
        buf.set_len(len + REC_BINDING);
    }
}

/// Finds the closing quote of a string body and counts the newlines inside it.
///
/// Values are usually short, so one byte loop does both jobs for the first 64 bytes;
/// longer strings fall back to the SIMD searches.
#[inline(always)]
fn scan_quoted(body: &[u8]) -> Option<(usize, u32)> {
    let mut newlines = 0;
    let mut i = 0;
    while i < body.len().min(64) {
        match body[i] {
            b'"' => return Some((i, newlines)),
            b'\\' => i += 1,
            b'\n' => newlines += 1,
            _ => {}
        }
        i += 1;
    }
    let end = find_unescaped_quote(body)?;
    Some((end, count_newlines(&body[..end])))
}

/// Counts `\n` bytes using SIMD acceleration, to keep line numbers accurate across
/// multi-line tokens.
#[inline(always)]
fn count_newlines(slice: &[u8]) -> u32 {
    // Most tokens are single-line: one search answers that before building an iterator.
    match memchr(b'\n', slice) {
        None => 0,
        Some(first) => 1 + memchr::Memchr::new(b'\n', &slice[first + 1..]).count() as u32,
    }
}

/// Trims trailing whitespace characters without scanning from start.
#[inline(always)]
pub(crate) fn trim_trailing_ws(data: &[u8], start: usize, mut end: usize) -> usize {
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
///
/// Crate-private on purpose: it skips UTF-8 validation, which is only sound because
/// `data` is validated when a [`Template`](crate::Template) is built and the parser only
/// emits offsets that fall on ASCII delimiters (and therefore on char boundaries).
#[inline(always)]
pub(crate) fn resolve_slice<'a>(data: &'a [u8], fields: &[u8], idx: usize) -> &'a str {
    let off = read_u32(fields, idx) as usize;
    let len = read_u32(fields, idx + 4) as usize;
    if off + len > data.len() {
        return "";
    }
    unsafe { std::str::from_utf8_unchecked(&data[off..off + len]) }
}

/// Resolves the key of the record at `idx`, decoding escapes if the key was quoted.
///
/// A quoted key is followed by its closing `"`; a bare key never is (see [`KEY_END`]).
#[cfg_attr(not(feature = "serde"), allow(dead_code))] // read by the serde deserializers
#[inline]
pub(crate) fn resolve_key<'a>(data: &'a [u8], fields: &[u8], idx: usize) -> (&'a str, std::borrow::Cow<'a, str>) {
    let key = resolve_slice(data, fields, idx);
    let end = read_u32(fields, idx) as usize + key.len();
    let text = if data.get(end) == Some(&b'"') { quoted_key_text(key) } else { std::borrow::Cow::Borrowed(key) };
    (key, text)
}

#[cold]
fn quoted_key_text(key: &str) -> std::borrow::Cow<'_, str> {
    crate::field::unescape(key)
}

/// Finds the delimiter (`\n`, `,` or `}`) that ends an unquoted value starting at `start`.
///
/// Commas and braces nested inside `(...)`, `[...]` or quoted strings belong to the value
/// (`rgba(1, 2, 3)`, `url("a,b")`); a newline always terminates it. Returns the absolute
/// position of the delimiter, or `None` if the value runs to the end of the buffer.
#[inline(always)]
pub(crate) fn find_value_end(raw: &[u8], start: usize) -> Option<usize> {
    // Values are usually a handful of bytes, so one LUT-driven byte loop beats
    // setting up SIMD searches per value.
    let mut i = start;
    while i < raw.len() {
        match VALUE_LUT[raw[i] as usize] {
            0 => i += 1,
            VALUE_END => return Some(i),
            _ => return find_nested_value_end(raw, i),
        }
    }
    None
}

/// Bytes that end a bare key: delimiters, whitespace and `"` (which starts a quoted key,
/// so a bare key is never directly followed by a quote).
pub(crate) const KEY_END: [bool; 256] = {
    let mut table = IS_DELIM_OR_WS;
    table[b'"' as usize] = true;
    table
};

/// Byte class for [`find_value_end`]: a delimiter that ends the value.
const VALUE_END: u8 = 1;
/// Byte class for [`find_value_end`]: a byte that may nest delimiters.
const VALUE_NEST: u8 = 2;

const VALUE_LUT: [u8; 256] = {
    let mut table = [0u8; 256];
    table[b'\n' as usize] = VALUE_END;
    table[b',' as usize] = VALUE_END;
    table[b'}' as usize] = VALUE_END;
    table[b'(' as usize] = VALUE_NEST;
    table[b'[' as usize] = VALUE_NEST;
    table[b'"' as usize] = VALUE_NEST;
    table
};

/// Slow path of [`find_value_end`] from the first `(`, `[` or `"` of a value.
#[cold]
fn find_nested_value_end(raw: &[u8], nested: usize) -> Option<usize> {
    let mut depth: u32 = 0;
    let mut i = nested;
    while i < raw.len() {
        match raw[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'"' => match find_unescaped_quote(&raw[i + 1..]) {
                Some(end) if memchr(b'\n', &raw[i + 1..i + 1 + end]).is_none() => i += end + 1,
                _ => {}
            },
            b'\n' => return Some(i),
            b',' | b'}' if depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Returns the position of the quote closing the quoted key that starts at `start`.
///
/// Out of line: quoted keys are rare, and inlined this path slows the bare-key loop.
#[cold]
#[inline(never)]
fn quoted_key_end(raw: &[u8], start: usize) -> Option<usize> {
    find_unescaped_quote(&raw[start + 1..]).map(|end| start + 1 + end)
}

/// Finds the `]` closing a list whose body starts at `start`.
///
/// Nested lists are balanced and quoted strings are skipped, so `["a]b"]` is one item.
/// A quote without a closing partner is treated as a plain character.
pub(crate) fn find_list_end(raw: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1u32;
    let mut i = start;
    // Only brackets and quotes matter: jump between them instead of walking every byte.
    while let Some(skip) = memchr3(b'[', b']', b'"', &raw[i.min(raw.len())..]) {
        i += skip;
        match raw[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' if raw[i..].starts_with(RAW_QUOTE) => {
                if let Some(end) = memchr::memmem::find(&raw[i + 3..], RAW_QUOTE) {
                    i += end + 5;
                }
            }
            b'"' => {
                if let Some(end) = find_unescaped_quote(&raw[i + 1..]) {
                    i += end + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Finds the next unescaped quotation mark `"` using SIMD acceleration.
#[inline(always)]
pub(crate) fn find_unescaped_quote(slice: &[u8]) -> Option<usize> {
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
