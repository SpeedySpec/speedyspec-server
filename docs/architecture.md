# Architecture: porting WordPress to Rust

WordPress is being ported to Rust, one area at a time, with PHP embedded for what remains. The first area is the
Hook API. This document is the pattern every later area follows.

## Ports and Adapters

The code is hexagonal (Ports and Adapters), with Clean Code, CQRS for use cases, and the Repository and Service
patterns. There is no DDD layering.

- **Every WordPress function and method is a use case**: a command (it changes state) or a query (it reads state),
  with one handler. `add_filter()` is the command `AddFilter`; `has_filter()` is the query `HasFilter`.
- **Inbound ports** are the use cases. **Outbound ports** are what the core needs from the world (`HookStore`).
- **Adapters** implement outbound ports with a technology (`MemoryHookStore`, `PhpHookStore`), or expose inbound
  ports to one (`WpHooksArea`: WordPress's PHP functions over the Rust handlers).
- A port and its adapters never share a crate. Salvo, `kube` and `ext-php-rs` appear only in adapter crates and the
  binary.

### Naming

| Kind | Name | Examples |
|---|---|---|
| Inbound port (use case) | the operation | `AddFilter`, `ApplyFilters`, `DidAction` |
| Its handler | the operation + `Handler` (the container key) | `SpeedySpec\WpHooks\AddFilterHandler` |
| Outbound port | a capability noun | `HookStore`, `DeprecatedHooks` |
| Adapter | the technology first | `MemoryHookStore`, `PhpHookStore`, `WpHooksArea` |
| Domain service | what it decides | `PriorityOrder`, `CallbackIdentity`, `HookIteration` |

Nothing is named `…Service`, `…Contract`, `…Interface` or `…Manager`.

### Crates, for one area

| Crate | Holds |
|---|---|
| `speedyspec-wp-hooks` | the ports, the use cases and their handler (`WpHookApi`), the domain services |
| `speedyspec-wp-hooks-memory` | `MemoryHookStore` |
| `speedyspec-wp-hooks-php` | the PHP side: WordPress's functions, `WP_Hook`, the globals, `PhpHookStore` |

## CQRS

`speedyspec-cqrs` has a `CommandBus` and a `QueryBus`. Each resolves a message's handler from the container by the
message's `KEY` and runs the bus's middleware around it, in registration order. Hot paths use direct dispatch:
`HandlerRef<C>` (and `QueryRef<Q>`) resolves the handler once, calls it with no middleware, and resolves it again only
when its binding changes. WordPress's PHP functions call their handlers through `HandlerRef`s.

## Hot paths across the boundary

A function PHP calls often (WordPress's `apply_filters()`, `do_action()`, `add_filter()`) must cost less than the PHP
it replaces. The Hook API's functions are the model:

- **PHP values stay PHP values.** A hot native function reads its arguments itself (`native::raw_function`) and
  converts only what Rust reads: the hook name, the priority, the accepted-args count, a callback's identity. Everything
  else passes through as the zval PHP gave (`Value::Opaque`, one more reference, no copy) and goes back to PHP as the
  same zval. A Rust callback reads such a value only when it runs (`Value::read`).
- **Nothing to do returns early.** A hook with no callbacks (and no `all` hook) counts the run and hands the value
  straight back, before any argument list is built (`WpHookApi::skip_if_empty`).
- **PHP's hash is the hash.** A hook name from PHP keeps the hash its `zend_string` carries (`HookKey::prehashed`);
  the store hashes nothing again. A name made in Rust is hashed the same way PHP would.
- **Sort when adding, walk when firing.** Priorities are kept in `ksort(SORT_NUMERIC)` order as callbacks are added
  and removed; a run shares the sorted lists and copies one only if a callback changes it mid-run.
- **Resolve a callback once.** A PHP callback is resolved per request into a `zend_fcall_info_cache` and called
  with `zend_call_function()` (`speedyspec_php::Callable`), not through `call_user_func_array()` on every call.
- **Count the crossings.** `boundary_crossings()` is the measure: an empty hook costs none, a PHP callback one (plus
  one push and one pop of `$wp_current_filter` per run). A change that adds crossings on a hot path is a regression,
  and the area's tests assert the counts.

## The service container

`speedyspec-container` has the functionality of Laravel's container (no Laravel compatibility is promised): `bind`,
`singleton`, `scoped`, `instance`, `alias`, `extend`, contextual bindings (`when()->needs()->give()`), tags,
resolving callbacks, `make_with` parameters, and autowiring of PHP classes through `ReflectionClass`.

**Lifetimes.** Every registration is `Worker` (for the life of the process) or `Request` (until the request ends).
Everything PHP registers is `Request`. Ending a request removes every request registration; each key answers with its
worker registration again, also after a fatal error.

**The override.** A port's Rust adapter is bound for the worker. PHP overrides it by binding the same key: a PHP
class that implements the port's PHP *twin* (an interface generated from the Rust trait by `#[php_port]`). The
container records that the binding is foreign; resolving it wraps the PHP object in the port's PHP adapter, so Rust
callers get the PHP implementation for that request. A key with no PHP binding resolves to Rust and **never enters
PHP**; every area has a test that counts PHP-boundary crossings and asserts zero. Ports that hold state for a request
(`StatefulPorts`, such as the hook store) can only be overridden before their first use, or for every request in
`app.<id>.php.bindings`.

## Seams meet only through the Container

A **seam** is one area (hooks, options, the database, kses, …). Its crates reach another seam only through that seam's
**contracts** (port traits, keys, value types) and resolve the implementation from the container, so any dependency
can be replaced: by another Rust implementation in configuration, or, for an edge port, by PHP.

**Roles.** Every crate declares `[package.metadata.speedyspec] seam = "…"` and `role = "…"`: `contracts`,
`implementation`, `adapter`, `php` (a seam's native area and provider), `platform` (the kernel every seam may use:
the container, the PHP engine bindings, the macros), `test`, or `root` (the server and the PHP runtime, the only
crates that name implementations of several seams). `scripts/check-seams.sh` fails CI when a crate depends on another
seam's crate that is neither contracts nor platform, when a contracts crate depends on an implementation or links
PHP, or when a crate has no metadata. `scripts/seam-allowlist.txt` lists the edges that predate the rule; each seam's
migration deletes its own lines, and an entry that is no longer a real edge fails the check.

**Ports and proxies.** A contracts crate declares each port with `#[port]` (from the PHP-free `speedyspec-port-macros`):

```rust
#[port(key = "SpeedySpec\\WpOptions\\OptionRepository", edge)]
pub trait OptionRepository {
    fn load(&self, name: String) -> Option<String>;
}
// Generated: OPTION_REPOSITORY_KEY, and ProxyOptionRepository, which forwards each call to whatever the container
// binds under the key now.
```

A provider hands other seams `Rc::new(ProxyOptionRepository::new(&container))` as their `Rc<dyn OptionRepository>`;
domain code never sees the container. A proxy forwards through a `PortCell`, which caches the resolved port against
the container's **epoch**, a counter that moves on any binding change and at every request start and end. A call with
nothing rebound costs one compare; the first call of each request resolves through the container (so a stateful
port's use is recorded and ADR-017's refusal to rebind it holds); transient bindings are built on every call.

**Edge ports and PHP.** An *edge* port's types convert to PHP (storage, engines, readers, policies). Its `-php` crate
generates the twin and the PHP adapter, and registers the adapter, with one line:

```rust
speedyspec_wp_options_contracts::__speedyspec_port_OptionRepository!(::speedyspec_php_port_macros::php_twin);
```

PHP then replaces it with `Container::getInstance()->bind(OptionRepository::class, …)`. *API* ports, whose types are
Rust-only (callbacks, rich domain values), are replaced from Rust only; PHP replaces the edge ports behind them.

**Providers.** Each seam's `-php` crate implements `speedyspec_container_php::Provider`: `register` binds the seam's
implementations (binding only, so order does not matter), `stateful` names its stateful ports, and `area` builds its
native area from proxies. `speedyspec-php-runtime`'s worker lists providers and calls `compose`; a seam that cannot run
in the configuration (`ProviderError::Unavailable`) is left out and logged, and one that fails stops the worker.

## Recorded compatibility

WordPress's behaviour is not reimplemented from reading its code alone; it is recorded and replayed.

1. **A catalogue** (`compat/<area>/scenarios.json`): scenarios of calls, with callback fixtures ("append `-a`",
   "remove myself"), covering every function and method and the edge cases (priorities, recursion, changing a hook
   while it runs, the globals).
2. **The recorder** (`cargo run -p speedyspec-compat -- record <area> --wordpress <version>`) fetches WordPress
   `<version>` from the release table (`compat/wordpress.json`, checked by SHA-256), runs every scenario against its
   original code in the embedded PHP, and writes `compat/<area>/wordpress-<version>.json`: every return value, every
   callback invocation, and the globals after each step.
3. **The replayer** (`speedyspec_compat::replay`) runs the scenarios against the port and compares each step
   exactly. A difference fails unless `compat/<area>/breaks.json` lists it as a documented break.
4. WordPress's own PHPUnit tests for the area also run against the port (`tests/php/`).
5. Then the area's WordPress implementation is deleted. The fixture and the replayer stay as the compatibility tests.

The driver that runs scenarios (`speedyspec-compat/src/driver.php`) is the same on both sides, and normalises what
depends on the implementation (object ids in callback ids, closures) to fixture labels.

### The WordPress release

`compat/wordpress.json` is the one release table: `wordpress`, the release the repository is pinned to, and
`releases`, each release's tarball URL and SHA-256. Every area, `speedyspec_compat::wordpress_release`, and the kses
testkit resolve releases from it. An area's `compat/<area>/source.json` is `{}`, or `{ "wordpress": "<version>" }`
to keep that one area on another release in the table; `speedyspec_compat::area_wordpress` returns the override or
the pin, and the replayer reads that release's fixture.

Moving to a new release:

1. Add its entry (URL and SHA-256) to `releases` in `compat/wordpress.json` and change `wordpress` to it.
2. Re-record each area (`cargo run -p speedyspec-compat -- record <area> --wordpress <version>`; posts and meta with
   their `record.php`), and regenerate the data two crates embed at compile time. `include_str!` and
   `include_bytes!` take only literals, so each names the release in a `data_release!` macro, and a test fails until
   the macro equals the area's release: `speedyspec-wp-kses` (`data/allowed-<release>.json`, from
   `scripts/generate-allowed.php`) and `speedyspec-wp-theme-json-contracts` (`wordpress-<release>.*`, from `tools/`).
3. Replay. A difference is a change in WordPress: follow it in the port, or list it in the area's `breaks.json`
   with the reason.

### The Hook API's documented break

Replacing `$wp_filter` or `$wp_filters` wholesale (`$wp_filter = [...]`) makes it a plain PHP array that the Rust
store no longer sees, and `$wp_filter[$name]` holds only `WP_Hook` objects. `$wp_filter`, `$wp_actions` and
`$wp_filters` are proxy objects over the store (reading, indexing, iterating, counting and appending work as in
WordPress; passing one to an array function does not). `$wp_current_filter` is a real PHP array, kept equal to the
store wherever PHP can see it.

## Two binaries

`speedyspec-server` holds no PHP: it serves static apps from memory, routes requests, gates the admin, and passes PHP
requests over FastCGI to `speedyspec-php-runtime`'s workers, which it starts, supervises and upgrades like any release.
The runtime is where the Rust WordPress API grows, area by area, behind the embedded PHP; it releases often, the server
rarely. `scripts/check-no-php-in-server.sh` keeps it that way.

- `speedyspec-runtime-contract`: what the two binaries share (the worker spec, the per-request report, the contract
  major).
- `speedyspec-php-runtime`: the runtime binary (`worker`, `manifest`).
- `speedyspec-server-gate`: the admin gate, health and maintenance, framework-neutral, over the `GateCalls` port the
  server implements over FastCGI.
