//! The UDP socket (D1): non-blocking `std::net::UdpSocket`s, datagrams of
//! at most `MAX_DATAGRAM` both ways, arrivals stamped with the owner's
//! clock the moment they are read.
//!
//! **Both IP versions.** Bound to the IPv6 wildcard (`[::]`), it takes
//! IPv4 players too: where the OS makes that socket dual-stack (macOS and
//! Linux by default) IPv4 peers arrive as mapped addresses, which it reads
//! as plain IPv4; where it does not (Windows), a second socket on the same
//! port takes IPv4. Whichever socket a datagram came in on, it reads both;
//! it sends IPv4 from the IPv4 socket when it has one. On Windows, an IPv4
//! socket the OS refuses (another program holds the port for IPv4) is told
//! (`ipv4_refused`), since no IPv4 player can reach the host then. A
//! machine with IPv6 turned off binds the IPv4 wildcard alone.
//!
//! **Buffers.** It asks the OS for `RECV_BUFFER` receive and
//! `SEND_BUFFER` send bytes (both Unix and Windows, or larger buffers when
//! hosting many seats) and says what it got, so a reader or sender held up
//! a moment drops nothing.
//!
//! **Wakeups.** `Udp::wait` polls every bound socket (both IPv6 and IPv4)
//! alongside an optional `Waker` (`poll`/`select` on Unix, `WSAPoll` on
//! Windows), so incoming IPv4 packets and main-thread wakeups return at
//! once instead of sleeping out the timeout.
//!
//! Sending never blocks: a datagram the socket will not take now (its
//! buffer full) is dropped and counted, as the network would drop it; the
//! protocol above resends what matters. Receiving reads what has arrived;
//! a datagram longer than `MAX_DATAGRAM` is dropped whole. An error a
//! datagram socket reports for an earlier send (Windows turns an ICMP
//! "port unreachable" into a connection reset on the next receive) is not
//! a reason to stop: it is counted and reading goes on (and on host sockets
//! Windows `SIO_UDP_CONNRESET` can be disabled so one vanished peer cannot
//! stall receives).

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use super::Clock;
use crate::{Datagram, MAX_DATAGRAM, Peer, Transport};

/// Receive errors in a row after which `recv` returns for now.
const MAX_ERRORS_IN_A_ROW: u32 = 16;
/// The receive buffer asked of the OS by default (bytes): 2 MiB.
pub const RECV_BUFFER: usize = 2 << 20;
/// The send buffer asked of the OS by default (bytes): 1 MiB.
pub const SEND_BUFFER: usize = 1 << 20;
/// Buffers used when hosting more than 8 seats.
pub const LARGE_RECV_BUFFER: usize = 8 << 20;
pub const LARGE_SEND_BUFFER: usize = 4 << 20;

/// Socket tuning applied at `Udp::bind_with`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketConfig {
    pub recv_buffer: usize,
    pub send_buffer: usize,
    /// Disable Windows `SIO_UDP_CONNRESET` so a vanished remote peer's ICMP
    /// "port unreachable" does not surface as `WSAECONNRESET` on host
    /// receives. Keep `false` on joining sockets so handshake port checks
    /// still report `errors > 0`.
    pub ignore_connreset: bool,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            recv_buffer: RECV_BUFFER,
            send_buffer: SEND_BUFFER,
            ignore_connreset: false,
        }
    }
}

impl SocketConfig {
    /// Host socket sizing scaled for `seats`.
    pub fn host(seats: u8) -> Self {
        if seats > 8 {
            Self {
                recv_buffer: LARGE_RECV_BUFFER,
                send_buffer: LARGE_SEND_BUFFER,
                ignore_connreset: true,
            }
        } else {
            Self {
                recv_buffer: RECV_BUFFER,
                send_buffer: SEND_BUFFER,
                ignore_connreset: true,
            }
        }
    }
}

/// Cross-thread wakeup handle for `Socket::wait` / `Udp::wait`.
#[derive(Clone)]
pub struct Waker {
    inner: Option<Arc<sys::WakerInner>>,
}

impl Default for Waker {
    fn default() -> Self {
        Self::new()
    }
}

impl Waker {
    /// Allocates a new wakeup pipe (Unix) or loopback socket (Windows).
    pub fn new() -> Self {
        Self {
            inner: sys::WakerInner::new().ok().map(Arc::new),
        }
    }

    /// Wakes a thread blocked inside `wait`, coalescing repeated calls.
    pub fn wake(&self) {
        if let Some(inner) = &self.inner {
            inner.wake();
        }
    }

