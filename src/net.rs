//! Rooms over iroh: every member keeps one direct QUIC connection to every other member.
// ponytail: full mesh is O(n²) connections; switch to iroh-gossip if rooms grow past ~10 sessions.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use tokio::sync::{Mutex, mpsc};

use crate::envelope::{Envelope, Hello, Member, Ticket, Welcome};
use crate::store::{self, RoomRecord};

const ALPN: &[u8] = b"claudeconnect/1";
/// Envelopes carry files inline, so allow the 25 MB file cap plus base64 overhead.
pub const MAX_FRAME: usize = 40 << 20;

pub struct Peer {
    pub member: Member,
    pub addr: EndpointAddr,
    pub conn: Connection,
    pub status: String,
}

pub struct Room {
    pub name: String,
    pub secret: String,
    pub peers: HashMap<String, Peer>,
}

pub enum Event {
    Message(Envelope),
    Joined { room: String, member: Member },
    Left { room: String, member: Member },
}

#[derive(Clone)]
pub struct Net {
    pub ep: Endpoint,
    pub me: Member,
    pub rooms: Arc<Mutex<HashMap<String, Room>>>,
    events: mpsc::UnboundedSender<Event>,
}

impl Net {
    pub async fn start(key: SecretKey, me: Member) -> anyhow::Result<(Net, mpsc::UnboundedReceiver<Event>)> {
        let ep = Endpoint::builder(presets::N0).secret_key(key).alpns(vec![ALPN.to_vec()]).bind().await?;
        let (tx, rx) = mpsc::unbounded_channel();
        let net = Net { ep, me, rooms: Default::default(), events: tx };
        let acc = net.clone();
        tokio::spawn(async move {
            while let Some(incoming) = acc.ep.accept().await {
                let acc = acc.clone();
                tokio::spawn(async move {
                    if let Ok(conn) = incoming.await {
                        let _ = acc.on_incoming(conn).await;
                    }
                });
            }
        });
        Ok((net, rx))
    }

