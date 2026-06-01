//! RESP parsing — three layers stacked bottom-up:
//!
//! - [`crlf`] — byte-slice utilities for finding the `\r\n` terminator that separates
//!   RESP fields. The lowest layer; everything above relies on it.
//! - [`frame`] — the parser/framer. Turns `&[u8]` into [`frame::Frame`] values (arrays
//!   and bulk strings) and tells callers when more bytes are needed via
//!   [`frame::FrameError::Incomplete`].
//! - [`command`] — `TryFrom<Frame>` for [`crate::domain::command::Command`]: validates
//!   verb + arity + argument shape and lifts a parsed `Frame` into a domain command.

pub mod command;
pub mod crlf;
pub mod frame;
