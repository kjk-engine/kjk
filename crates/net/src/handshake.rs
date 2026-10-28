//! Joining: hello, challenge, connect, then accept or a typed refusal
//! (design section 9, D11). Sans-IO, like the link: both sides take
//! datagrams with a time and say what to send.
//!
//! 1. The client sends **Hello** (padded to `MAX_DATAGRAM`) with a random
//!    nonce the caller supplies.
//! 2. The server answers with a **Challenge**: a cookie, keyed BLAKE3
//!    under the server's secret over the client's address, the nonce and
//!    an expiry `COOKIE_LIFETIME_US` ahead. The server keeps nothing for a
//!    Hello, so a flood of spoofed Hellos costs it a hash each and gets
//!    the spoofed victim a reply smaller than the flood.
//! 3. The client sends **Connect** (padded): the cookie, the nonce, the
//!    expiry, its proof of the join password if it has one, and its
//!    `Credentials` (build, game-data fingerprints, name, reconnect token).
//! 4. The server checks the cookie (it proves the client receives at the
//!    address it sends from), then its expiry, then the proof when it has
//!    a join password (`Refusal::Password` without the right one), then
//!    asks the caller's acceptance check (seats, `Credentials::check`), and
//!    answers **Accept** with the session id or **Deny** with a `Refusal`.
//!
//! **The join password.** A host may have one (`JoinKey`); with none, any
//! client of the same build and data joins. The password never travels:
//! both sides derive a key from it, and the client sends a keyed BLAKE3 of
//! the cookie, nonce and expiry under that key. The cookie is the host's,
//! made for this address and attempt, so a proof is good for nothing
//! else; the host compares it in constant time. Wrong proofs have limits
//! of their own, apart from the Hellos': `GUESS_RATE` a second (and a
//! burst of as many) per sending host — an IPv4 address, an IPv6 /64,
//! whatever its ports — and `GUESS_BUDGET` a second from everyone
//! together. One past either is dropped unanswered; a right proof is
//! never limited. So online guessing goes at `GUESS_BUDGET` a second at
//! most however many ports or hosts guess, and one host's guessing
//! leaves the others' wrong-password answers alone. What this does not
//! stop: an eavesdropper who saw a proof can try passwords against it
//! offline, so a long one is better. And a seat's reconnect token travels
//! in clear (in `Credentials` and the welcome): one who sees it can take
//! that seat once its player has been quiet `session::TAKEOVER_QUIET_US`
//! (the host refuses it while the player is heard), so the password
//! keeps strangers out, not someone already watching the traffic.
//!
//! The session id is keyed BLAKE3 too, over the address, nonce and
//! expiry: unguessable without the secret, and the same for a Connect
//! sent again after a lost Accept, so a retry is answered with the same
//! session (the caller sees `Accepted` twice for one session and keeps
//! one).
//!
//! Hellos are rate limited to `HELLO_RATE` per second per sending host
//! (as above: the address without its port, an IPv6 /64; a burst of as
//! many), in a fixed table of `RATE_BUCKETS` cells indexed by a keyed
//! hash of the host: constant memory and time whatever the flood, and a
//! sender rotating its ports keeps to one cell. The price: two hosts may
//! (rarely) share a cell and its rate, players behind one NAT share
//! theirs, and senders forging many source addresses can still crowd
//! the table (they get no cookies for it). A request of another protocol
//! version is answered with `Refusal::Protocol`; every handshake packet
//! starts with the same frozen prefix in every version, so the client
//! reads it from the reply's version alone.
//!
//! The client resends its current request every `RETRY_US` and gives up
//! with `Refusal::Timeout` after `GIVE_UP_US`.

use crate::wire::{COOKIE_LEN, Handshake, PROOF_LEN, PROTOCOL_VERSION, WireError, peek_prefix};
pub use crate::wire::{Credentials, GameFingerprint, Refusal};
use crate::{MAX_DATAGRAM, NetTime, Peer};
use std::net::{IpAddr, SocketAddr};

