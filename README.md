# speedyspec-server

The SpeedySpec web server. One binary serves the SpeedySpec admin (a static site, held in memory), and runs the admin
API, the front-end API and the blog (PHP apps, on the PHP runtime's supervised workers over FastCGI), each mounted by
host and path, over HTTP/1.1, HTTP/2 and HTTP/3. It gates the admin: sign-in and per-page access, decided by the admin
API. Each app, and the PHP runtime, has its own releases: it is downloaded, verified, kept, switched and rolled back
without touching the others. The server
upgrades itself by handing its listening sockets to a new process, so no connection is refused during an upgrade.

The same binary is the control tool: `status`, `release …` and `reload` talk to the running server through
`<run_dir>/keeper.sock`.

## How it runs

`speedyspec-server run` starts the **keeper**. The keeper binds the listeners, holds `<run_dir>/server.lock` and
`keeper.sock`, and starts one **generation**: a child process (`speedyspec-server serve`, hidden) that does all the
serving, on sockets the keeper passes it. The keeper serves no traffic. It restarts a generation that crashes (with
back-off), runs upgrades, and stays the process that systemd or the container runtime watches.

## Modes

| | `managed` (default) | `immutable` |
|---|---|---|
| Where | A host or VM under systemd, or Kubernetes with a writable volume for `data_dir` | Containers with read-only storage |
| Apps come from | `source`: a release index URL | `path`: a directory baked into the image |
| Writes to disk | `data_dir` (releases, `state.json`) and `run_dir` | Nothing; sockets only, in an existing `run_dir` |
| Upgrades | `release …` and auto-update; the server upgrades itself by handoff | A new image, rolled out by the `speedyspec` CLI |
| Refused | — | `release install/switch/rollback/pin/unpin` and `reload` |

In immutable mode the keeper still restarts a crashed generation, but no handoff ever happens. The server never
creates `run_dir`: if it does not exist, the server serves but `status` and `release list` cannot reach it. Mount an
empty, writable `run_dir` (for example an `emptyDir`) if you want them.

## Commands

```
speedyspec-server run      [--config <path>]   # the keeper (systemd's ExecStart)
speedyspec-server status   [--config <path>]
speedyspec-server reload   [--config <path>]
speedyspec-server manifest                     # this binary's release manifest, as JSON
speedyspec-server release list     [<app>]
speedyspec-server release check    [<app>]
speedyspec-server release install  <app> [<version>]
speedyspec-server release switch   <app>@<version> [<app>@<version> …]
speedyspec-server release rollback <app> [<version>]
speedyspec-server release pin      <app> <version>
speedyspec-server release unpin    <app>
```

`--config` defaults to `/etc/speedyspec/server.toml`, and every `release` command takes it too. Errors go to stderr,
prefixed `speedyspec-server:`, with exit code 1.

`status` prints, tab-separated: the server's version and mode, the active generation (PID, version, `trial` or
`confirmed`), the keeper's own version, then one line per app (`app kind active kept pinned failed`, `-` for none).
This is during a server upgrade's trial:

```
server	0.2.0	managed
generation	66985	0.2.0	trial
keeper	0.1.0
admin	static	1.0.0	1.0.0	-	-
```

and after the trial, once the keeper has re-executed itself from the new binary:

```
server	0.2.0	managed
generation	66985	0.2.0	confirmed
keeper	0.2.0
admin	static	1.0.0	1.0.0	-	-
```

`release list` prints only the app lines. Lines starting `incompatible` follow if the kept set's requirements
conflict.

## Configuration

