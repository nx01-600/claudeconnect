//! Wire types: the universal message envelope, the room ticket, and the join handshake.

use anyhow::{Context, anyhow};
use data_encoding::{BASE32_NOPAD, BASE64};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

/// Message kinds every client understands. `custom` + `topic` is the open extension point.
pub const KINDS: &[&str] = &[
    "text", "image", "context", "memory", "status", "warning", "change", "delegate", "result",
    "custom",
];

/// Who a session is: its iroh endpoint id plus a human name and the machine it runs on.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Member {
    pub session_id: String,
    pub name: String,
    pub pc_id: String,
    pub pc_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub name: String,
    /// Base64 file content.
    pub data: String,
}

impl Attachment {
    pub fn from_bytes(name: String, bytes: &[u8]) -> Self {
        Self { name, data: BASE64.encode(bytes) }
    }

    pub fn bytes(&self) -> anyhow::Result<Vec<u8>> {
        BASE64.decode(self.data.as_bytes()).context("bad attachment encoding")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub id: String,
    pub room: String,
    pub from: Member,
    /// Target session id; `None` means the whole room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub ts: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

/// Everything needed to join a room, pasted between people as one line of text.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Ticket {
    pub room: String,
    pub name: String,
    pub secret: String,
    pub host: EndpointAddr,
}

impl Ticket {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("ticket serializes");
        format!("cc1{}", BASE32_NOPAD.encode(&json).to_lowercase())
    }

    pub fn decode(s: &str) -> anyhow::Result<Self> {
        let body = s.trim().strip_prefix("cc1").ok_or_else(|| anyhow!("not a claudeconnect ticket"))?;
        let bytes = BASE32_NOPAD.decode(body.to_uppercase().as_bytes()).context("corrupted ticket")?;
        serde_json::from_slice(&bytes).context("corrupted ticket")
    }
}

/// First thing a dialer sends on a new connection.
#[derive(Serialize, Deserialize)]
pub struct Hello {
    pub room: String,
    pub secret: String,
    pub me: Member,
    pub addr: EndpointAddr,
}

/// The acceptor's answer: the room's current members so the joiner can connect to all of them.
#[derive(Serialize, Deserialize, Default)]
pub struct Welcome {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub room_name: String,
    #[serde(default)]
    pub members: Vec<(Member, EndpointAddr)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn ticket_round_trip() {
        let host = EndpointAddr::from(SecretKey::generate().public());
        let t = Ticket { room: "r1".into(), name: "demo".into(), secret: "s3".into(), host };
        let enc = t.encode();
        assert!(enc.starts_with("cc1"));
        assert_eq!(Ticket::decode(&format!("  {enc}\n")).unwrap(), t);
        assert!(Ticket::decode("hello").is_err());
    }

    #[test]
    fn envelope_round_trip() {
        let from = Member {
            session_id: "abc".into(),
            name: "nico".into(),
            pc_id: "p1".into(),
            pc_name: "DESK".into(),
        };
        let e = Envelope {
            id: "m1".into(),
            room: "r1".into(),
            from,
            to: None,
            ts: 1,
            kind: "image".into(),
            topic: None,
            text: "look".into(),
            files: vec![Attachment::from_bytes("a.png".into(), &[0, 1, 2, 255])],
            reply_to: None,
        };
        let back: Envelope = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.files[0].bytes().unwrap(), vec![0, 1, 2, 255]);
    }
}
