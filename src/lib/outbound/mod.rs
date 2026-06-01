//! Outbound layer — everything that turns a domain reply back into wire bytes.
//!
//! Currently just [`resp`], which carries the [`resp::reply::Reply`] enum and its
//! `write_to` serializer. Mirrors the structure of the inbound layer so the wire format
//! can be swapped in one place if it ever needs to.

pub mod resp;
