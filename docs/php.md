# PHP apps

A `kind = "php"` app is served by a pool of `php-worker` processes per release. Each worker embeds PHP once and answers
FastCGI on its own Unix socket, one request at a time. Build the server with the `php` feature; without it, a PHP app
answers 503.

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
| `ini` | none | INI settings, applied with `ini_set()` at the start of every request (`PHP_INI_ALL` settings only) |

## Requests

A path that names an existing `.php` file under the document root runs it (`PATH_INFO` is what follows the file); a
directory with an `index.php` runs that; another existing file is served as is, from `site_dir` first, then the
release; every other path goes to `entry`. The worker gets the CGI/1.1 parameters as `$_SERVER`, the body on
`php://input`, and sends back the status and headers from `header()`/`http_response_code()`. Both bodies stream: a
slow client holds the worker back rather than being buffered, and the whole request is bounded by `request_timeout`.

Each request starts clean: request-lifetime bindings, hook callbacks and globals end with the previous request, also
after a fatal error. A script that dies before sending output is a 500; a worker that crashes is a 502 and is
replaced; a request past `request_timeout` is a 504 and its worker is killed.

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

Worker sockets live under `<run_dir>/php/<app>/<version>/`, or under `/tmp/speedyspec-php-…` when `run_dir` does not
exist or the path would be too long for a socket.

Each event is a log line: `php_pool_started`, `php_worker_started`, `php_worker_exited`, `php_request_timeout`,
`php_worker_recycled`, `php_pool_drained`; PHP's own errors arrive as `php_stderr`.

## The site directory

`site_dir` holds what an app writes: uploads, caches, `wp-content`. Workers define `WP_CONTENT_DIR` as
`site_dir/wp-content` and `WP_CONTENT_URL` from the request's host and the mount. Files in `site_dir` are served
before the release's. `site_dir/.env`, if present, is read as `KEY=value` lines (nothing is evaluated) into the
workers' environment, for `getenv()`.

## `php.bindings`

A port such as the hook store can be implemented in PHP: a class implementing the port's PHP interface (its twin, for
the hook store `SpeedySpec\WpHooks\HookStorage`), bound under the port's key. Ports that hold state for a request can
only be overridden before their first use in it, so bind them here, where they are installed at the start of every
request. A Rust caller of a port with no PHP binding never enters PHP.
