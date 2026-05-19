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