```toml
mode = "managed"                       # or "immutable"
data_dir = "/var/lib/speedyspec"       # required in managed mode
run_dir = "/run/speedyspec"            # default

[[listener]]
address = "127.0.0.1:8080"             # plain HTTP/1.1 and HTTP/2 (h2c)

[[listener]]
address = "0.0.0.0:8443"
tls = { cert = "/etc/speedyspec/tls/cert.pem", key = "/etc/speedyspec/tls/key.pem" }
http3 = true                           # also HTTP/3 on UDP 8443; needs tls

[proxy]
trusted = ["10.0.0.0/8"]               # peers whose forwarding headers are believed
headers = "x-forwarded"                # or "forwarded" (RFC 7239)

[app.server]
kind = "server"                        # exactly one; its release is this binary
source = "https://speedyspec.github.io/speedyspec-server/server/index.json"
keys = ["RWT1iFxWT0DcUmOoHd7G9+GvpsUxaAPTOqtDr0GyVUvsJHHuBNRxI+0n"] # this repository's minisign.pub

[app.admin]
kind = "static"                        # or "php"
source = "https://releases.example.com/admin/index.json"
keys = ["RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"]
keep = 2                               # kept releases (default 2: the live one and the one to roll back to)
update = "auto"                        # or "manual" (the server app defaults to manual)
check_interval = "15m"
prerelease = false
allow_unsigned = false

[app.php-runtime]
kind = "runtime"                       # the PHP runtime binary the php apps run; see docs/php.md
source = "https://speedyspec.github.io/speedyspec-server/php-runtime/index.json"
keys = ["RWT1iFxWT0DcUmOoHd7G9+GvpsUxaAPTOqtDr0GyVUvsJHHuBNRxI+0n"] # the same key as the server's
update = "manual"

[app.blog]
kind = "php"
source = "https://releases.example.com/blog/index.json"
runtime = "php-runtime"                # may be left out when only one runtime app is configured

[app.admin-api]
kind = "php"                           # the admin API: the gate asks it who is signed in
source = "https://releases.example.com/admin-api/index.json"

[app.admin-api.php]
site_dir = "/var/lib/speedyspec/sites/admin-api"
env_file = "/etc/speedyspec/admin-api.env"   # e.g. a database user that can write; merged after site_dir/.env

[app.blog.php]                         # php apps only; see docs/php.md
workers = { min = 1, max = 4 }         # start min, grow to max on demand
max_requests = 1000                    # requests before a worker is recycled
request_timeout = "60s"                # a longer request is a 504
memory_limit = "256MiB"                # a worker past this after a request is recycled
idle_timeout = "10s"                   # idle workers above min exit after this
site_dir = "/var/lib/speedyspec/sites/blog"  # the app's writable directory; required in managed mode
bindings = {}                          # PHP classes bound to ports for every request
env = { WP_ENVIRONMENT = "production" }  # merged last, after site_dir/.env and env_file

[[mount]]
host = "*"                             # or an exact host, which beats "*" on that host
path = "/admin/"
app = "admin"
access = "signed-in"                   # or "public" (default)

[[mount]]
path = "/admin-api/"
app = "admin-api"
access = "signed-in"

[access]
implementation = "gate"                # or "deny"; "dev-allow-all" only with every listener on loopback

[gate]                                 # see docs/php.md, "The admin gate"
app = "admin-api"                      # the php app the gate asks
session_ttl = "60s"                    # how long a signed-in answer is remembered
timeout = "5s"
site_ttl = "2s"                        # how long the site state is remembered
poll = "1s"                            # how often the revocation feed is read
max_pending = 16                       # uncached session checks at once; more get 503
# maintenance_apps = ["blog", "frontend-api"] # the apps the admin API's maintenance covers (default: every app; never the admin)

[health]                               # each php app's health probes
interval = "2s"
timeout = "1s"
fail_after = 3                         # failed rounds in a row before maintenance
pass_after = 3

[runtime]
trial_period = "2m"                    # how long a new runtime release is on trial

[switch]
migrate_timeout = "30m"                # how long a release's migration may run

[limits]
static_memory = 536870912              # bytes of static files held in memory (512 MiB)

[release.static]
fallback_grace = "1h"                  # how long the previous release still serves hashed assets after a switch

[handoff]
ready_timeout = "30s"                  # a new generation must report ready within this
drain_timeout = "5m"                   # how long a retiring generation keeps open connections
trial_period = "2m"                    # how long a new server version is on trial
```

In immutable mode, apps take `path` (a directory holding `speedyspec-release.json`) instead of `source`; `source` is
refused there, and `path` is refused in managed mode. A runtime app's `path` holds its `speedyspec-php-runtime`
binary. The server app takes neither `path` nor a mount. An app may name `maintenance_page = "<static app>"`, whose
release's maintenance page it shows while in maintenance. Durations are written like `30s`, `15m`,
`1h`, and the `[handoff]` ones must be at least `1s`.

## Releases (managed mode)

A release index is JSON: `{ "app": "<id>", "releases": [{ "version", "published", "artifacts": [{ "target", "url",
"sha256", "minisig" }] }] }`. Static and PHP apps publish one artifact with `target = "any"`; the server publishes one
per Rust target triple. Downloads are checked against `sha256` and, unless `allow_unsigned = true`, against their
minisign signature and one of the app's `keys`.

- `release install` downloads and keeps a release without switching to it. `update = "auto"` apps install and switch
  on their own every `check_interval`.
- `release switch` makes kept releases live, several apps at once as one step. The server keeps `keep` releases per
  app and removes older ones.
- `release rollback <app>` switches back to the previous kept release (or to the one given), marks the release it
  left as failed, and **pins** the app there, so auto-update does not bring the bad release back. `release unpin`
  lets auto-update run again.
