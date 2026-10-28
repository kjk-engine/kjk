# The network (`net`, `kjk net`, `kjk host`, `kjk join`)

The host's
world is the only truth; clients receive it exactly and run ahead only for
their own seat. All sans-IO: state machines fed bytes and a `NetTime`.
Co-op on top of it (I31: seats and bodies, holds, save copies): `COOP.md`.

## Layers (`crates/net`)
- `wire`, `link`: the 19-byte header, frames, acks, RTT and loss, one
  reliable ordered channel (sliced, paced), keepalive, timeout.
- `handshake`: hello, cookie, connect with the build and each game's world
  data (`vfs::fingerprint` groups that build worlds); a typed `Refusal`.
  A host may have a join password (`JoinKey`): the client proves it with a
  keyed BLAKE3 over the host's cookie (the password never travels), the
  host compares in constant time and refuses "wrong password". Wrong
  passwords are answered at most 4 a second per sending host (an IPv4
  address or IPv6 /64, whatever its ports) and 32 a second from everyone
  (past that, dropped), so online guessing goes at 32 a second at most;
  a captured proof can still be attacked offline, and a reconnect token
  travels in clear (see below). Hellos are limited per host too (8 a
  second). Protocol 2.
- `codec`: snapshots as exact XOR deltas against one the peer confirmed,
  deflated when that is smaller (`pack`: a fight's deltas by half,
  keyframes by ~10); an 8-byte hash check on every rebuild. The modules
  left go whole in a keyframe and as one byte in a delta (they change only
  at an install, which starts an epoch).
- `session`: `Server` (seats, input buffer, slack, snapshots, installs),
  `Client` (rebuild, clocks, commands, prediction, view).
- `io`: `Udp` (`[::]` takes IPv4 too; 1 MiB receive buffer), `Loopback`,
  `SimNet`; `netem`: seeded conditions, `KJK_NETEM` or `--netem`.