/// How long a cookie is good for.
pub const COOKIE_LIFETIME_US: u64 = 5_000_000;
/// Hellos per second per sending host (and the burst).
pub const HELLO_RATE: u64 = 8;
/// Cells of the Hello rate limiter.
pub const RATE_BUCKETS: usize = 4096;
/// Wrong join passwords per second per sending host (and the burst).
pub const GUESS_RATE: u64 = 4;
/// Wrong join passwords per second from all senders together (and the
/// burst).
pub const GUESS_BUDGET: u64 = 32;
/// Cells of the per-host wrong-password limiter.
pub const GUESS_BUCKETS: usize = 1024;
/// How often the client sends its request again.
pub const RETRY_US: u64 = 250_000;
/// When the client gives up.
pub const GIVE_UP_US: u64 = 5_000_000;

/// The key a join password makes: what the host checks proofs with and
/// a client makes them with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JoinKey([u8; 32]);

impl JoinKey {
    /// The key of `password`.
    pub fn new(password: &str) -> JoinKey {
        JoinKey(blake3::derive_key(
            "kjk-net join password",
            password.as_bytes(),
        ))
    }

    /// The proof of this key for one Connect: over the host's cookie, the
    /// nonce and the expiry.
    pub fn proof(&self, cookie: &[u8; COOKIE_LEN], nonce: u64, expiry: u64) -> [u8; PROOF_LEN] {
        let mut hasher = blake3::Hasher::new_keyed(&self.0);
        hasher.update(b"kjk-net join proof");
        hasher.update(cookie);
        hasher.update(&nonce.to_le_bytes());
        hasher.update(&expiry.to_le_bytes());
        let mut proof = [0; PROOF_LEN];
        proof.copy_from_slice(&hasher.finalize().as_bytes()[..PROOF_LEN]);
        proof
    }
}

impl std::fmt::Debug for JoinKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JoinKey(..)")
    }
}

/// What the server made of a datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A Hello, answered with a Challenge.
    Challenged,
    /// A Connect accepted: the session to open for the sender.
    Accepted {
        session: u64,
        credentials: Credentials,
    },
    /// A request refused with a Deny.
    Refused(Refusal),
    /// Nothing sent back.
    Dropped(Dropped),
}

/// Why the server sent nothing back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    /// Not a well-formed handshake packet (short requests included).
    Malformed(WireError),
    /// A reply, not a request.
    NotARequest,
    /// More Hellos from the sending host than `HELLO_RATE` allows, or a
    /// wrong join password past `GUESS_RATE` or `GUESS_BUDGET`.
    RateLimited,
    /// A cookie this server did not make for this address, nonce and
    /// expiry.
    Cookie,
    /// A cookie past its expiry.
    Expired,
}

/// The server's answer to one datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The datagram to send back to the sender, never longer than what it
    /// sent.
    pub reply: Option<Vec<u8>>,
    pub event: Event,
}

/// The server side: stateless per client, but for the rate cells.
pub struct Server {
    secret: [u8; 32],
    /// The join password's key, if the host has one.
    key: Option<JoinKey>,
    /// Per cell, the Hello limiter's theoretical arrival time (GCRA).
    cells: Vec<u64>,
    /// The same for wrong join passwords: per cell, and for everyone.
    guesses: Vec<u64>,
    all_guesses: u64,
}

impl Server {
    /// A server whose cookies and session ids are keyed by `secret` (made
    /// once at start from the OS's randomness, by the caller), letting in
    /// only clients that prove `key` when there is one.
    pub fn new(secret: [u8; 32], key: Option<JoinKey>) -> Server {
        Server {
            secret,
            key,
            cells: vec![0; RATE_BUCKETS],
            guesses: vec![0; GUESS_BUCKETS],
            all_guesses: 0,
        }
    }