    pub fn link(conn: &Connection) -> &'static str {
        if conn.paths().iter().any(|p| p.is_ip()) { "direct" } else { "relay" }
    }

    pub async fn create(&self, name: &str) -> anyhow::Result<Ticket> {
        self.ep.online().await;
        let id = store::random_hex(8);
        let secret = store::random_hex(16);
        self.rooms.lock().await.insert(id.clone(), Room { name: name.into(), secret: secret.clone(), peers: HashMap::new() });
        self.remember(&id).await;
        Ok(Ticket { room: id, name: name.into(), secret, host: self.ep.addr() })
    }

    pub async fn ticket(&self, room: &str) -> anyhow::Result<Ticket> {
        let rooms = self.rooms.lock().await;
        let r = rooms.get(room).ok_or_else(|| anyhow!("not in room {room}"))?;
        Ok(Ticket { room: room.into(), name: r.name.clone(), secret: r.secret.clone(), host: self.ep.addr() })
    }

    /// Join through any reachable member, then connect to everyone else it reports.
    pub async fn join(&self, room: &str, name: &str, secret: &str, via: Vec<EndpointAddr>) -> anyhow::Result<String> {
        self.ep.online().await;
        self.rooms.lock().await.entry(room.into()).or_insert(Room { name: name.into(), secret: secret.into(), peers: HashMap::new() });
        let mut last_err = anyhow!("no members to contact");
        let mut members = None;
        for addr in via {
            match self.dial(room, secret, addr).await {
                Ok(m) => {
                    members = Some(m);
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        let Some(members) = members else {
            self.remember(room).await;
            return Err(last_err);
        };
        for (m, addr) in members {
            let known = self.rooms.lock().await.get(room).is_some_and(|r| r.peers.contains_key(&m.session_id));
            if m.session_id != self.me.session_id && !known {
                let _ = self.dial(room, secret, addr).await;
            }
        }
        self.remember(room).await;
        let n = self.rooms.lock().await.get(room).map_or(0, |r| r.peers.len());
        Ok(format!("joined room {name} ({room}) with {n} other session(s)"))
    }

    pub async fn leave(&self, room: &str) -> anyhow::Result<()> {
        self.remember(room).await;
        let r = self.rooms.lock().await.remove(room).ok_or_else(|| anyhow!("not in room {room}"))?;
        for p in r.peers.values() {
            p.conn.close(0u32.into(), b"left");
        }
        Ok(())
    }

    /// Send to one session (`to`) or the whole room. Returns how many sessions got it.
    pub async fn send(&self, env: &Envelope) -> anyhow::Result<usize> {
        let targets: Vec<Connection> = {
            let rooms = self.rooms.lock().await;
            let r = rooms.get(&env.room).ok_or_else(|| anyhow!("not in room {}", env.room))?;
            r.peers.values().filter(|p| env.to.as_ref().is_none_or(|t| *t == p.member.session_id)).map(|p| p.conn.clone()).collect()
        };
        let bytes = serde_json::to_vec(env)?;
        let mut sent = 0;
        for conn in targets {
            let ok = async {
                let mut s = conn.open_uni().await?;
                s.write_all(&bytes).await?;
                s.finish()?;
                anyhow::Ok(())
            };
            if ok.await.is_ok() {
                sent += 1;
            }
        }
        Ok(sent)
    }

    async fn dial(&self, room: &str, secret: &str, addr: EndpointAddr) -> anyhow::Result<Vec<(Member, EndpointAddr)>> {
        let conn = self.ep.connect(addr.clone(), ALPN).await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        let hello = Hello { room: room.into(), secret: secret.into(), me: self.me.clone(), addr: self.ep.addr() };
        send.write_all(&serde_json::to_vec(&hello)?).await?;
        send.finish()?;
        let welcome: Welcome = serde_json::from_slice(&recv.read_to_end(1 << 20).await?)?;
        if !welcome.ok {
            anyhow::bail!(welcome.error.unwrap_or_else(|| "rejected".into()));
        }
        let member = welcome
            .members
            .iter()
            .find(|(m, _)| m.session_id == conn.remote_id().to_string())
            .map(|(m, _)| m.clone())
            .context("host did not introduce itself")?;
        self.add_peer(room, member, addr, conn).await;
        Ok(welcome.members)
    }

    async fn on_incoming(&self, conn: Connection) -> anyhow::Result<()> {
        let (mut send, mut recv) = conn.accept_bi().await?;
        let hello: Hello = serde_json::from_slice(&recv.read_to_end(1 << 20).await?)?;
        let mut welcome = Welcome::default();
        {
            let rooms = self.rooms.lock().await;
            match rooms.get(&hello.room) {
                Some(r) if r.secret == hello.secret && hello.me.session_id == conn.remote_id().to_string() => {
                    welcome.ok = true;
                    welcome.room_name = r.name.clone();
                    welcome.members.push((self.me.clone(), self.ep.addr()));
                    welcome.members.extend(r.peers.values().map(|p| (p.member.clone(), p.addr.clone())));
                }
                _ => welcome.error = Some("unknown room or wrong secret".into()),
            }
        }
        send.write_all(&serde_json::to_vec(&welcome)?).await?;
        send.finish()?;
        if welcome.ok {
            self.add_peer(&hello.room, hello.me, hello.addr, conn).await;
            self.remember(&hello.room).await;
        } else {
            // Give the answer time to arrive before dropping the connection.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), conn.closed()).await;
        }
        Ok(())
    }

    async fn add_peer(&self, room: &str, member: Member, addr: EndpointAddr, conn: Connection) {
        let replaced = {
            let mut rooms = self.rooms.lock().await;
            let Some(r) = rooms.get_mut(room) else { return };
            r.peers.insert(member.session_id.clone(), Peer { member: member.clone(), addr, conn: conn.clone(), status: String::new() }).is_some()
        };
        if !replaced {
            let _ = self.events.send(Event::Joined { room: room.into(), member: member.clone() });
        }
        let net = self.clone();
        let room = room.to_string();
        tokio::spawn(async move {
            while let Ok(mut recv) = conn.accept_uni().await {
                let Ok(bytes) = recv.read_to_end(MAX_FRAME).await else { continue };
                let Ok(env) = serde_json::from_slice::<Envelope>(&bytes) else { continue };
                // Never trust the claimed sender or room: the connection says who it is.
                if env.from.session_id != member.session_id || env.room != room {
                    continue;
                }
                if env.kind == "status" {
                    if let Some(p) = net.rooms.lock().await.get_mut(&room).and_then(|r| r.peers.get_mut(&member.session_id)) {
                        p.status = env.text.clone();
                    }
                }
                let _ = net.events.send(Event::Message(env));
            }
            // Only report a departure if this is still the live connection for that member.
            let mut rooms = net.rooms.lock().await;
            if let Some(r) = rooms.get_mut(&room) {
                if r.peers.get(&member.session_id).is_some_and(|p| p.conn.stable_id() == conn.stable_id()) {
                    r.peers.remove(&member.session_id);
                    let _ = net.events.send(Event::Left { room, member });
                }
            }
        });
    }

    async fn remember(&self, room: &str) {
        let rooms = self.rooms.lock().await;
        let Some(r) = rooms.get(room) else { return };
        let mut members: Vec<_> = r.peers.values().map(|p| (p.member.clone(), p.addr.clone())).collect();
        // Keep members we knew before but who are offline now, so rejoin can still try them.
        if let Some(old) = store::load_rooms().remove(room) {
            for (m, a) in old.members {
                if !members.iter().any(|(x, _)| x.session_id == m.session_id) {
                    members.push((m, a));
                }
            }
        }
        let rec = RoomRecord { name: r.name.clone(), secret: r.secret.clone(), members, last_seen: store::now(), session: self.me.name.clone() };
        let _ = store::save_room(room, rec);
    }
}
