//! The UDP socket (D1): non-blocking `std::net::UdpSocket`s, datagrams of
//! at most `MAX_DATAGRAM` both ways, arrivals stamped with the owner's
//! clock the moment they are read.
//!
//! **Both IP versions.** Bound to the IPv6 wildcard (`[::]`), it takes
//! IPv4 players too: where the OS makes that socket dual-stack (macOS and
//! Linux by default) IPv4 peers arrive as mapped addresses, which it reads
//! as plain IPv4; where it does not (Windows), a second socket on the same
//! port takes IPv4. Whichever socket a datagram came in on, it reads both;
//! it sends IPv4 from the IPv4 socket when it has one.
//!
//! **Receive buffer.** It asks the OS for `RECV_BUFFER` bytes (on macOS
//! and Linux; Windows keeps its default) and says what it got, so a reader
//! held up a moment drops nothing.
//!
//! Sending never blocks: a datagram the socket will not take now (its
//! buffer full) is dropped and counted, as the network would drop it; the
//! protocol above resends what matters. Receiving reads what has arrived;
//! a datagram longer than `MAX_DATAGRAM` is dropped whole. An error a
//! datagram socket reports for an earlier send (Windows turns an ICMP
//! "port unreachable" into a connection reset on the next receive) is not
//! a reason to stop: it is counted and reading goes on.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

use super::Clock;
use crate::{Datagram, MAX_DATAGRAM, Peer, Transport};

/// Receive errors in a row after which `recv` returns for now.
const MAX_ERRORS_IN_A_ROW: u32 = 16;
/// The receive buffer asked of the OS (bytes): a second of a busy host's
/// traffic.
pub const RECV_BUFFER: usize = 1 << 20;

/// What the socket did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UdpStats {
    pub sent: u64,
    pub received: u64,
    /// Datagrams not sent: to a peer that is not an address, too long, or
    /// refused by the socket.
    pub dropped: u64,
    /// Datagrams received longer than `MAX_DATAGRAM`.
    pub oversized: u64,
    /// Errors reported on receive and passed over.
    pub errors: u64,
}

/// A UDP socket (or an IPv6 and an IPv4 one on one port) as a `Transport`.
pub struct Udp {
    /// The bound socket first; an IPv4 one beside an IPv6 wildcard that is
    /// not dual-stack.
    sockets: Vec<UdpSocket>,
    local: Peer,
    clock: Clock,
    buffer: Vec<u8>,
    stats: UdpStats,
    /// The receive buffer the OS gave (bytes), where it was asked.
    recv_buffer: Option<usize>,
    /// The socket `recv` reads first next (they take turns).
    next: usize,
}

/// How a datagram to an address leaves.
fn mapped(to: SocketAddr) -> SocketAddr {
    match to {
        SocketAddr::V4(v4) => SocketAddr::new(v4.ip().to_ipv6_mapped().into(), v4.port()),
        v6 => v6,
    }
}

impl Udp {
    /// Binds `address` (port 0: any free port), non-blocking.
    pub fn bind(address: SocketAddr, clock: Clock) -> std::io::Result<Udp> {
        let first = UdpSocket::bind(address)?;
        let local = first.local_addr()?;
        let mut sockets = vec![first];
        // Where the IPv6 wildcard is not dual-stack, the port is free for
        // IPv4 too; where it is, binding again fails and one socket serves.
        if let SocketAddr::V6(v6) = address
            && v6.ip().is_unspecified()
            && let Ok(v4) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, local.port()))
        {
            sockets.push(v4);
        }
        let mut recv_buffer = None;
        for socket in &sockets {
            socket.set_nonblocking(true)?;
            recv_buffer = sys::grow_recv_buffer(socket, RECV_BUFFER).or(recv_buffer);
        }
        Ok(Udp {
            sockets,
            local: Peer::Addr(local),
            clock,
            // One byte more than a datagram may carry: a longer one fills
            // it and is known to be too long.
            buffer: vec![0; MAX_DATAGRAM + 1],
            stats: UdpStats::default(),
            recv_buffer,
            next: 0,
        })
    }

    /// The address it is bound to.
    pub fn address(&self) -> Option<SocketAddr> {
        match self.local {
            Peer::Addr(address) => Some(address),
            Peer::Local(_) | Peer::Steam(_) => None,
        }
    }

    /// Whether it takes IPv4 on a socket of its own beside IPv6.
    pub fn split_stack(&self) -> bool {
        self.sockets.len() > 1
    }

    /// The receive buffer the OS gave, where it was asked (bytes).
    pub fn recv_buffer(&self) -> Option<usize> {
        self.recv_buffer
    }

    pub fn stats(&self) -> UdpStats {
        self.stats
    }

    /// Waits until a datagram is ready to read on the bound socket or
    /// `timeout` passes (without reading it): a loop with nothing else to
    /// do sleeps here. One that came in on the IPv4 socket beside it waits
    /// for the timeout.
    pub fn wait(&self, timeout: Duration) {
        let socket = &self.sockets[0];
        if timeout.is_zero() || socket.set_nonblocking(false).is_err() {
            return;
        }
        let _ = socket.set_read_timeout(Some(timeout));
        let mut probe = [0u8; 1];
        let _ = socket.peek_from(&mut probe);
        let _ = socket.set_nonblocking(true);
    }

    /// The socket to send to `to` from, and the address as that socket
    /// takes it (IPv4 through a dual-stack IPv6 socket as a mapped
    /// address).
    fn route(&self, to: SocketAddr) -> (&UdpSocket, SocketAddr) {
        match (to, self.sockets.as_slice(), self.local) {
            (SocketAddr::V4(_), [_, v4], _) => (v4, to),
            (_, [only], Peer::Addr(SocketAddr::V6(_))) => (only, mapped(to)),
            _ => (&self.sockets[0], to),
        }
    }
}