    /// Takes a handshake datagram from `from` at `now`. `accept` decides a
    /// verified Connect: the sender's address, the session id it would
    /// get, and its credentials.
    pub fn receive(
        &mut self,
        now: NetTime,
        from: Peer,
        bytes: &[u8],
        accept: impl FnOnce(Peer, u64, &Credentials) -> Result<(), Refusal>,
    ) -> Outcome {
        let dropped = |why| Outcome {
            reply: None,
            event: Event::Dropped(why),
        };
        let prefix = match peek_prefix(bytes) {
            Ok(prefix) => prefix,
            Err(error) => return dropped(Dropped::Malformed(error)),
        };
        if !prefix.is_request() {
            return dropped(Dropped::NotARequest);
        }
        if bytes.len() != MAX_DATAGRAM {
            return dropped(Dropped::Malformed(WireError::Unpadded { len: bytes.len() }));
        }
        if prefix.is_hello() && !self.allow_hello(now, from) {
            return dropped(Dropped::RateLimited);
        }
        let nonce = prefix.nonce;
        if prefix.version != PROTOCOL_VERSION {
            let refusal = Refusal::Protocol {
                host: PROTOCOL_VERSION,
                here: prefix.version,
            };
            return deny(nonce, refusal);
        }
        match Handshake::decode(bytes) {
            Ok(Handshake::Hello { nonce }) => {
                let expiry = now.0.saturating_add(COOKIE_LIFETIME_US);
                let challenge = Handshake::Challenge {
                    nonce,
                    expiry,
                    cookie: self.cookie(from, nonce, expiry),
                };
                reply(&challenge, Event::Challenged)
            }
            Ok(Handshake::Connect {
                nonce,
                expiry,
                cookie,
                proof,
                credentials,
            }) => {
                if !same(&cookie, &self.cookie(from, nonce, expiry)) {
                    return dropped(Dropped::Cookie);
                }
                if now.0 > expiry {
                    return dropped(Dropped::Expired);
                }
                if let Some(key) = &self.key {
                    let right = key.proof(&cookie, nonce, expiry);
                    if !proof.is_some_and(|proof| same(&proof, &right)) {
                        if !self.allow_guess(now, from) {
                            return dropped(Dropped::RateLimited);
                        }
                        let given = proof.is_some();
                        return deny(nonce, Refusal::Password { given });
                    }
                }
                let session = self.session(from, nonce, expiry);
                match accept(from, session, &credentials) {
                    Ok(()) => reply(
                        &Handshake::Accept { nonce, session },
                        Event::Accepted {
                            session,
                            credentials,
                        },
                    ),
                    Err(refusal) => deny(nonce, refusal),
                }
            }
            Ok(_) => dropped(Dropped::NotARequest),
            Err(error) => dropped(Dropped::Malformed(error)),
        }
    }

    /// A Hello from `from`'s host, if its cell allows one.
    fn allow_hello(&mut self, now: NetTime, from: Peer) -> bool {
        let cell = self.cell(b"kjk-net hello cell", from, RATE_BUCKETS);
        let Some(next) = gcra(self.cells[cell], now, HELLO_RATE) else {
            return false;
        };
        self.cells[cell] = next;
        true
    }

    /// A wrong join password from `from`'s host, if both its cell and the
    /// budget for everyone allow one (neither is spent otherwise).
    fn allow_guess(&mut self, now: NetTime, from: Peer) -> bool {
        let cell = self.cell(b"kjk-net guess cell", from, GUESS_BUCKETS);
        let host = gcra(self.guesses[cell], now, GUESS_RATE);
        let all = gcra(self.all_guesses, now, GUESS_BUDGET);
        let (Some(host), Some(all)) = (host, all) else {
            return false;
        };
        self.guesses[cell] = host;
        self.all_guesses = all;
        true
    }

    fn keyed(&self, domain: &[u8], peer: Peer) -> blake3::Hasher {
        let mut hasher = blake3::Hasher::new_keyed(&self.secret);
        hasher.update(domain);
        hash_peer(&mut hasher, peer);
        hasher
    }

    /// The cell of `peer`'s host (not its port) in a table of `cells`.
    fn cell(&self, domain: &[u8], peer: Peer, cells: usize) -> usize {
        let mut hasher = blake3::Hasher::new_keyed(&self.secret);
        hasher.update(domain);
        hash_host(&mut hasher, peer);
        let hash = hasher.finalize();
        let bytes: [u8; 8] = hash.as_bytes()[..8].try_into().unwrap_or([0; 8]);
        (u64::from_le_bytes(bytes) % cells as u64) as usize
    }

    /// The cookie for `peer`, `nonce` and `expiry`.
    pub fn cookie(&self, peer: Peer, nonce: u64, expiry: u64) -> [u8; COOKIE_LEN] {
        let mut hasher = self.keyed(b"kjk-net cookie", peer);
        hasher.update(&nonce.to_le_bytes());
        hasher.update(&expiry.to_le_bytes());
        let mut cookie = [0; COOKIE_LEN];
        cookie.copy_from_slice(&hasher.finalize().as_bytes()[..COOKIE_LEN]);
        cookie
    }

