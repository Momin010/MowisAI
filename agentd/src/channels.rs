use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

/// Structured messages passed over a channel between sandboxes.
#[derive(Debug, Clone)]
pub struct Message {
    pub from: u64,
    pub to: u64,
    pub payload: String,
}

/// Communication channel between agents.
pub struct Channel {
    pub id: u64,
    pub from: u64,
    pub to: u64,
}

impl Channel {
    pub fn new(id: u64, from: u64, to: u64) -> Self {
        Channel { id, from, to }
    }
}

// global channel registry and message store for prototyping
lazy_static::lazy_static! {
    static ref CHANNEL_STORE: Mutex<HashMap<u64, Channel>> = Mutex::new(HashMap::new());
    static ref MESSAGE_STORE: Mutex<HashMap<u64, Vec<Message>>> = Mutex::new(HashMap::new());
}
static CHANNEL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// create a channel from `from` sandbox to `to` sandbox. returns the new channel id.
pub fn create_channel(from: u64, to: u64) -> u64 {
    let id = CHANNEL_COUNTER.fetch_add(1, Ordering::SeqCst);
    let chan = Channel::new(id, from, to);
    CHANNEL_STORE.lock().unwrap().insert(id, chan);
    MESSAGE_STORE.lock().unwrap().insert(id, Vec::new());
    id
}

/// send a message on the given channel; errors if channel does not exist or
/// sender does not match.
pub fn send_message(channel_id: u64, msg: Message) -> anyhow::Result<()> {
    let store = CHANNEL_STORE.lock().unwrap();
    let chan = store
        .get(&channel_id)
        .ok_or_else(|| anyhow::anyhow!("channel {} not found", channel_id))?;
    if chan.from != msg.from {
        return Err(anyhow::anyhow!("sender mismatch"));
    }
    drop(store);
    MESSAGE_STORE
        .lock()
        .unwrap()
        .entry(channel_id)
        .or_default()
        .push(msg);
    Ok(())
}

/// read all messages on a channel (non-destructive).
pub fn read_messages(channel_id: u64) -> anyhow::Result<Vec<Message>> {
    let msgs = MESSAGE_STORE.lock().unwrap();
    let v = msgs
        .get(&channel_id)
        .ok_or_else(|| anyhow::anyhow!("channel {} not found", channel_id))?;
    Ok(v.clone())
}
