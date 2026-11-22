# PHP apps

A `kind = "php"` app is served by a pool of worker processes per release. The workers are the PHP runtime,
`speedyspec-php-runtime worker`: each embeds PHP once and answers FastCGI on its own Unix socket, one request at a time.
The server holds no PHP; it starts, supervises and upgrades the runtime's processes.

## The PHP runtime

The runtime is a separate binary, released as an app of its own with `kind = "runtime"`: one binary per target
triple, like the server's, kept as `<release>/speedyspec-php-runtime` (in immutable mode, `path` is that release
directory). `speedyspec-php-runtime manifest` prints `{ "version", "contracts", "php" }`. Every php app runs on one
runtime app (`app.<id>.runtime`, or the only one configured), and a php release can require its versions with
`"php": { "runtime": "^1.4" }`. This repository publishes it (README, "Published releases of the server and the PHP runtime"):
its `source` is `https://releases.speedyspec.com/php-runtime/index.json`, for an app with the id
`php-runtime`.

- **The contract.** The server and the runtime share `speedyspec-runtime-contract`: the worker's start
  (`worker --spec-stdin`, the spec as JSON on standard input, so an app's credentials never show in the process list), FastCGI on the worker's 0600 socket, and the per-request report (an `FCGI_STDERR` record
  starting `\0speedyspec:`). Its major is 1. The server refuses a runtime whose `contracts` lack it, and a worker
  refuses a spec of another major; adding a field is a minor change the other side ignores.
- **Switching the runtime** starts every php app on it again from the new binary, swaps them all at once, and drains
  the old pools. A runtime is switched on its own, never in the same step as an app on it. For a runtime major whose
  range no current app release accepts, first release and switch app versions whose range accepts both majors
  (`">=1, <3"`), then switch the runtime.
- **The trial.** A new runtime release is on trial for `runtime.trial_period` (2 minutes). If a php app on it turns
  unhealthy during the trial, the runtime switches back and the new release is marked failed (auto-update never offers
  it again). In cluster mode one replica's report rolls back every replica; independent replicas each roll back
  themselves.

## The PHP build

`scripts/build-php.sh` builds a static, non-thread-safe PHP (the version in `crates/speedyspec-php/php-version`) with static-php-cli, into
`.php-build/` (`scripts/linux-php.sh scripts/build-php.sh` for Linux musl, into `.php-build-linux*/`). The extension
list is every extension WordPress's Site Health checks for, plus what WordPress otherwise relies on:

```
bcmath ctype curl dom exif fileinfo filter gd iconv imagick intl mbstring mysqli mysqlnd opcache openssl pdo
pdo_mysql pdo_sqlite phar session simplexml sodium sqlite3 tokenizer xml xmlreader xmlwriter zip zlib
```

with gd's JPEG, PNG, WebP, AVIF and FreeType support. `cargo test -p speedyspec-php --test extensions` checks that
every one loads.

**ext-sodium is guaranteed.** WordPress needs libsodium, and loads the `sodium_compat` polyfill when ext-sodium is
missing. The runtime never needs that polyfill, because it never runs without ext-sodium:

- `build-php.sh` fetches libsodium 1.0.22 itself, checks its SHA-256 (verified against the release's minisign
  signature), and hands that tarball to spc. Afterwards it fails unless `lib/libsodium.a` exists, `link-flags.txt`
  has `-lsodium`, spc built from the pinned tarball, and PHP reports `SODIUM_LIBRARY_VERSION` 1.0.22.
- `speedyspec-php`'s build script refuses a PHP prefix without `-lsodium` or `libsodium.a`.
- The `extensions` test checks the version too.
- A runtime worker checks ext-sodium at start-up and exits with `php_worker_sodium_missing` without it, and the
  release smoke check fails a runtime whose manifest doesn't report the pinned libsodium.

The Rust crypto port (`speedyspec-wp-crypto*`) links the same `libsodium.a`, so PHP and Rust share one libsodium:
`.cargo/config.toml` sets `SODIUM_LIB_DIR` to the buildroot's `lib`, and `speedyspec-php`'s build script refuses any
other directory. Point `SODIUM_LIB_DIR` at `<prefix>/lib` whenever you set `SPEEDYSPEC_PHP_PREFIX`, `PHP` and
`PHP_CONFIG` to another PHP build.

The `speedyspec_wordpress` extension (the Rust side: the container, the Hook API) is built in.

### FFmpeg

The runtime reads audio, video and image metadata with FFmpeg instead of WordPress's getID3 (see
[licensing](licensing.md) for the licence and how releases meet it). `scripts/build-ffmpeg.sh` builds it, and `build-php.sh` runs it at its end, so
building PHP builds FFmpeg too (into `.php-build/ffmpeg/prefix`, or `.php-build-linux*/ffmpeg/prefix` in the Linux
container). Run it alone to rebuild FFmpeg against an existing PHP build; `--print-configure` prints its configure
arguments.