    /// The session id for a verified Connect (never 0).
    fn session(&self, peer: Peer, nonce: u64, expiry: u64) -> u64 {
        let mut hasher = self.keyed(b"kjk-net session", peer);
        hasher.update(&nonce.to_le_bytes());
        hasher.update(&expiry.to_le_bytes());
        let bytes: [u8; 8] = hasher.finalize().as_bytes()[..8]
            .try_into()
            .unwrap_or([0; 8]);
        u64::from_le_bytes(bytes).max(1)
    }
}

fn reply(packet: &Handshake, event: Event) -> Outcome {
    Outcome {
        reply: packet.encode().ok(),
        event,
    }
}

fn deny(nonce: u64, refusal: Refusal) -> Outcome {
    let packet = Handshake::Deny {
        nonce,
        refusal: refusal.clone(),
    };
    reply(&packet, Event::Refused(refusal))
}

/// GCRA over one cell whose theoretical arrival time is `tat`: at most
/// `rate` at once, then one per `1/rate` s. The cell's next time if one
/// more is allowed at `now`.
fn gcra(tat: u64, now: NetTime, rate: u64) -> Option<u64> {
    let interval = 1_000_000 / rate;
    let tat = tat.max(now.0);
    (tat - now.0 <= interval * (rate - 1)).then_some(tat + interval)
}

