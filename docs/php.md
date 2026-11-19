# PHP apps

A `kind = "php"` app is served by a pool of worker processes per release. The workers are the PHP runtime,
`speedyspec-php-runtime worker`: each embeds PHP once and answers FastCGI on its own Unix socket, one request at a time.
The server holds no PHP; it starts, supervises and upgrades the runtime's processes.

## The PHP runtime

The runtime is a separate binary, released as an app of its own with `kind = "runtime"`: one binary per target
triple, like the server's, kept as `<release>/speedyspec-php-runtime` (in immutable mode, `path` is that release
directory). `speedyspec-php-runtime manifest` prints `{ "version", "contracts", "php" }`. Every php app runs on one
runtime app (`app.<id>.runtime`, or the only one configured), and a php release can require its versions with
`"php": { "runtime": "^1.4" }`.

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

`scripts/build-php.sh` builds a static, non-thread-safe PHP (the version in `php-version`) with static-php-cli, into
`.php-build/` (`scripts/linux-php.sh scripts/build-php.sh` for Linux musl, into `.php-build-linux*/`). The extension
list is every extension WordPress's Site Health checks for, plus what WordPress otherwise relies on:

```
bcmath ctype curl dom exif fileinfo filter gd iconv imagick intl mbstring mysqli mysqlnd opcache openssl pdo
pdo_mysql pdo_sqlite phar session simplexml sodium sqlite3 tokenizer xml xmlreader xmlwriter zip zlib
```

with gd's JPEG, PNG, WebP, AVIF and FreeType support. `cargo test -p speedyspec-php --test extensions` checks that
every one loads. The `speedyspec_wordpress` extension (the Rust side: the container, the Hook API) is built in.

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

## `php.bindings`

A port such as the hook store can be implemented in PHP: a class implementing the port's PHP interface (its twin, for
the hook store `SpeedySpec\WpHooks\HookStorage`), bound under the port's key. Ports that hold state for a request can
only be overridden before their first use in it, so bind them here, where they are installed at the start of every
request. A Rust caller of a port with no PHP binding never enters PHP.

## The server's own calls

Each pool keeps two workers beyond `workers.max` for the server's own calls: one for health probes, one for the gate's
questions and migrations. A pool full of visitors never starves them, and gate calls never delay a probe. These calls
are internal requests to the app's first mount, with no client address; their paths (the release's probes, the gate's
revocation feed, the migration) answer 404 to visitors and never reach PHP, nor does any path below them or after a
`.php` segment (`/migrate.php/x`, `/index.php/migrate.php`). Internal paths come from the live release: a release that
still ships its migration script must keep declaring it.

## Health and maintenance

A php release may name its probes: `"php": { "health": { "live": "/healthz", "ready": "/readyz" } }`. The server calls
both every `health.interval`; `health.fail_after` failed rounds in a row (a non-200 answer, or none within
`health.timeout`) put the app in maintenance, `health.pass_after` passed rounds end it. An unhealthy app makes `/readyz`
answer 503, so a load balancer stops sending this replica traffic.

Maintenance is per app, on while any of these is: the admin API's site state (for `gate.maintenance_apps`, or every
app), a switch to a release that migrates (or one whose migration failed), an operator's
`speedyspec-server maintenance on [app...]` (`off`, and `clear` to end a failed migration's too; in the state file, so
it survives restarts), or failing health. While the gate app fails its health checks, every gated mount is in
maintenance. An app in maintenance answers 503 with `Retry-After`: the maintenance page of the static app it names
(`app.<id>.maintenance_page`; that release's `"static": { "maintenance_page": "maintenance.html" }`, one self-contained
file held in memory), or a small JSON body.

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