- **Pinned.** FFmpeg 9.1 from a SHA-256-pinned tarball, checked against FFmpeg's release signature. A changed pin or
  configure argument changes the build stamp and rebuilds.
- **Metadata only.** `--enable-small --disable-everything`; no programs, network, protocols, encoders, muxers or
  external libraries; `libavformat`, `libavcodec` and `libavutil` only. Nothing GPL, version 3 or nonfree, so FFmpeg
  stays LGPL 2.1 or later.
- **What it reads.** Every demuxer that reads one byte stream, except a deny list in the script: network and manifest
  formats (HLS, DASH, RTSP, …), formats that open other files (`concat`, `image2` and its variants, `ffmetadata`,
  IMF, VobSub), formats that accept any bytes or need a frame size from outside (raw video, header-less PCM, `tty`,
  `data`, `v210`), text art and synthesis scripts, and subtitles and captions. All parsers, and the image header
  decoders width and height need. A deny entry the pinned source lacks stops the build (a rename would let the
  renamed demuxer in), and `crates/speedyspec-ffmpeg-sys/demuxers.txt` lists every demuxer reviewed for the build:
  the link test fails on any demuxer not in it, so an FFmpeg upgrade's new demuxers are reviewed one by one.
- **One zlib.** FFmpeg links the PHP buildroot's `libz.a`; `speedyspec-ffmpeg-sys`'s build script links nothing from
  outside the FFmpeg and PHP prefixes (`FFMPEG_PREFIX` in `.cargo/config.toml`; move it with the PHP variables).
- **Checked.** The script fails on a GPL, version 3 or nonfree flag, an enabled protocol, or zlib bundled into FFmpeg;
  `cargo test -p speedyspec-ffmpeg-sys` checks the built library: its licence, the formats WordPress accepts, no
  protocols, none of the denied demuxers, exactly the reviewed demuxers, and the pinned version.

Sizes (FFmpeg 9.1, `-Os` via `--enable-small`, measured 2026-11-22):

| Target | `libavcodec.a` | `libavformat.a` | `libavutil.a` | Stripped binary growth, everything linked |
|---|---|---|---|---|
| macOS aarch64 | 2.69 MB | 2.30 MB | 0.89 MB | 2.71 MB |
| Linux aarch64 (musl) | 2.88 MB | 2.67 MB | 0.99 MB | 3.08 MB |

Linux x86_64 is built by CI's `php (linux-x86_64)` job.

## The release

A php release's `speedyspec-release.json` may have a `php` section; without one, the defaults apply:

```json
"php": { "document_root": "public", "entry": "index.php", "ini": { "memory_limit": "256M" } }
```

| Field | Default | Meaning |
|---|---|---|
| `document_root` | `.` | the directory URLs map into |
| `entry` | `index.php` | the front controller under `document_root`; a release without it is refused at unpack |
| `ini` | none | INI settings for the release's workers, applied at boot as php.ini would (`upload_max_filesize` included) |
| `cron` | none | `{ "path": "/cron.php" }`: the path the server's cron scheduler calls (see "Scheduled cron calls") |
| `internal` | none | more paths only the server may call, such as `["/wp-cron.php"]`: visitors get 404 |

A release that uses `cron` or `internal` must declare `"requires": { "<the server app's id>": "^<the version that ships
them>" }`: an older server refuses manifest fields it does not know.