    /// Sleeps up to `timeout` or until `wake()` fires (used when there is no
    /// UDP socket, e.g. Steam-only connections).
    pub(super) fn wait_alone(&self, timeout: Duration) {
        if timeout.is_zero() {
            return;
        }
        if let Some(inner) = &self.inner {
            sys::wait_sockets(&[], Some(inner), timeout);
        } else {
            std::thread::sleep(timeout);
        }
    }
}

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
    /// The OS's error for the IPv4 socket beside a lone IPv6 one.
    ipv4_refused: Option<String>,
}

/// How a datagram to an address leaves.
fn mapped(to: SocketAddr) -> SocketAddr {
    match to {
        SocketAddr::V4(v4) => SocketAddr::new(v4.ip().to_ipv6_mapped().into(), v4.port()),
        v6 => v6,
    }
}

impl Udp {
    /// Binds `address` (port 0: any free port), non-blocking. The IPv6
    /// wildcard falls back to IPv4's on a machine whose IPv6 is turned off.
    pub fn bind(address: SocketAddr, clock: Clock) -> std::io::Result<Udp> {
        Self::bind_with(address, clock, SocketConfig::default())
    }

    /// Binds `address` with explicit `config` buffer sizes and Windows
    /// connection-reset behaviour.
    pub fn bind_with(
        address: SocketAddr,
        clock: Clock,
        config: SocketConfig,
    ) -> std::io::Result<Udp> {
        let wildcard_v6 = matches!(address, SocketAddr::V6(v6) if v6.ip().is_unspecified());
        let first = match UdpSocket::bind(address) {
            Ok(socket) => socket,
            Err(error) if wildcard_v6 && error.kind() != ErrorKind::AddrInUse => {
                UdpSocket::bind((Ipv4Addr::UNSPECIFIED, address.port()))?
            }
            Err(error) => return Err(error),
        };
        let local = first.local_addr()?;
        let mut sockets = vec![first];
        let mut ipv4_refused = None;
        // Where the IPv6 wildcard is not dual-stack, the port is free for
        // IPv4 too; where it is, binding again fails and one socket serves.
        if local.is_ipv6() && wildcard_v6 {
            match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, local.port())) {
                Ok(v4) => sockets.push(v4),
                // Windows never makes the IPv6 wildcard dual-stack, so
                // there no IPv4 player could reach this socket.
                Err(error) if cfg!(windows) => ipv4_refused = Some(error.to_string()),
                Err(_) => {}
            }
        }
        let mut recv_buffer = None;
        for socket in &sockets {
            socket.set_nonblocking(true)?;
            recv_buffer = sys::configure_socket(socket, config).or(recv_buffer);
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
            ipv4_refused,
        })
    }

    /// Why IPv4 players cannot reach this socket, where the OS refused the
    /// IPv4 socket beside an IPv6 one that is not dual-stack (another
    /// program holds the port for IPv4).
    pub fn ipv4_refused(&self) -> Option<&str> {
        self.ipv4_refused.as_deref()
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

    /// Waits until a datagram is ready to read on any bound socket or
    /// `timeout` passes (without reading it).
    pub fn wait(&self, timeout: Duration) {
        self.wait_with(timeout, None);
    }

    /// Waits until a datagram is ready on any bound socket, `waker` fires,
    /// or `timeout` passes.
    pub(super) fn wait_with(&self, timeout: Duration, waker: Option<&Waker>) {
        if timeout.is_zero() {
            return;
        }
        let wake_inner = waker.and_then(|w| w.inner.as_deref());
        sys::wait_sockets(&self.sockets, wake_inner, timeout);
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

/// OS-specific socket options, multi-socket readiness polling, and wakeup.
#[cfg(unix)]
mod sys {
    use std::net::UdpSocket;
    use std::os::fd::{AsRawFd, RawFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use super::SocketConfig;

    pub(super) struct WakerInner {
        read_fd: RawFd,
        write_fd: RawFd,
        pending: AtomicBool,
    }

    impl WakerInner {
        pub(super) fn new() -> std::io::Result<Self> {
            let mut fds = [0 as libc::c_int; 2];
            // SAFETY: `fds` points to 2 writable `c_int` slots.
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for fd in fds {
                let ok = unsafe {
                    let flags = libc::fcntl(fd, libc::F_GETFD);
                    let set_cloexec =
                        flags >= 0 && libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) == 0;
                    let status = libc::fcntl(fd, libc::F_GETFL);
                    let set_nonblock = status >= 0
                        && libc::fcntl(fd, libc::F_SETFL, status | libc::O_NONBLOCK) == 0;
                    set_cloexec && set_nonblock
                };
                if !ok {
                    let err = std::io::Error::last_os_error();
                    // SAFETY: both pipe fds were returned by `pipe` above.
                    unsafe {
                        libc::close(fds[0]);
                        libc::close(fds[1]);
                    }
                    return Err(err);
                }
            }
            Ok(Self {
                read_fd: fds[0],
                write_fd: fds[1],
                pending: AtomicBool::new(false),
            })
        }

        pub(super) fn wake(&self) {
            if !self.pending.swap(true, Ordering::SeqCst) {
                let byte = [1u8];
                // SAFETY: `write_fd` is non-blocking and valid until `Drop`.
                unsafe {
                    libc::write(self.write_fd, byte.as_ptr().cast(), 1);
                }
            }
        }

        pub(super) fn drain(&self) {
            self.pending.store(false, Ordering::SeqCst);
            let mut buf = [0u8; 64];
            loop {
                // SAFETY: `read_fd` is non-blocking and valid until `Drop`.
                let n = unsafe { libc::read(self.read_fd, buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
            }
        }
    }

    impl Drop for WakerInner {
        fn drop(&mut self) {
            // SAFETY: `read_fd` and `write_fd` are owned exclusively by `WakerInner`.
            unsafe {
                libc::close(self.read_fd);
                libc::close(self.write_fd);
            }
        }
    }

    fn set_sockbuf(fd: RawFd, opt: libc::c_int, bytes: usize) -> Option<usize> {
        let size = libc::c_int::try_from(bytes).ok()?;
        let len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `fd` is open while `socket` lives; `size` is a `c_int`.
        let set =
            unsafe { libc::setsockopt(fd, libc::SOL_SOCKET, opt, (&raw const size).cast(), len) };
        let mut given: libc::c_int = 0;
        let mut given_len = len;
        // SAFETY: as above; `given` and `given_len` are writable `c_int`/`socklen_t`.
        let got = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                opt,
                (&raw mut given).cast(),
                &raw mut given_len,
            )
        };
        (set == 0 && got == 0)
            .then(|| usize::try_from(given).ok())
            .flatten()
    }

    /// Configures receive and send buffers; returns the OS receive buffer size.
    pub(super) fn configure_socket(socket: &UdpSocket, config: SocketConfig) -> Option<usize> {
        let fd = socket.as_raw_fd();
        let recv = set_sockbuf(fd, libc::SO_RCVBUF, config.recv_buffer);
        let _ = set_sockbuf(fd, libc::SO_SNDBUF, config.send_buffer);
        recv
    }

    pub(super) fn wait_sockets(
        sockets: &[UdpSocket],
        waker: Option<&WakerInner>,
        timeout: Duration,
    ) {
        let max_fd = sockets
            .iter()
            .map(|s| s.as_raw_fd())
            .chain(waker.map(|w| w.read_fd))
            .max();
        let Some(max_fd) = max_fd else {
            return;
        };
        // Use `select` when all fds fit in `fd_set` so sub-ms timeouts
        // retain microsecond precision on both macOS and Linux.
        if max_fd >= 0 && (max_fd as usize) < libc::FD_SETSIZE {
            let mut tv = libc::timeval {
                tv_sec: timeout.as_secs().min(i32::MAX as u64) as _,
                tv_usec: timeout.subsec_micros() as _,
            };
            // SAFETY: all fds are valid and `< FD_SETSIZE`.
            unsafe {
                let mut readfds: libc::fd_set = std::mem::zeroed();
                libc::FD_ZERO(&raw mut readfds);
                for s in sockets {
                    libc::FD_SET(s.as_raw_fd(), &raw mut readfds);
                }
                if let Some(w) = waker {
                    libc::FD_SET(w.read_fd, &raw mut readfds);
                }
                let ready = libc::select(
                    max_fd + 1,
                    &raw mut readfds,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut tv,
                );
                if ready > 0
                    && let Some(w) = waker
                    && libc::FD_ISSET(w.read_fd, &raw const readfds)
                {
                    w.drain();
                }
            }
            return;
        }
        let mut fds = Vec::with_capacity(sockets.len() + usize::from(waker.is_some()));
        for s in sockets {
            fds.push(libc::pollfd {
                fd: s.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let wake_idx = waker.map(|w| {
            let idx = fds.len();
            fds.push(libc::pollfd {
                fd: w.read_fd,
                events: libc::POLLIN,
                revents: 0,
            });
            idx
        });
        let timeout_ms = timeout.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        // SAFETY: `fds` points to `fds.len()` initialized `pollfd` entries.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if ready > 0
            && let Some(idx) = wake_idx
            && (fds[idx].revents & libc::POLLIN) != 0
            && let Some(w) = waker
        {
            w.drain();
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
    use std::os::windows::io::AsRawSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use windows_sys::Win32::Networking::WinSock::{
        POLLRDNORM, SIO_UDP_CONNRESET, SO_RCVBUF, SO_SNDBUF, SOCKET, SOCKET_ERROR, SOL_SOCKET,
        WSAIoctl, WSAPOLLFD, WSAPoll, getsockopt, setsockopt,
    };

    use super::SocketConfig;

    pub(super) struct WakerInner {
        recv_socket: UdpSocket,
        send_socket: UdpSocket,
        recv_addr: SocketAddr,
        pending: AtomicBool,
    }

    impl WakerInner {
        pub(super) fn new() -> std::io::Result<Self> {
            let recv_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            recv_socket.set_nonblocking(true)?;
            let recv_addr = recv_socket.local_addr()?;
            let send_socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
            send_socket.set_nonblocking(true)?;
            Ok(Self {
                recv_socket,
                send_socket,
                recv_addr,
                pending: AtomicBool::new(false),
            })
        }

        pub(super) fn wake(&self) {
            if !self.pending.swap(true, Ordering::SeqCst) {
                let _ = self.send_socket.send_to(&[1u8], self.recv_addr);
            }
        }

        pub(super) fn drain(&self) {
            self.pending.store(false, Ordering::SeqCst);
            let mut buf = [0u8; 64];
            while self.recv_socket.recv_from(&mut buf).is_ok() {}
        }
    }

    fn set_sockbuf(sock: SOCKET, opt: i32, bytes: usize) -> Option<usize> {
        let size = i32::try_from(bytes).ok()?;
        let len = std::mem::size_of::<i32>() as i32;
        // SAFETY: `sock` is open while `socket` lives; `size` is an `i32`.
        let set = unsafe { setsockopt(sock, SOL_SOCKET, opt, (&raw const size).cast(), len) };
        let mut given: i32 = 0;
        let mut given_len = len;
        // SAFETY: as above; `given` and `given_len` are writable `i32`s.
        let got = unsafe {
            getsockopt(
                sock,
                SOL_SOCKET,
                opt,
                (&raw mut given).cast(),
                &raw mut given_len,
            )
        };
        (set != SOCKET_ERROR && got != SOCKET_ERROR)
            .then(|| usize::try_from(given).ok())
            .flatten()
    }

    pub(super) fn configure_socket(socket: &UdpSocket, config: SocketConfig) -> Option<usize> {
        let sock = socket.as_raw_socket() as SOCKET;
        if config.ignore_connreset {
            let mut enable: u32 = 0;
            let mut returned: u32 = 0;
            // SAFETY: `sock` is valid; `enable` and `returned` are valid `u32` buffers.
            unsafe {
                WSAIoctl(
                    sock,
                    SIO_UDP_CONNRESET,
                    (&raw mut enable).cast(),
                    std::mem::size_of::<u32>() as u32,
                    std::ptr::null_mut(),
                    0,
                    &raw mut returned,
                    std::ptr::null_mut(),
                    None,
                );
            }
        }
        let recv = set_sockbuf(sock, SO_RCVBUF, config.recv_buffer);
        let _ = set_sockbuf(sock, SO_SNDBUF, config.send_buffer);
        recv
    }

    pub(super) fn wait_sockets(
        sockets: &[UdpSocket],
        waker: Option<&WakerInner>,
        timeout: Duration,
    ) {
        let mut fds = Vec::with_capacity(sockets.len() + usize::from(waker.is_some()));
        for s in sockets {
            fds.push(WSAPOLLFD {
                fd: s.as_raw_socket() as SOCKET,
                events: POLLRDNORM as i16,
                revents: 0,
            });
        }
        let wake_idx = waker.map(|w| {
            let idx = fds.len();
            fds.push(WSAPOLLFD {
                fd: w.recv_socket.as_raw_socket() as SOCKET,
                events: POLLRDNORM as i16,
                revents: 0,
            });
            idx
        });
        if fds.is_empty() {
            return;
        }
        let timeout_ms = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: `fds` points to `fds.len()` valid `WSAPOLLFD` structs.
        let ready = unsafe { WSAPoll(fds.as_mut_ptr(), fds.len() as u32, timeout_ms) };
        if ready > 0
            && let Some(idx) = wake_idx
            && (fds[idx].revents & POLLRDNORM as i16) != 0
            && let Some(w) = waker
        {
            w.drain();
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use std::net::UdpSocket;
    use std::time::Duration;

    use super::SocketConfig;

    pub(super) struct WakerInner;

    impl WakerInner {
        pub(super) fn new() -> std::io::Result<Self> {
            Ok(Self)
        }
        pub(super) fn wake(&self) {}
        pub(super) fn drain(&self) {}
    }

    pub(super) fn configure_socket(_socket: &UdpSocket, _config: SocketConfig) -> Option<usize> {
        None
    }

    pub(super) fn wait_sockets(
        _sockets: &[UdpSocket],
        _waker: Option<&WakerInner>,
        timeout: Duration,
    ) {
        std::thread::sleep(timeout);
    }
}
