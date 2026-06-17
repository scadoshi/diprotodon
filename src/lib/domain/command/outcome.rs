#[derive(Debug)]
pub enum TtlOutcome {
    KeyNotFound,
    TtlNotFound,
    Some(u64),
}

#[derive(Debug)]
pub enum CommandOutcome {
    Value(Option<Vec<u8>>),
    Ok,
    Bool(bool),
    Ttl(TtlOutcome),
    Pong(Option<Vec<u8>>),
    Integer(i64),
}