## Requests

A path that names an existing `.php` file under the document root, or under `site_dir/wp-content`, runs it
(`PATH_INFO` is what follows the file); a directory with an `index.php` runs that (named without its trailing slash, it
redirects to it first); another existing file is served as is, from `site_dir/wp-content` first, then the release;
every other path goes to `entry`. Nothing in `site_dir` outside `wp-content` is ever served, and no path with a
dot-segment (`.env`, `.git/`) is served or run. The worker gets the CGI/1.1 parameters as `$_SERVER`, the body on
`php://input`, and sends back the status and headers from `header()`/`http_response_code()`. Both bodies stream: a
slow client holds the worker back rather than being buffered, and the whole request is bounded by `request_timeout`.

Each request starts clean: request-lifetime bindings, hook callbacks and globals end with the previous request, also
after a fatal error. A script that dies before sending output is a 500; a worker that crashes is a 502 and is
replaced; a request past `request_timeout` is a 504 and its worker is killed.

## The hook globals

`$wp_filter`, `$wp_actions` and `$wp_filters` are proxy objects over the Rust hook store: reading, indexing,
`isset`, iterating, counting and appending work as in WordPress, but passing one to an array function
(`array_keys($wp_filter)`, `in_array(…, $wp_actions)`) throws a `TypeError`, and replacing one wholesale is not seen
by the store. `$wp_current_filter` is a real array, kept equal to the store wherever PHP can see it. These are listed
in `compat/hooks/breaks.json`.

## Pool settings

```toml
[app.blog.php]
workers = { min = 1, max = 4 }
max_requests = 1000
request_timeout = "60s"
memory_limit = "256MiB"
idle_timeout = "10s"
site_dir = "/var/lib/speedyspec/sites/blog"
bindings = { "SpeedySpec\\WpHooks\\HookStore" = "App\\HookStore" }
```

| Key | Default | Meaning |
|---|---|---|
| `workers` | `{ min = 1, max = 4 }` | `min` always run; up to `max` start on demand; requests wait while all are busy |
| `max_requests` | 1000 | a worker is recycled after this many requests |
| `request_timeout` | 60 s | a longer request is a 504 and its worker is killed |
| `memory_limit` | 256 MiB | a worker whose peak memory passes this after a request is recycled |
| `idle_timeout` | 10 s | an idle worker above `min` exits after this |
| `site_dir` | required in managed mode | the app's writable directory |
| `bindings` | none | PHP classes bound to ports for every request, before any PHP runs |

A release switch starts the new release's pool before it goes live. Requests already running finish on the old
release's pool, which then drains: no new requests, its workers exit after their last one.

Each pool's worker sockets live in a directory of its own, private to the server's user:
`<run_dir>/php/<app>/<version>/<pid>-<n>/`, or a fresh `mkdtemp` directory under `/tmp` when `run_dir` does not exist
or the path would be too long for a socket. Workers exit when the server that started them is gone.

Each event is a log line: `php_pool_started`, `php_worker_started`, `php_worker_exited`, `php_request_timeout`,
`php_worker_recycled`, `php_pool_drained`; PHP's own errors arrive as `php_stderr`.

## The site directory

`site_dir` holds what an app writes: uploads, caches, `wp-content`, and its secrets. Workers define `WP_CONTENT_DIR`
as `site_dir/wp-content` (WordPress derives `WP_CONTENT_URL` from `siteurl`). Only `site_dir/wp-content` is served,
before the release's own `wp-content`.

A worker's environment is merged by the server when a pool starts, a later source winning for a key:
`site_dir/.env` (`KEY=value` lines, nothing evaluated), then `app.<id>.php.env_file`, then `app.<id>.php.env`. That is
how each app gets its own credentials: the admin API a database user that can write, the front-end API and the blog
only what they need. Of the server's own environment, a worker keeps only `PATH`, `HOME`, `USER`, `TMPDIR`, `TZ`,
`LANG` and `LC_ALL`. Credentials are isolated between apps only as far as their workers are: pools of one OS user can
read each other's process environment.

### Native WordPress areas

