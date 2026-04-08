use libagent::channels::{create_channel, read_messages, send_message, Message};
use libagent::{ResourceLimits, Sandbox};

#[test]
fn channel_send_receive() {
    let s1 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let s2 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let chan_id = create_channel(s1.id(), s2.id());
    let msg = Message {
        from: s1.id(),
        to: s2.id(),
        payload: "hello".to_string(),
    };
    send_message(chan_id, msg.clone()).unwrap();
    let msgs = read_messages(chan_id).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].payload, "hello");
}