impl Transport for Udp {
    fn local(&self) -> Peer {
        self.local
    }

    fn send(&mut self, to: Peer, bytes: &[u8]) {
        let Peer::Addr(address) = to else {
            self.stats.dropped += 1;
            return;
        };
        if bytes.len() > MAX_DATAGRAM {
            self.stats.dropped += 1;
            return;
        }
        let (socket, address) = self.route(address);
        match socket.send_to(bytes, address) {
            Ok(_) => self.stats.sent += 1,
            Err(_) => self.stats.dropped += 1,
        }
    }

    fn recv(&mut self) -> Option<Datagram> {
        let count = self.sockets.len();
        for turn in 0..count {
            let index = (self.next + turn) % count;
            if let Some(datagram) = self.recv_on(index) {
                self.next = (index + 1) % count;
                return Some(datagram);
            }
        }
        None
    }
}

impl Udp {
    /// The next datagram on socket `index`, its sender as plain IPv4 when
    /// it came mapped.
    fn recv_on(&mut self, index: usize) -> Option<Datagram> {
        let mut errors = 0;
        loop {
            match self.sockets[index].recv_from(&mut self.buffer) {
                Ok((len, from)) => {
                    if len > MAX_DATAGRAM {
                        self.stats.oversized += 1;
                        continue;
                    }
                    self.stats.received += 1;
                    return Some(Datagram {
                        from: Peer::Addr(SocketAddr::new(from.ip().to_canonical(), from.port())),
                        at: (self.clock)(),
                        bytes: self.buffer[..len].to_vec(),
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return None,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    // A reset or unreachable report for an earlier send:
                    // the socket is fine.
                    self.stats.errors += 1;
                    errors += 1;
                    if errors == MAX_ERRORS_IN_A_ROW {
                        return None;
                    }
                }
            }
        }
    }
}

/// The receive buffer, where the standard library has no word for it.
#[cfg(unix)]
mod sys {
    use std::net::UdpSocket;
    use std::os::fd::AsRawFd;

    /// Asks for `bytes` of receive buffer; what the OS gave.
    pub fn grow_recv_buffer(socket: &UdpSocket, bytes: usize) -> Option<usize> {
        let fd = socket.as_raw_fd();
        let size = libc::c_int::try_from(bytes).ok()?;
        let len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `fd` is this socket's, open while `socket` lives; the
        // option's value is a `c_int` and `len` its size.
        let set = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&raw const size).cast(),
                len,
            )
        };
        let mut given: libc::c_int = 0;
        let mut given_len = len;
        // SAFETY: as above; `given` and `given_len` are writable and sized
        // for a `c_int`.
        let got = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&raw mut given).cast(),
                &raw mut given_len,
            )
        };
        (set == 0 && got == 0)
            .then(|| usize::try_from(given).ok())
            .flatten()
    }
}

#[cfg(not(unix))]
mod sys {
    use std::net::UdpSocket;

    /// Windows keeps its default (the standard library has no word for
    /// it, and nothing here binds winsock).
    pub fn grow_recv_buffer(_socket: &UdpSocket, _bytes: usize) -> Option<usize> {
        None
    }
}