`DB_NAME` in a worker's environment turns on the database, object cache, options and cron areas: native `$wpdb`,
`wp_cache_*`, `get_option()`, `wp_schedule_event()` and the rest, over one engine kept for the worker's life. Cron's
writes to the `cron` option are compare-and-swap: two workers (or replicas) scheduling at once both keep their
events. The variables are
WordPress's `wp-config.php` names:

| Variable | Default |
|---|---|
| `DB_NAME` | none: without it the worker serves without these areas |
| `DB_USER` | empty |
| `DB_PASSWORD` | empty |
| `DB_HOST` | `localhost` (host, `host:port` or `host:/path/to/socket`, as WordPress parses it) |
| `DB_CHARSET`, `DB_COLLATE` | none |
| `TABLE_PREFIX` | `wp_` (letters, digits and underscores only) |

The configuration is read from the environment, not from `wp-config.php`, because the areas are fixed when PHP boots.
The app's `wp-config.php` reads the same variables. The worker logs and exits when it cannot use the database:
`php_worker_boot_failed` for a refused configuration, `php_worker_database_unreachable` when the engine does not
answer at start. Each request is answered 500 without running PHP while a `db.php` drop-in exists where
`WP_CONTENT_DIR` points: `site_dir/wp-content`, or the release's `wp-content` without a `site_dir`
(`php_worker_drop_in_refused`). A transaction a request leaves open is rolled back when it ends
(`php_worker_open_transaction`).

A release served this way must not load WordPress's own `option.php`, `cache.php` or `class-wpdb.php` functions beside
the native ones (nor `cron.php`'s ported functions): each port's `prepare_release` writes a copy of WordPress without
them.

## `php.bindings`

A port such as the hook store can be implemented in PHP: a class implementing the port's PHP interface (its twin, for
the hook store `SpeedySpec\WpHooks\HookStorage`), bound under the port's key. Ports that hold state for a request can
only be overridden before their first use in it, so bind them here, where they are installed at the start of every
request. A Rust caller of a port with no PHP binding never enters PHP.

## The server's own calls

Each pool keeps workers beyond `workers.max` for the server's own calls: one for health probes, one for the gate's
questions and migrations, and one for scheduled cron calls, started the first time the scheduler calls. A pool full of
visitors never starves them, gate calls never delay a probe, and a long cron run holds only its own worker. These calls
are internal requests to the app's first mount, with no client address; their paths (the release's probes, the gate's
revocation feed, the migration) answer 404 to visitors and never reach PHP, nor does any path below them or after a
`.php` segment (`/migrate.php/x`, `/index.php/migrate.php`). Internal paths come from the live release: a release that
still ships its migration script must keep declaring it.

## Scheduled cron calls

The server calls an app's `php.cron` path (internal, `GET <path>?budget=<seconds>`, on the cron worker) when the app
says it is next due: every worker report may carry `next_cron`, the earliest Unix time the request learned, and the
app's scheduler wakes then. Any PHP code can report one with `\SpeedySpec\Runtime\report_next_wake( int $timestamp )`;
the runtime's native WordPress cron does so whenever it writes the `cron` option, and its runner when it finishes. The
server never reads the app's data: it knows a path, a time and a number in a report.

```toml
[app.admin-api.php.cron]
enabled = true          # default: call when the live release declares php.cron
max_interval = "60s"    # the longest wait between calls, however late the app says it is next due
timeout = "120s"        # one call's limit
budget = "30s"          # how long a call may run due events before it stops and asks to be called again
```

A report moves the next call earlier, never later; calls are at least a second apart and one at a time. A failed call
is retried after 5 s, doubling up to `max_interval`. No call is made while the app is in maintenance. Every replica
runs its own scheduler; the app makes sure each due event runs once (WordPress's runner claims each event with a
compare-and-swap write), so replicas never coordinate. `max_interval` is how a replica notices events scheduled
through another replica.

## Health and maintenance

A php release may name its probes: `"php": { "health": { "live": "/healthz", "ready": "/readyz" } }`. The server calls
both every `health.interval`; `health.fail_after` failed rounds in a row (a non-200 answer, or none within
`health.timeout`) mark the app unhealthy, `health.pass_after` passed rounds end it. An unhealthy app makes `/readyz`
answer 503, so a load balancer stops sending this replica traffic.

