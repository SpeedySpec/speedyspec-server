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

## Releasing UDP under load (`handoff_under_load`, plan 3 Task 8)

Waiting for the old endpoint to go idle, as above, is correct but too slow under load. A closed endpoint keeps
reading the shared socket for 3 × PTO (about 3 s for connections without RTT samples), and it **refuses** every new
handshake it sees in that time: 4,733 refused HTTP/3 requests in one upgrade. `Serving::stop_accepting` therefore
releases UDP in this order:

1. `Endpoint::set_server_config(None)`. A handshake that arrives now is dropped, not refused. The client retransmits
   it, and the next generation answers.
2. `Endpoint::close(0x5353, …)` for every open connection.
3. Wait 50 ms (`CLOSE_FLUSH`). `close` only queues the close packets, and they must leave from the shared socket: if
   they go out from another address, clients ignore them and the connection hangs until its idle timeout.
4. `Endpoint::rebind(throwaway local socket)`. The old endpoint stops reading the shared socket at once, and finishes
   closing on its own.
5. Stop the Salvo server and drop the endpoint (its `wait_idle` runs in the background).

HTTP/1.1 keep-alive connections race a graceful close: a client may send a request on a connection just as the
server closes it. So while draining, the old generation answers HTTP/1.x with `Connection: close`, and stops
accepting TCP only after `TCP_GRACE` (1 s). Busy clients leave cleanly within that time; HTTP/2 gets `GOAWAY`.

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

**Trial ends:** K asks N+1 to confirm the version in `state.json` (`ConfirmServer`), then → N `Shutdown`. If the
confirm fails, the trial fails: N+1 drains, N resumes and reverts the group, N+1 shuts down.

**A switch the keeper refuses** (during a trial or while the previous generation drains): K asks the generation to
undo it (`CancelServerSwitch`, nothing is marked failed) before the client gets the refusal. From recording a server
switch until it is resumed, cancelled or drained, a generation refuses release commands and pauses auto-update, so
two generations never write `state.json` at once.

**N+1 exits during the trial:**
- If N is alive: K → N `Resume { failed }`. N builds new `Server`s on fresh clones and new endpoints, and reverts the
  pending group.
- Otherwise: K starts a fresh generation from the confirmed binary.

**Keeper killed:** the generation sees EOF on fd 3, stops accepting and closes every listening socket at once (so a
restarted keeper can bind them), lets open connections run for up to 1.5 s, and exits.

**Reload (`speedyspec-server reload`):** the keeper answers it itself. It re-reads the config; if the listeners
(address, TLS on or off, `http3`) differ from the running ones, it refuses, since only a restart can rebind. Otherwise
it runs the upgrade above with the confirmed binary as N+1, trial included. A failed reload trial resumes N with
`Resume { failed: None }`: nothing is marked failed or reverted.

**The keeper re-executing itself:** when a trial is confirmed for a version newer than the keeper's own, the keeper
waits until N has been reaped (after `execve` nothing could reap it), then clears close-on-exec on the listening
sockets, `keeper.sock`'s listener, `server.lock` and N+1's channel, and `execve`s the confirmed binary as
`run --config <same>`, with those fd numbers, N+1's PID and version, and the confirmed binary's path as JSON in
`SPEEDYSPEC_KEEPER_STATE`. The new keeper adopts them (it sets close-on-exec again so generations do not inherit them),
waits on N+1 by PID from a thread (it is still the parent, so `waitpid` works), and logs `keeper_reexecuted`. N+1
never notices. Before the re-exec, a binary that needs a newer protocol than the keeper speaks is refused.

**Control requests that meet a dying generation:** `status`, `release list` and `release check` are asked again of
the generation the keeper resumes or restarts in its place. Requests that change anything are never retried.