- On first start the running binary is registered as the first confirmed server release, copied to
  `<data_dir>/releases/server/<version>/`. From then on the keeper runs the confirmed binary from there. A later
  start from a binary newer than the confirmed one (a package upgraded in place) adopts it the same way, unless that
  version has failed before; an older one runs the confirmed binary instead.

### Published releases of the server and the PHP runtime

This repository publishes both, each with its own version, on its GitHub releases (tags `speedyspec-server-v<version>`
and `speedyspec-php-runtime-v<version>`): one binary per target (`x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, `aarch64-apple-darwin`), its `.minisig`, and `SHA256SUMS`. GitHub Pages serves their
indexes, which managed mode reads as the `source` of the apps `server` and `php-runtime` (the index names the app, so
the ids must be these):

- `https://speedyspec.github.io/speedyspec-server/server/index.json`
- `https://speedyspec.github.io/speedyspec-server/php-runtime/index.json`

Every binary is signed with the key in [`minisign.pub`](minisign.pub); its second line goes in both apps' `keys`. To
check a download by hand:

```bash
minisign -V -p minisign.pub -m speedyspec-server-x86_64-unknown-linux-musl
```

```bash
grep ' speedyspec-server-x86_64-unknown-linux-musl$' SHA256SUMS | sha256sum -c
```

### Releasing

release-please keeps a release pull request open on `main`. Merging it builds that component for every target at the
merge commit and runs its tests there; only when every build passes are the tag and the GitHub release created. The
binaries are then signed and uploaded, and both indexes are rebuilt from the GitHub releases and deployed to Pages. A
failed build publishes nothing, and the next push to `main` tries again.

The *Release* workflow can also be run by hand: with a tag, it finishes a release whose publishing failed part-way
(a release whose binaries, signatures and `SHA256SUMS` are all there is never changed: builds are not reproducible,
so a replaced binary would fail every server's checksum; a bad release is fixed by the next version); with no tag, it
only redeploys the indexes.

First-time setup, once:

1. Generate a key pair with `minisign -G -p minisign.pub -s minisign.key`, commit `minisign.pub`, and keep
   `minisign.key` and its password out of the repository.
2. Add the repository secrets `MINISIGN_SECRET_KEY` (the whole contents of `minisign.key`) and `MINISIGN_PASSWORD`.
   Without them, or without `minisign.pub`, nothing is published.
3. Set *Settings → Pages → Source* to *GitHub Actions*.
4. After the first server and runtime releases, remove `release-as` from both packages in
   `release-please-config.json`.

## Upgrading the server, and the trial

`release switch server@0.2.0` (or a group that includes the server) upgrades by handoff:

1. The keeper starts a generation of the new binary on the same sockets. It must report ready within
   `ready_timeout`, or the upgrade is refused and the old generation keeps serving.
2. The old generation stops accepting (HTTP/2 `GOAWAY`; HTTP/1.1 answers carry `Connection: close`; HTTP/3
   connections are closed with code `0x5353`, so clients reconnect) and lets open requests finish, up to
   `drain_timeout`.
3. The new generation takes over every listener, and the new version is **on trial** for `trial_period`:
   `status` shows `trial`.
4. If the new generation exits during the trial, the old one resumes serving at once. The version is marked failed,
   and the other apps switched with it in the same group are switched back.
5. If it survives the trial, the version is confirmed, the old generation shuts down, and the keeper re-executes
   itself from the new binary (same PID, same sockets, no interruption). `status` then shows the new keeper version.

```
$ speedyspec-server release switch server@0.2.0
switched server 0.2.0 (on trial for 2m)
```

One upgrade at a time: another is refused while a trial runs or the previous generation is still draining, and
other release commands are refused while the new generation starts. If the keeper is stopped (`SIGTERM`) at any point,
every generation drains and the keeper exits. A binary
that needs a newer keeper protocol than the running keeper speaks is refused before any handoff. `release rollback
server` is an upgrade to an older kept binary, with the same trial.

## `reload`

`reload` re-reads the config and hands off to a new generation of the confirmed binary, with the same trial. Use it
for mounts, apps, `[proxy]`, `[access]` and the other settings:

```
$ speedyspec-server reload
reloaded (new generation on trial for 2m)
```

Listener changes (address, TLS on or off, `http3`) need a full restart, since the keeper owns the sockets, and are
refused: `listener changes need a full restart (…)`. A reload is refused in immutable mode.

`SIGHUP` to the keeper reloads TLS certificates in place, without a handoff. `SIGTERM` and `SIGINT` drain the current
generation and exit.

## systemd

[`docs/systemd/speedyspec-server.service`](docs/systemd/speedyspec-server.service) runs the keeper with
`Type=notify`: it sends `READY=1` once the first generation serves. `systemctl reload` runs `speedyspec-server reload`.
`KillMode=mixed` sends `SIGTERM` to the keeper only, which drains its generation; anything left is killed after
`TimeoutStopSec`. Install a config at `/etc/speedyspec/server.toml`; the unit's `StateDirectory` and
`RuntimeDirectory` create `/var/lib/speedyspec` and `/run/speedyspec`.

## Containers

Run `speedyspec-server run --config …` as the container's main process, in immutable mode with each app's files baked
into the image. Upgrade and roll back by rolling out a different image with the `speedyspec` CLI. On Kubernetes with a
writable volume for `data_dir`, managed mode works as on a host.

## Kubernetes: several replicas (managed)

Several replicas can share one `data_dir` volume. One pod leads, elected through a `coordination.k8s.io/v1` Lease: it
alone downloads, switches and writes `state.json`. The others follow and serve what it makes live. Add `[cluster]` to
the config:

```toml
[cluster]
leadership = "kubernetes"              # "single" (the default without [cluster]) is one host, always the leader
pod = "web-0"                          # default: $POD_NAME, else the host name
lease_name = "speedyspec-server"
lease_duration = "15s"
renew_deadline = "10s"                 # a leader that has not renewed by then stops writing; below lease_duration
retry_period = "2s"
poll_interval = "2s"                   # how often followers read state.json
prepare_timeout = "60s"                # how long a switch waits for every live pod to prepare
turn_timeout = "10m"                   # how long one pod's rollout turn may take before the rollout fails
probe = "0.0.0.0:9090"                 # /livez and /readyz
```

[`deploy/kubernetes/`](deploy/kubernetes) has the ServiceAccount, the Role (`create` on Leases; `get` and `update` on
the one Lease), the RoleBinding, the PVC, a three-replica Deployment wired to the probes and `POD_NAME`, and a Service.
The `Dockerfile` builds the image (a static binary on an empty base).

- **The volume must be ReadWriteMany** (NFS, CephFS, EFS, Azure Files, …) when pods run on several nodes. Only the
  leader writes shared files, and no pod takes `flock` on the volume.
- **Run release commands on the leader.** A follower refuses them, naming it
  (`this pod is not the leader; run release commands on web-0`). `status`, `release list` and `release check` work
  on any pod, and `status` adds a `cluster` line and one `member` line per pod. `reload` works on every pod.
- **App switches happen in two steps.** Every pod prepares the new set, then all switch within one poll. A pod that
  cannot prepare it keeps its old set and answers `/readyz` with 503 until a set it can prepare is active.
- **Server upgrades roll pod by pod**, by name with the leader last, each finishing its trial before the next.
  `release switch server@0.2.0` answers `rolling out server 0.2.0 across 3 pods (…), one at a time`. A failed trial
  on any pod reverts the pods already upgraded and marks the version failed.

See [`docs/cluster.md`](docs/cluster.md) for how leaders, followers, epochs and rollouts work.

## PHP apps

An app with `kind = "php"` runs on a pool of supervised PHP workers, per release, over FastCGI. The workers are the
PHP runtime (`speedyspec-php-runtime`: an embedded, static PHP with every extension WordPress needs, and WordPress's hook
functions, `WP_Hook` and a service container native, in Rust), released as an app of its own with `kind = "runtime"`.
The server itself holds no PHP.

Its settings are under `[app.<id>.php]`, as in the example configuration above.

See [`docs/php.md`](docs/php.md) for the build, the release's `php` section, the pool settings, the site directory and
`php.bindings`, and [`docs/architecture.md`](docs/architecture.md) for how WordPress is being ported.

## Development

```
scripts/build-php.sh                                   # once: the static PHP, into .php-build/
cargo test --workspace --features speedyspec-server/test-hooks
```

On Linux musl, `scripts/linux-php.sh scripts/build-php.sh` and `scripts/linux-php.sh cargo test …`. Without the static
PHP, the server still builds (`scripts/check-no-php-in-server.sh` checks it holds none); its tests start the runtime's
workers, so they run where PHP is built (as the CI's `php` job does).

`test-hooks` turns on the hooks the keeper tests need (fake versions and protocols, crash and never-ready switches, a
slow streaming route, and the one-host lease file `leadership = "file"` the multi-process cluster tests use). It is
never on in a release build. The tests pass on macOS arm64 and Linux musl. TLS uses `ring`;
`cargo tree -i aws-lc-sys` prints nothing. See [`docs/handoff.md`](docs/handoff.md) for how the handoff works.

The Kubernetes end-to-end test runs on a throwaway k3s in Docker:

```
scripts/k3s.sh up
SPEEDYSPEC_E2E_KUBECONFIG=$(scripts/k3s.sh kubeconfig) \
  cargo test -p speedyspec-server --features test-hooks --test kubernetes_e2e -- --ignored --test-threads=1
scripts/k3s.sh down
```