Failing health is an outage, not maintenance: an unhealthy app answers a plain `503 service unavailable`, with no
`Retry-After` and no maintenance page, without reaching PHP. While the gate app fails its health checks, every gated
mount answers the same. If every replica is unhealthy at once (a shared database is down), the load balancer has no
ready replica and answers its own 5xx; either way the visitor sees a server error.

Maintenance is planned, per app, and on while any of these is: the admin API's site state (for `gate.maintenance_apps`,
or every app), a switch to a release that migrates (or one whose migration failed), or an operator's
`speedyspec-server maintenance on [app...]` (`off`, and `clear` to end a failed migration's too; in the state file, so
it survives restarts). An app in maintenance answers 503 with `Retry-After`: the maintenance page of the static app it
names (`app.<id>.maintenance_page`; that release's `"static": { "maintenance_page": "maintenance.html" }`, one
self-contained file held in memory), or a small JSON body. Maintenance wins over failing health: an app in both shows
its maintenance page.

The admin API's site maintenance never applies to the admin itself (the gate app and every app behind a
`signed-in` mount), even when `gate.maintenance_apps` names it: admins sign in and turn it off there, or use the CLI.
The admin goes into maintenance only for a moment: while one of its own releases migrates, an operator's
`maintenance on`, or while the admin API is down (which needs the CLI to fix anyway).

## The admin gate

`access.implementation = "gate"` checks every request to a `signed-in` mount. The live release's access rules decide
first (`"static"` or `"php"` section, `"access": [...]`, the first match winning):

```json
"access": [
  { "match": "", "public": "always" },
  { "match": "signin/**", "public": "always" },
  { "match": "install/**", "public": "not-installed" },
  { "match": "components/{page}/**", "page": "{page}" }
]
```

A pattern is a glob over the path below the mount (`**` across `/`, `*` within a segment, `{page}` a page id,
`<namespace>/<name>`). `public` is `always`, `not-installed` (only while the site is not installed) or `maintenance`
(only while the app is in maintenance); a conditional entry whose condition does not hold answers 404 to everyone.
Anything no rule matches needs sign-in.

Then the gate app (`[gate] app`, the admin API) is asked, at the paths its release names
(`"php": { "gate": { "version": 1, "site": ..., "session": ..., "changes": ... } }`; a gate-app release without it, or
of another version, is refused):

- `GET site` → `{ "installed": bool, "maintenance": bool }`, remembered for `gate.site_ttl`.
- `GET session`, with only the visitor's `Cookie` → `200 { "user": { "id": n }, "pages": ["core/general", ...] }`, or
  `401`. Anything else is not signed in. Signed-in answers are remembered for `gate.session_ttl`, keyed by a hash of
  the whole `Cookie` header; concurrent checks share one call; beyond `gate.max_pending` uncached checks a request gets
  503.
- `GET changes?since=<cursor>` → `{ "cursor": "...", "users": [ids] }` or `{ "cursor": "...", "reset": true }`, polled
  every `gate.poll`: the users named are asked again. While the feed fails, nothing is remembered.

Not signed in is 401, signed in without a page 403, a disabled entry 404, and every gated response is
`Cache-Control: private, no-store`. A gate-app response that clears a cookie its request sent (a sign-out) forgets that
answer before the client hears of it.

## Migrations

`"php": { "switch": { "maintenance": "self", "migrate": "/migrate.php" } }` (or `"site"`) migrates on switch: the server
prepares the new release, turns maintenance on, `POST`s the migrate path to the prepared release (up to
`switch.migrate_timeout`, 2xx is success), then switches and ends maintenance. A failed migration keeps the old release
live and maintenance on until `speedyspec-server release retry-migration <app>` or `maintenance clear <app>`. In cluster
mode the leader migrates once, before any replica switches; on independent replicas each replica migrates, so a
migration must be safe to run more than once and at the same time (a database lock in the app). A release that migrates
must leave the database usable by the release before it, so a rollback stays instant.