## Rates and rules
- 125 Hz ticks; snapshots every 2nd tick (every tick in-process, every 32nd
  to a host's own client), to every 4th when loss passes 10% over 2 s or
  the RTT doubles, back after 5 s. A host splits and hashes a tick's
  snapshot only when a frame, a hold or a joiner reads it.
- Each command carries its view lag (I25, C18: its tick minus the remote
  time shown, the lead in it: about the RTT and 75–80 ms more, 181 ms at
  100 ms RTT and 375 at 300; the sim rewinds at most 400, and holds blade hits
  on that seat for C19's grace, at most 100). A missing one: the seat's last
  for 16 ticks, then its idle command (`Command::idle`, `COOP.md`).
- Client lead: slack + loss margin; remote view D = interval + p95 jitter
  + 4 ms behind arriving snapshots. The host measures each command's slack
  when its tick is stepped (a late one's too, negative); the loss margin
  grows only for misses of commands the client made, not for ticks it
  skipped after a long frame. `kjk net margins` measures what each margin
  buys (host and player stepping in frames, hitches). Prediction restores the newest
  snapshot and steps again; own-avatar mismatches are logged by cause.
- The own character (`play_view`): a prediction that changes what its legs
  or torso play at a tick already shown (the host's hit, parry or bounce,
  which the client never predicts) is a rewrite (`play.own_rewrites` in a
  dump; at 150 ms RTT in a duel, ~170 ticks in runs of 8–15). The
  own figure plays on the sim's skeleton's timing (`cast::Figure::adopt`),
  so the animation the host started shows as far in as the host has it
  (QUIRKS.md Q-0072); its place is corrected
  as before (50 ms half-life, a snap past 64 units).
- Installs (D13): clients plan in parallel, commit locally, `check_after`;
  the world resumes when all loaded or after 30 s (a late loader catches
  up). Each `Installer` keeps built areas (`session::AreaCache`).
- Ending: a disconnect goes in three packets (`HostQuit`, `Left`).
- Coming back: a player whose link times out joins again by itself with
  its seat's token (`ClientConfig::rejoin_us`; `kjk join` tries 45 s,
  from the same socket), told so on screen. The host keeps a timed-out
  player's seat 60 s for the token, and the token stays the seat's across
  rejoins (an attempt whose accept was lost leaves the next the same
  one). A token naming a connection still open (a new port, before the
  host's own timeout) takes its seat over only once the host has heard
  nothing from it for 1 s (`TAKEOVER_QUIET_US`); before that it is
  refused `InUse` and the rejoining client tries again. Tokens travel in
  clear: one who sees a token can take that seat when its player goes
  quiet, never while it plays. A rejoin that lands in another seat (the
  host restarted) ends the session. A link that broke the protocol frees
  its seat at once.
- Actions name objects by their index in the module: those waiting are
  dropped when an install starts and when it commits, and none is taken
  while it loads or from a player not loaded in the current epoch.

## Processes (`launcher`, `game`)
- Solo (`kjk play`, `kjk sim view`): the server and its own client over
  `Loopback` in lockstep: scripted runs record and show what they did
  before the network. The views draw the host's own snapshots (shared,
  never copied); the client predicts nothing (the host steps each tick in
  the same frame) and rebuilds every 32nd tick's snapshot, hash-checked, so
  the codec stays proven in every session (`net.json`'s `rebuilt`,
  `check_failures`) without costing a rebuild a tick.
- `kjk host`: the same for `--seats`, listening on UDP 29170; a net thread
  (`net_thread`, D3) pumps the socket and links, so acks never wait for a
  frame. Every stepped hash goes to `hashes.txt`. It says "listening on"
  once its stage is loaded: `cargo xtask duo` and `refusals` join on that
  line (whole lines only, a line still being written is not read).
- `kjk join <host>`: handshake, then the host's stage loads while the net
  thread answers; its own avatar and camera predicted, every other player
  and creature from the host's snapshots at the remote view.
- **Through Steam** (`io::steam`, the `steam` feature, on by default):
  beside UDP, a host takes players through Steam's relayed messages
  (`ISteamNetworkingMessages`), so nobody forwards a port. The session's
  `Socket` carries both at once: a datagram leaves by its peer's kind
  (`Peer::Addr` or `Peer::Steam`), the handshake and link above are the
  same. Steam opens as app 480 (Valve's "Spacewar" test app, until kjk has
  an app of its own), so friends see the host "playing Spacewar". The
  menu's host always offers it (`kjk host --steam` when scripted) and sets
  the rich presence Steam's friends list reads: a friend's "Join Game" (or
  an invite) joins from their main menu, and the Join screen and `kjk join`
  take `steam:<id>` or a hosting friend's name. Without Steam running (or
  with `KJK_STEAM=0`) it is by address alone. Headless and scripted runs
  never open Steam unless they host with `--steam` or join a `steam:` host.
  The build copies Steam's library (`libsteam_api`) beside `kjk`
  (`launcher/build.rs`); `--no-default-features --features probes` builds
  without it, and so does the release (`cargo xtask dist`, docs/RELEASE.md),
  which ships no Valve code.
- Refused players are told why: another build, "your k1 override folder
  differs from the host's", "the host's 3 seats are all taken".
- Each run writes `net.json` (with the net feel's 250 ms samples);
  `KJK_PERF=1` adds `net.*` spans and counters to `trace.json`. Measured
  (`end_m01aa`'s fight, 100 ms RTT): ~60 KB/s down (97 before packing),
  4 KB/s up, RTT read 100–103. Of the unpacked delta, the module's
  creatures are ~70%: their Force state (`regen_at` moves every tick
  while a pool is full), player state and place, last command and timers
  (`kjk net bandwidth`).

## Commands
```bash
kjk net probes | kjk net codec          # the core (with seats: breaks, timeouts, takeovers, actions across installs); the snapshot codec
kjk net bandwidth HOST/inputs.kjkr [--back 2,10,20]  # a client's KB/s, by kind and field
kjk net session [--only NAME] [--soak-minutes 10]   # sessions in virtual time
kjk net margins [--rtt 100,200] [--only NAME]       # what each clock margin buys
kjk net feel HOST/net.json PLAYER/net.json          # net-feel.md
kjk host --stage end_m01aa --arrive wp_player01 [--seats 3] [--guests companions|avatars] [--follow TAG] [--port 29170] [--password P]
kjk host --load 0001 [--seats 3]   # host a save (`KJK_SAVES`) instead of a stage; its guests take its companions
kjk host --stage end_m01aa --steam   # Steam friends join too, no port forwarded
kjk join steam:76561197960287930    # or a hosting Steam friend's name: kjk join "Bastila"
kjk join 127.0.0.1[:29170] [--password P] [--moves …] [--plan-delay S] [--trace own.csv] [--headless --cmds '…']
#   --password: the command line only for now (the menu's host and join screens have no field for it)
cargo xtask duo [--netem …] [--connect ::1] [--plan-delay 35]   # or: make duo
#   host + player as processes: rebuilds, replay, installs, avatars, net-feel.md
cargo xtask duo --stage duel   # two Jedi facing off, fighting (I25)
cargo xtask refusals [--old-build PATH]   # each refusal, as processes
```
