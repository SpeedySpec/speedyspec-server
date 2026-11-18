# Serving from pre-bound sockets with Salvo

The keeper binds every listening socket and hands it to a generation, so a generation must serve from sockets it did
not bind. This records how that works with Salvo, as proven by `crates/speedyspec-server/tests/listeners_proof.rs`
(HTTP/1.1, h2c, HTTPS with ALPN h2, and HTTP/3 from one `serve` call).

## Versions

| Piece | Version |
| --- | --- |
| salvo / salvo_core | 1.2.1 (features `server`, `server-handle`, `http1`, `http2`, `http2-cleartext`, `quinn`, `rustls`, `ring`; default features off) |
| salvo-http3 | 0.7.0, which re-exports upstream `h3` 0.0.8 and `h3-quinn` 0.0.10, plus Salvo's own `webtransport` module |
| quinn | 0.11.12 (`runtime-tokio`, `rustls-ring`), named directly so `quinn::Endpoint::new` can take a std socket |

Default features are off so that `aws-lc-rs` is not pulled in; `ring` builds on musl with only a C compiler.
`cargo tree -i aws-lc-sys` prints nothing.

## Per protocol

All of Salvo's acceptor constructors are public, so no custom acceptor or patch is needed.

- **Cleartext TCP (HTTP/1.1 and h2c).** A tiny `Listener` (`Prebound`) wraps the `std::net::TcpListener`: its
  `try_bind` sets it non-blocking, converts it with `tokio::net::TcpListener::from_std`, and returns
  `TcpAcceptor::try_from(listener)`. With the `http2-cleartext` feature that acceptor offers HTTP/1.1 and HTTP/2
  (prior knowledge).
- **TLS TCP (HTTPS).** `RustlsListener::new(RustlsConfig, Prebound(listener)).try_bind()`. ALPN offers `h2` and
  `http/1.1`. `RustlsListener` takes a config *stream*, so plan 3's certificate reload on `SIGHUP` can feed new
  configs into the same listener without rebinding.
- **UDP (HTTP/3).** The `RustlsConfig` converts into `salvo::conn::quinn::ServerConfig` with `try_into` (which strips
  `h2` / `http/1.1` from ALPN, leaving `h3`). `quinn::Endpoint::new(EndpointConfig::default(), Some(config), socket,
  Arc::new(quinn::TokioRuntime))` builds the endpoint over the given socket, and `QuinnAcceptor::new(endpoint,
  local_addr, CancellationToken::new())` turns it into a Salvo acceptor.

Each acceptor gets its own `Server`, all spawned into one `JoinSet` and serving `Service::new(router.clone())` from
one shared `Arc<Router>`. `Service` is not `Clone` and is `#[non_exhaustive]`, which is why `serve` takes the router,
not a `Service`.

## Shutdown

`Server::handle()` (feature `server-handle`) returns a `ServerHandle`. `serve` waits for its shutdown future (or the
first server to fail), calls `stop_graceful(GRACEFUL_TIMEOUT)` (30 s) on every handle, and waits for all servers to
return. `stop_graceful` stops accepting and lets in-flight requests finish.

## What this means for handoff (plan 3)

- **Stop accepting TCP while requests finish:** `ServerHandle::stop_graceful` does exactly that, per server.
- **Several processes on one TCP listener:** both generations can hold the same listening socket; the draining one
  simply stops accepting.
- **QUIC:** quinn offers `Endpoint::set_server_config(None)` (refuse new connections while existing ones continue),
  `Endpoint::close(code, reason)` (close every connection with an application code, for the "server upgrading" close),
  and `Endpoint::rebind(std::net::UdpSocket)`. There is no "pause reading" call: the draining generation stops reading
  the UDP socket by closing its endpoint, and resume (trial fall-back) builds a new endpoint on the socket. Two
  processes must never read the same UDP socket at once, so the keeper orders the two steps (spec, "Socket
  handoff", steps 4–5).
- Generation-side WebTransport close codes go through `Endpoint::close` or per-session close in Salvo's
  `webtransport` module; plan 4 picks which.
