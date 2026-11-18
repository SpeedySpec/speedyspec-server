# Socket handoff: what was proven, and the message flow

`crates/speedyspec-server/tests/handoff_proof.rs` proves the four mechanisms the keeper and generations rely on. Each
test passes on macOS arm64 and Linux musl.

## Pausing and resuming accepts on one listener (`tcp_stops_accepting_then_resumes`)

- A Salvo `Server` built on a `TcpAcceptor` from a `try_clone` of the listener stops accepting on
  `ServerHandle::stop_graceful`. Requests already accepted finish: a 1 s request started before the stop completes.
- Dropping that acceptor closes only its own fd. The listening socket stays open through the other clones.
- Connections made while nobody accepts wait in the kernel backlog. A new `Server` on another `try_clone` serves them.
  A `Server` cannot be restarted, so **resume means a fresh `Server` on a fresh clone**, which is why `Serving` keeps
  one clone per socket.
- `stop_graceful` makes hyper send HTTP/2 `GOAWAY` (`graceful_shutdown`). For HTTP/3, Salvo's quinn builder sends
  `GOAWAY` with `conn.shutdown(0)`.

## Sharing listening sockets across processes (`two_processes_share_a_listener`)

- The fds travel as `SCM_RIGHTS` over a Unix socketpair, with `rustix::net::{sendmsg, recvmsg}` and
  `SendAncillaryBuffer` / `RecvAncillaryBuffer`. One byte of payload carries them.
- The child's end of the socketpair is placed on fd 3 with `dup2` in `Command::pre_exec`. `dup2` clears close-on-exec
  on the new fd, so only fd 3 crosses `exec`.
- Both processes may accept on the same TCP listener at once, and the kernel hands each connection to one of them.
  Once the parent stops accepting, every new connection reaches the child.

## Handing a QUIC endpoint to another process (`quic_endpoint_hands_over`)

- `quinn::Endpoint::new(EndpointConfig::default(), Some(server_config), std_udp_socket, Arc::new(TokioRuntime))`
  builds an endpoint over a socket received from another process.
- `Endpoint::close(0x5353, b"server upgrading, reconnect")` sends every open connection an application close with
  that code. The client sees `ConnectionError::ApplicationClosed` with `error_code == 0x5353`, which is the "upgrading"
  close code a WebTransport session will receive.
- **Finding:** after `close`, the endpoint's driver keeps reading the UDP socket until its connections have finished
  closing. A second endpoint reading the same socket in that window loses packets to the first, which answers
  "the server refused to accept a new connection". The releasing side must therefore:
  1. `close`;
  2. stop its Salvo server (which drops the `QuinnAcceptor`'s endpoint clone);
  3. `Endpoint::wait_idle().await`;
  4. drop every remaining `Endpoint` handle;
  5. only then report `UdpReleased`.
  With that order, the child's first HTTP/3 request succeeds, five times out of five, with no retry.

## Rebuilding a QUIC endpoint in the same process (`quic_endpoint_resumes`)

The same order as above (close, stop, `wait_idle`, drop) followed by a new endpoint on another `try_clone` of the
socket serves HTTP/3 again. This is the resume path of a failed trial.

## The message flow

Keeper K, old generation N, new generation N+1 (`speedyspec-server-handoff`, protocol 1).

**Start or crash restart:**

1. K binds every socket and spawns a generation with its channel on fd 3.
2. K → `Hello { sockets }` + fds.
3. The generation prepares everything and → `Ready`.
4. K → `StartUdp`.
5. K notifies systemd once, the first time.

**Upgrade (spec "Socket handoff" steps 1–6):**

1. N → `Upgrade { version, binary }` (from `release switch server@v`).
2. K spawns N+1 → `Hello` (N+1 accepts TCP at once, not UDP) → N+1 `Ready` (within `ready_timeout`).
3. K → N `Drain`.
4. N stops accepting TCP (`GOAWAY`), closes its QUIC endpoints with `0x5353`, waits idle, drops them, stops
   auto-update and refuses release commands, then → `UdpReleased`.
5. K → N+1 `StartUdp`. N+1 is active, and `keeper.sock` forwards to it.
6. The trial starts.
7. N keeps its in-memory state, and its open connections, until the trial ends.

**Trial ends:** K confirms the version in `state.json`, then → N `Shutdown`.

**N+1 exits during the trial:**
- If N is alive: K → N `Resume { failed }`. N builds new `Server`s on fresh clones and new endpoints, and reverts the
  pending group.
- Otherwise: K starts a fresh generation from the confirmed binary.

**Keeper killed:** the generation sees EOF on fd 3, stops accepting, waits idle (up to 30 s), and exits.
