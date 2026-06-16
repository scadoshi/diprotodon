//! The pub/sub command family and the outcomes its handlers produce.
//!
//! [`ChannelCommand`] is the parsed form of `SUBSCRIBE` / `UNSUBSCRIBE` / `PUBLISH`. It is
//! handled by the per-session router (it needs the session's id, sender, and the shared
//! registry), not by the cache service. The handler returns a [`ChannelCommandOutcome`],
//! which the inbound layer turns into the wire replies.

/// A parsed pub/sub command.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelCommand {
    /// Join one or more channels. An empty channel list is rejected at parse time.
    Subscribe {
        channel_ids: Vec<Vec<u8>>,
    },
    /// Leave the listed channels; an empty list means "every channel this session is in".
    Unsubscribe {
        channel_ids: Vec<Vec<u8>>,
    },
    /// Broadcast `message` to every subscriber of `channel_id`.
    Publish {
        channel_id: Vec<u8>,
        message: Vec<u8>,
    },
}

impl ChannelCommand {
    pub fn subscribe(channel_ids: impl Into<Vec<Vec<u8>>>) -> Self {
        Self::Subscribe {
            channel_ids: channel_ids.into(),
        }
    }

    pub fn unsubscribe(channel_ids: impl Into<Vec<Vec<u8>>>) -> Self {
        Self::Unsubscribe {
            channel_ids: channel_ids.into(),
        }
    }

    pub fn publish(channel_id: impl Into<Vec<u8>>, message: impl Into<Vec<u8>>) -> Self {
        Self::Publish {
            channel_id: channel_id.into(),
            message: message.into(),
        }
    }
}

/// One channel's entry in a subscribe/unsubscribe outcome: the channel acted on and the
/// session's *cumulative* subscription count immediately after that action (Redis reports a
/// running total, so it climbs across a multi-channel `SUBSCRIBE` and descends on `UNSUBSCRIBE`).
///
/// `channel_id` is `None` only for the one edge case Redis defines: a no-arg `UNSUBSCRIBE`
/// issued while subscribed to nothing, which acks with a null channel and a count of 0.
#[derive(Debug, Clone)]
pub struct SubUnsubInnerEntry {
    pub channel_id: Option<Vec<u8>>,
    pub subscription_count: usize,
}

impl SubUnsubInnerEntry {
    /// An entry naming a concrete channel.
    pub fn new(channel_id: impl Into<Vec<u8>>, subscription_count: usize) -> Self {
        Self {
            channel_id: Some(channel_id.into()),
            subscription_count,
        }
    }

    /// The null-channel entry — a no-arg `UNSUBSCRIBE` while subscribed to no channels.
    pub fn new_null(subscription_count: usize) -> Self {
        Self {
            channel_id: None,
            subscription_count,
        }
    }
}

/// The result of handling a [`ChannelCommand`], pre-wire. Subscribe/unsubscribe carry one
/// entry per channel acted on; publish carries the number of subscribers reached.
#[derive(Debug, Clone)]
pub enum ChannelCommandOutcome {
    Subscribe { inner: Vec<SubUnsubInnerEntry> },
    Unsubscribe { inner: Vec<SubUnsubInnerEntry> },
    Publish { sent_count: u32 },
}