/// Compares two cookies or proofs in time independent of where they
/// differ.
fn same<const N: usize>(a: &[u8; N], b: &[u8; N]) -> bool {
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// Feeds an address's bytes to a hash: a tag, then the address.
fn hash_peer(hasher: &mut blake3::Hasher, peer: Peer) {
    match peer {
        Peer::Addr(SocketAddr::V4(addr)) => {
            hasher.update(&[4]);
            hasher.update(&addr.ip().octets());
            hasher.update(&addr.port().to_le_bytes());
        }
        Peer::Addr(SocketAddr::V6(addr)) => {
            hasher.update(&[6]);
            hasher.update(&addr.ip().octets());
            hasher.update(&addr.port().to_le_bytes());
        }
        Peer::Local(index) => {
            hasher.update(&[0]);
            hasher.update(&index.to_le_bytes());
        }
        Peer::Steam(id) => {
            hasher.update(&[7]);
            hasher.update(&id.to_le_bytes());
        }
    }
}

/// Feeds the sending host of an address to a hash, for the rate limits:
/// an IPv4 address (an IPv4-mapped IPv6 one as that), an IPv6 address's
/// /64 (what one subscriber is usually given), never the port; a Steam ID
/// whole.
fn hash_host(hasher: &mut blake3::Hasher, peer: Peer) {
    match peer {
        Peer::Addr(addr) => match addr.ip() {
            IpAddr::V4(ip) => {
                hasher.update(&[4]);
                hasher.update(&ip.octets());
            }
            IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
                Some(ip) => {
                    hasher.update(&[4]);
                    hasher.update(&ip.octets());
                }
                None => {
                    hasher.update(&[6]);
                    hasher.update(&ip.octets()[..8]);
                }
            },
        },
        Peer::Local(index) => {
            hasher.update(&[0]);
            hasher.update(&index.to_le_bytes());
        }
        Peer::Steam(id) => {
            hasher.update(&[7]);
            hasher.update(&id.to_le_bytes());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Hello,
    Connect {
        expiry: u64,
        cookie: [u8; COOKIE_LEN],
    },
    Done(Result<u64, Refusal>),
}

/// The client side: one attempt to join `server`.
pub struct Client {
    server: Peer,
    nonce: u64,
    credentials: Credentials,
    key: Option<JoinKey>,
    started: NetTime,
    phase: Phase,
    last_sent: Option<NetTime>,
    sends: u32,
    rtt: Option<u64>,
}

impl Client {
    /// An attempt starting at `now`, with a random `nonce` from the
    /// caller, proving `key` if the host asks for a password. Fails if the
    /// credentials cannot be sent (over the limits, or longer than a
    /// datagram).
    pub fn new(
        now: NetTime,
        server: Peer,
        nonce: u64,
        credentials: Credentials,
        key: Option<JoinKey>,
    ) -> Result<Client, WireError> {
        Handshake::Connect {
            nonce,
            expiry: 0,
            cookie: [0; COOKIE_LEN],
            proof: key.map(|_| [0; PROOF_LEN]),
            credentials: credentials.clone(),
        }
        .encode()?;
        Ok(Client {
            server,
            nonce,
            credentials,
            key,
            started: now,
            phase: Phase::Hello,
            last_sent: None,
            sends: 0,
            rtt: None,
        })
    }

    /// The server this attempt is for.
    pub fn server(&self) -> Peer {
        self.server
    }

    /// The result, once there is one: the session id, or why not.
    pub fn outcome(&self) -> Option<&Result<u64, Refusal>> {
        match &self.phase {
            Phase::Done(result) => Some(result),
            _ => None,
        }
    }

    /// The last round trip from a request to its reply (an over-estimate
    /// when the reply answers an earlier copy of the request).
    pub fn rtt(&self) -> Option<u64> {
        self.rtt
    }

    /// Requests sent so far.
    pub fn sends(&self) -> u32 {
        self.sends
    }

    /// Holds the next request until `RETRY_US` after `now` (an attempt
    /// that follows a refusal worth trying again).
    pub fn pause(&mut self, now: NetTime) {
        self.last_sent = Some(now);
    }

    /// The request to send at `now`, if one is due.
    pub fn poll_transmit(&mut self, now: NetTime) -> Option<Vec<u8>> {
        if matches!(self.phase, Phase::Done(_)) {
            return None;
        }
        if now.since(self.started) >= GIVE_UP_US {
            self.phase = Phase::Done(Err(Refusal::Timeout));
            return None;
        }
        if self
            .last_sent
            .is_some_and(|sent| now.since(sent) < RETRY_US)
        {
            return None;
        }
        let packet = match &self.phase {
            Phase::Hello => Handshake::Hello { nonce: self.nonce },
            Phase::Connect { expiry, cookie } => Handshake::Connect {
                nonce: self.nonce,
                expiry: *expiry,
                cookie: *cookie,
                proof: self.key.map(|key| key.proof(cookie, self.nonce, *expiry)),
                credentials: self.credentials.clone(),
            },
            Phase::Done(_) => return None,
        };
        let bytes = packet.encode().ok()?;
        self.last_sent = Some(now);
        self.sends += 1;
        Some(bytes)
    }

    /// When `poll_transmit` next has something to do.
    pub fn next_deadline(&self) -> Option<NetTime> {
        if matches!(self.phase, Phase::Done(_)) {
            return None;
        }
        let retry = self
            .last_sent
            .map_or(self.started, |sent| sent.plus(RETRY_US));
        Some(retry.min(self.started.plus(GIVE_UP_US)))
    }

    /// Takes a datagram from `from` at `now`. Anything not from the
    /// server, not for this attempt's nonce, or not expected now is
    /// ignored.
    pub fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        if from != self.server || matches!(self.phase, Phase::Done(_)) {
            return;
        }
        let Ok(prefix) = peek_prefix(bytes) else {
            return;
        };
        if prefix.nonce != self.nonce || prefix.is_request() {
            return;
        }
        let rtt = self.last_sent.map(|sent| now.since(sent));
        if prefix.version != PROTOCOL_VERSION {
            self.rtt = rtt;
            self.phase = Phase::Done(Err(Refusal::Protocol {
                host: prefix.version,
                here: PROTOCOL_VERSION,
            }));
            return;
        }
        match (Handshake::decode(bytes), &self.phase) {
            (Ok(Handshake::Challenge { expiry, cookie, .. }), Phase::Hello) => {
                self.phase = Phase::Connect { expiry, cookie };
                self.last_sent = None;
            }
            (Ok(Handshake::Accept { session, .. }), Phase::Connect { .. }) => {
                self.phase = Phase::Done(Ok(session));
            }
            (Ok(Handshake::Deny { refusal, .. }), _) => {
                self.phase = Phase::Done(Err(refusal));
            }
            _ => return,
        }
        self.rtt = rtt;
    }
}
