# RESP — notes from working it out

Loose notes from the first pass at RESP. The point is to capture the mental model, not to be a spec — the [real spec](https://redis.io/docs/latest/develop/reference/protocol-spec/) is the source of truth.

## What RESP actually is

A wire protocol. It's the language clients and the server use to talk over TCP. Nothing about storage. The server can hold values however it wants internally; RESP only governs the bytes on the socket.

Two clean boundaries:

- `bytes -> Command` (parser, inbound)
- `Reply -> bytes` (serializer, outbound)

Everything between those two is the server's house, its rules. Keys and values get stored as byte strings (`Vec<u8>` or similar) — not as `RespValue`. Don't tangle the wire enum into the cache.

## Frame shape

Sigil on the first byte tells you the type. Every framed piece ends in `\r\n`.

- `+OK\r\n` — simple string
- `-ERR something\r\n` — simple error
- `:123\r\n` — integer (ASCII digits, not 8 binary bytes; bounded so no length prefix needed)
- `$5\r\npedro\r\n` — bulk string: header `$<len>\r\n`, then raw bytes (no sigil), then `\r\n`
- `$-1\r\n` — null bulk string (RESP2 legacy null)
- `*2\r\n...\r\n...\r\n` — array of N RESP values (recursive)

Whole protocol is human-readable ASCII *except* bulk-string payloads, which are arbitrary bytes. Means you can `nc` into a server and type commands by hand.

## Scope for this project

A Redis server speaking GET / SET / DEL only ever needs to handle a narrow slice:

**Inbound (parser must handle):** array of bulk strings. That's it. Every command from every real client looks like `*<n>\r\n$<n>\r\n<cmd>\r\n$<n>\r\n<arg>\r\n...`. Command args are never integers or simple strings on the wire — always bulk strings.

**Outbound (serializer must handle):** five types.
- Simple string — `+OK\r\n` for SET
- Bulk string — `$3\r\nbar\r\n` for GET hits
- Null bulk — `$-1\r\n` for GET miss
- Integer — `:1\r\n` for DEL count (and EXISTS later)
- Simple error — `-ERR ...\r\n`

RESP3 types (Map, Set, Push, Attribute, BigNumber, VerbatimString, Boolean, Double, BulkError) are out of scope. A real interviewer accepts "I scoped to RESP2 bulk-string arrays because that's what real clients send" — they will not accept an untested VerbatimString implementation.

Inline commands (telnet-style `PING\r\n` without RESP framing) are also out of scope. Reject anything that doesn't start with `*` as `-ERR`.

## Example frames

GET:

```
*2\r\n$3\r\nGET\r\n$5\r\npedro\r\n
```

SET:

```
*3\r\n$3\r\nSET\r\n$5\r\npedro\r\n$4\r\ngood\r\n
```

## The framing problem

Naive instinct: "read until `\r\n`, then parse." This breaks because bulk-string payloads can contain `\r\n` — e.g. `$6\r\nhe\r\nlo\r\n` is a valid 6-byte payload `he\r\nlo`. You can't find the end of a frame by scanning for `\r\n`; you have to parse the `$<len>` header and *count bytes*.

Slightly less naive: "wait until the buffer ends in `\r\n`, then parse." Also breaks. TCP delivers bytes in arbitrary chunks. A partial read can perfectly land right after an interior `\r\n` and look complete — e.g. the first chunk is `*2\r\n$3\r\nGET\r\n` (ends in `\r\n`!) but the key is still on its way.

**The only thing that knows whether a frame is complete is the parser**, because completeness depends on the `$<len>` counts and the `*<n>` array size. So the parser returns three states, not two:

- `Ok((value, rest))` — parsed one frame, here's what was left over
- `Err(malformed)` — bytes were invalid RESP
- `Incomplete` — ran out of bytes mid-parse, need more

The reader loop: try-parse → on `Incomplete`, read more bytes into the buffer, retry from the start. On `Ok`, dispatch the value and keep `rest` for the next round (TCP can deliver multiple commands in one read).

In other words, **the parser is the framer**. There's no separate "is this complete" pass.

## Parser shape

Free function over `&[u8]`, returning slices that borrow from the input:

```
fn next_value(buf: &[u8]) -> ParseResult<(RespValue, &[u8])>
```

Both outputs share the input's lifetime — elided, no annotations needed. Slices are pointer + length, no allocation. Owning the bytes (cloning into `Vec<u8>`) for every step is the wrong instinct; the lifetime story for one-input-many-outputs is the gentlest case in Rust.

Sigil-on-first-byte dispatch:

- `+` → simple string parser
- `-` → simple error parser
- `:` → integer parser
- `$` → bulk string parser (read length, then *count bytes*, then consume trailing `\r\n`)
- `*` → array parser (read length, then recurse `next_value` N times)

Arrays are recursive — an array element is itself any RESP type, parsed by the same function.

## Layering

Two-step pipeline:

```
bytes -> RespValue -> Command
```

- `RespValue` knows nothing about GET/SET. Just RESP types.
- `Command` knows nothing about `\r\n`. Just domain.
- Serializer is the inverse: `Reply -> bytes`. `RespValue` is reusable for the reply side (you'll need to *write* RESP, not just read it).

Easier to test each layer in isolation. Easier to talk about in an interview ("I separated framing from semantics so the parser doesn't need to learn new commands when I add EXISTS").

## Implementation decisions (running log)

Captured as the parser comes together. Not a spec — just what was chosen and why, so future-me doesn't re-litigate.

### `Value` enum shape

Only two variants for now: `Array(Vec<Value>)` and `BulkString(Vec<u8>)`. That's the entire inbound surface (commands are always arrays of bulk strings). Reply-side variants (`SimpleString`, `Integer`, `SimpleError`, null bulk) get added when the serializer lands — no point modeling them before they're parsed or emitted.

### `BulkString(Vec<u8>)`, not `String`

First pass was `String`. Switched to `Vec<u8>` to stay binary-safe — RESP bulk payloads are arbitrary bytes (could be a jpeg, could contain interior `\r\n`). `String` would force UTF-8 validation at parse time and reject valid frames. Cost of `Vec<u8>`: command dispatch has to `std::str::from_utf8` the command-name slice when matching `b"get"` / `b"set"` / `b"del"`, which is cheap (borrow, no allocation).

### Parser signature

```rust
fn parse_one(bytes: &[u8]) -> Result<(Value, &[u8]), ValueError>
```

- Free-standing (well, associated method on `Value`) — not a `TryFrom` impl, because `TryFrom` can't return the leftover slice. `TryFrom<&[u8]> for Value` stays as the outer entry point that parses exactly one whole frame.
- Borrows in, borrows out — no allocation for the leftover. The "rest of the buffer" is a sub-slice of the input, so it shares the input's lifetime (elided).
- `Result<_, ValueError>` instead of a three-state `Ok/Err/Incomplete` enum — incompleteness is just one variant of `ValueError` (`BytesLenMismatch`, etc.). Works fine for synchronous read-and-retry; if we move to streaming we may want a real `Incomplete` variant the caller can distinguish from malformed.

### Helper: `Crlf` trait on `[u8]`

`utils.rs` exposes two extension methods on `[u8]`:

- `is_crlf(&self) -> bool` — true iff the slice starts with `\r\n`.
- `split_crlf(&self) -> Option<(&[u8], &[u8])>` — find the first `\r\n`, return (before, after). `None` if no `\r\n` anywhere — i.e. incomplete header.

Both return borrows. The trait shape is just for the `bytes.split_crlf()` ergonomics; nothing else implements it.

### Length parsing without allocation

```rust
std::str::from_utf8(&sigil_len_str[1..])?.parse::<usize>()?
```

`from_utf8` returns `&str` — a view over the existing bytes, no allocation. `.parse::<usize>()` consumes the `&str`. Two error types collapsed into one `ParseLengthError` enum (`Utf8` + `ParseInt`) with `#[from]` conversions so `?` works.

Could roll a digit-by-digit accumulator (`n = n*10 + (b - b'0') as usize`) and skip the validation entirely — more interview-flex, shows what `parse` does under the hood. Not done yet, but on the table.

### `parse_bulk_string(bytes, len)`

Header is parsed by `parse_one` (which has already consumed `$<n>\r\n` via `split_crlf`). `parse_bulk_string` takes the bytes *after* the header and the pre-parsed length, returns the value and the leftover *after* the trailing `\r\n`.

Three things it must do (TODO: trailing `\r\n` consumption and validation still owed at time of writing):

1. Bounds-check: `bytes.len() >= len + 2` (payload + terminator), else incomplete.
2. Validate `&bytes[len..len+2] == b"\r\n"` — if the length lied, frame is malformed.
3. Return `&bytes[len+2..]` as the leftover, not `&bytes[len..]`.

### `parse_array(bytes, len)`

Stubbed. The plan: loop `len` times, each iteration calls `parse_one` on the current leftover, pushes the resulting `Value` into a `Vec`, threads the new leftover forward. Final return: `(Value::Array(vec), leftover)`. State lives on the call stack — no `Parser<Mode>` machinery needed for this scope.

### Considered and rejected: split-all-on-`\r\n`

Tempting one-liner: `bytes.split(|&b| ...)` to chop the whole frame into tokens. Forbidden — bulk-string payloads can contain `\r\n` (`$6\r\nhe\r\nlo\r\n` is a 6-byte payload `he\r\nlo`, valid), and split would shred it. The only correct framer respects length prefixes and counts bytes.

### Considered and rejected: `Parser<Mode>` type-state machine

Overkill. The state needed to parse one frame fits on the call stack via recursion. A type-state machine is what you reach for when streaming megabyte values chunk-by-chunk without materializing them, or when enforcing "you can't call `read_body` before `read_header`" at compile time. Not in scope.
