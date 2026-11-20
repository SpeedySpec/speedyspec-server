# Native Firewall Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust firewall that the PHP runtime can run before any PHP. It covers IP allow and block lists, rate limiting with bans, pattern rules over the request line, login lockout, XML-RPC blocking and user-enumeration blocking.

**Architecture:** Hexagonal. The domain crate holds the value objects and the ports (counter store, ban store, settings source, clock, log). The app crate holds the rules as a Chain of Responsibility, plus the login guard. Adapters back the stores with the existing `CachingStore` (memory, database, Valkey/ReDB, file) and read settings through the ported options API. A presentation crate adapts the firewall to a new, generic request-stage contract and registers the Rust hook callbacks. This plan does not edit the runtime. It ends with a written wiring request for the runtime's owner.

**Tech Stack:** Rust 1.99, edition 2024. Crates already in the workspace: `speedyspec-cache` (+ `-memory`, `-kv`, `-file`, `-sql`), `speedyspec-data-memory`, `speedyspec-wp-options` (+ `-memory`), `speedyspec-wp-hooks` (+ `-memory`), `speedyspec-hooks`. New external dependencies: `regex` and `ipnet`. Both are already in the workspace through `speedyspec-server-presentation`; reuse those versions.

**Spec:** `docs/superpowers/specs/2026-11-19-native-plugin-features-design.md` in the `wordpress-develop` repo, in particular the sections "Runtime request stages", "Firewall and login security", "Shared state" and "Which apps run which stages".

## Global Constraints

- The work goes in the `SpeedySpec/speedyspec-server` repo. Make a new worktree at `wordpress-develop/packages/speedyspec-server-firewall`, on branch `feat/native-firewall`.
  - Cut it from `origin/feat/persistent-cache-transients-network-options` (PR #19). Once #19 is merged, rebase onto `origin/main`.
  - Never edit another worktree. Never edit `crates/speedyspec-php-runtime`, `crates/speedyspec-php`, `crates/speedyspec-server*`, or the data, cache, options or hooks crates. If a defect turns up in those crates, write it down in the wiring request (Task 7) and work around it.
- New crates go under `crates/` with workspace `edition`, `rust-version` and `license`.
  - Names: `speedyspec-native-stage`, `speedyspec-firewall`, `speedyspec-firewall-app`, `speedyspec-firewall-infra-cache`, `speedyspec-firewall-infra-options`, `speedyspec-firewall-presentation`. Never use a `-core` suffix.
- Dependencies point inward:
  - `speedyspec-firewall` depends only on `std`, `ipnet` and `regex`.
  - `-app` depends only on `speedyspec-firewall`.
  - The infra crates depend on the domain crate and their own backend.
  - Only `-presentation` knows the stage contract and the hook API.
- Contracts live apart from their implementations. Every store port has at least two implementations: the cache adapter, plus a test double in the tests that use it.
- Domain objects are plain structs with no storage code.
- Single-threaded per worker: use `Rc` and `RefCell` like the rest of the workspace. Nothing here is `Send`.
- **The firewall fails open.** A store error, a settings error or a missing client address gives `Continue`, never a block, and is reported once through `FirewallLog`.
- **Blocked responses:**
  - 403: header `Cache-Control: no-store`, `Content-Type: text/plain; charset=utf-8`, body `Forbidden\n`.
  - 429: the same headers plus `Retry-After: <seconds, rounded up, at least 1>`, body `Too Many Requests\n`.
- Option names are listed in Task 5, verbatim.
- Commits: `type(scope): sentence` in lowercase, a prose body, and the last line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Scopes are `native-stage` and `firewall`.
- Patterns used, and where:
  - **Chain of Responsibility:** `Firewall` over `RequestRule`s.
  - **Specification:** `IpSet` and `PatternRule` matching.
  - **Strategy:** store backends behind `CounterStore` and `BanStore`.
  - **Adapter:** the cache and options infra crates, and `FirewallStage`.
  - **Observer:** hook callbacks on `wp_login_failed`.
  - **Factory:** `FirewallComposition`, the composition root.
  - **Memoization:** `FirewallCache`, which rebuilds rules only when the raw settings change.

## Review Focus

1. **IPv4-mapped IPv6 clients** (`::ffff:203.0.113.7`) must match IPv4 entries in the allow and block lists, and must share rate-limit counters with the plain IPv4 address. The test goes in Task 2: `ip_set_matches_ipv4_mapped_ipv6`.
2. **The backing store is down** (every store call errors): every request continues, nothing panics, and the warning is logged once per request. The test goes in Task 3: `store_errors_fail_open`.
3. **An allow-listed IP** that is also block-listed, over the rate limit, or banned is still allowed, and none of its requests are counted. The test goes in Task 3: `allow_list_wins_over_everything`.
4. **A malformed settings line** (bad CIDR, bad regex, unknown field) is skipped and reported, while every valid line still applies. The test goes in Task 2: `malformed_lines_are_skipped_and_reported`.
5. **A counter entry evicted between `add` and `increment`** (`increment` returns `None`): the hit is retried once and then counted. The test goes in Task 4: `evicted_counter_is_recreated`.

---

### Task 1: The request-stage contract

**Files:**
- Create: `crates/speedyspec-native-stage/{Cargo.toml,src/lib.rs}`
- Test: `crates/speedyspec-native-stage/tests/chain.rs`

**Interfaces:**
- Produces:
  - `pub struct StageRequest<'a> { pub params: &'a [(String, String)] }` with `fn param(&self, name: &str) -> Option<&str>`.
  - `pub struct NativeResponse { pub status: u16, pub headers: Vec<(String, String)>, pub body: Vec<u8> }`.
  - `pub enum StageOutcome { Continue, Respond(NativeResponse) }`.
  - `pub trait NativeStage { fn name(&self) -> &str; fn answer(&self, request: &StageRequest<'_>) -> StageOutcome; }`.
  - `pub struct StageChain` with `new()`, `push(Rc<dyn NativeStage>)` and `answer(&StageRequest) -> StageOutcome`. The first `Respond` wins; otherwise it returns `Continue`.
- The contract gives a stage the FastCGI params only, never the body: reading the body would take it away from PHP. Say so in the crate docs.

- [ ] **Step 1: Write the failing tests.**
  - `empty_chain_continues`
  - `first_respond_wins_and_later_stages_are_not_called`, using a stage that counts its calls with `Cell<u32>`
  - `param_finds_by_exact_name`, which checks that `param("REMOTE_ADDR")` returns `Some("192.0.2.1")` and `param("remote_addr")` returns `None`
- [ ] **Step 2:** Run `cargo test -p speedyspec-native-stage`. Expect a FAIL, because the crate is missing.
- [ ] **Step 3:** Implement the types above in `src/lib.rs`.
- [ ] **Step 4:** Run `cargo test -p speedyspec-native-stage`. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(native-stage): a contract for answering a request in rust before php`.

### Task 2: Firewall domain

**Files:**
- Create in `crates/speedyspec-firewall/`: `Cargo.toml` and `src/{lib.rs, facts.rs, ip_set.rs, pattern.rs, settings.rs, verdict.rs, ports.rs}`
- Test: `crates/speedyspec-firewall/tests/{facts.rs, ip_set.rs, pattern.rs, settings.rs}`

**Interfaces:**
- Produces:
  - `RequestFacts { client: IpAddr, method: String, path: String, query: String, user_agent: String, has_auth: bool }`.
    - `RequestFacts::from_params(params: &[(String, String)]) -> Result<RequestFacts, FirewallError>` reads `REMOTE_ADDR`, `REQUEST_METHOD`, `REQUEST_URI` (split at the first `?`), `HTTP_USER_AGENT` and `HTTP_COOKIE`.
    - `has_auth` is true when `HTTP_AUTHORIZATION` is present, or when the cookie header contains a cookie whose name starts with `wordpress_logged_in_`.
    - A missing or unparsable `REMOTE_ADDR` is an `Err`.
    - The client is normalised with `to_canonical()`, so an IPv4-mapped address becomes IPv4.
  - `IpSet::parse(text: &str) -> (IpSet, Vec<SettingsIssue>)` takes one IP or CIDR per line, trims each line, and ignores blank lines and lines starting with `#`. It provides `contains(&self, ip: IpAddr) -> bool`.
  - `Field { Path, Query, UserAgent }`.
  - `PatternRule { field: Field, status: u16, regex: Regex }`.
    - `PatternRule::parse_lines(text) -> (Vec<PatternRule>, Vec<SettingsIssue>)` takes lines of the form `<field> <status> <regex>`, where field is `path`, `query` or `ua` and status is `403` or `429`.
    - It has `matches(&self, facts: &RequestFacts) -> bool`.
  - `SettingsIssue { setting: &'static str, line: usize, message: String }`, with `line` counted from 1.
  - `RawSettings`: one `String`, `bool` or `i64` per option name in Task 5, using the same field names minus the prefix. Derive `Clone`, `PartialEq` and `Default`. The defaults are listed in Task 5.
  - `FirewallSettings::from_raw(raw: &RawSettings) -> (FirewallSettings, Vec<SettingsIssue>)` has these fields:
    - `enabled`, `allow: IpSet`, `block: IpSet`, `patterns: Vec<PatternRule>`
    - `rate: Option<RateLimit>`, which is `None` when `rate_requests <= 0`. `RateLimit { requests: u64, window: Duration, ban_for: Option<Duration> }`, where `ban_for` is `None` when the ban length is `<= 0`.
    - `login: Option<LoginPolicy>`, which is `None` when `login_max_failures <= 0`. `LoginPolicy { max_failures: u64, window: Duration, lockout: Duration, signin_paths: Vec<String> }`.
    - `block_xmlrpc: bool`, `block_user_enumeration: bool`
  - `Verdict { Allow, Continue, Block { status: u16, retry_after: Option<Duration>, reason: String } }`.
  - Ports in `ports.rs`:
    - `trait CounterStore { fn hit(&self, key: &str, window: Duration) -> Result<u64, FirewallError>; }` counts one hit in the fixed window containing *now* and returns the count so far.
    - `trait BanStore { fn ban(&self, key: &str, for_: Duration, reason: &str) -> Result<(), FirewallError>; fn banned(&self, key: &str) -> Result<Option<String>, FirewallError>; fn lift(&self, key: &str) -> Result<(), FirewallError>; }`
    - `trait SettingsSource { fn load(&self) -> Result<RawSettings, FirewallError>; }`
    - `trait Clock { fn now(&self) -> SystemTime; }`
    - `trait FirewallLog { fn warn(&self, message: &str); }`
  - `FirewallError(String)` implements `Display` and `Error`.

- [ ] **Step 1: Write the failing tests.**
  - `facts_from_params_splits_uri_and_detects_auth_cookie`. Input: `REQUEST_URI=/a/b?x=1`, `HTTP_COOKIE=foo=1; wordpress_logged_in_abc=v`. It asserts `path == "/a/b"`, `query == "x=1"` and `has_auth`.
  - `facts_without_remote_addr_is_an_error`
  - `ip_set_matches_cidr_and_single_ip`. The set `10.0.0.0/8\n2001:db8::1` contains `10.1.2.3` and `2001:db8::1`, but not `11.0.0.1`.
  - `ip_set_matches_ipv4_mapped_ipv6` (Review Focus 1). `RequestFacts` built from `REMOTE_ADDR=::ffff:10.0.0.5` has `client == 10.0.0.5`.
  - `pattern_rule_matches_field`. The rule `path 403 ^/wp-config` matches path `/wp-config.php`. The rule `ua 403 (?i)sqlmap` matches user agent `SQLMap/1.0`.
  - `malformed_lines_are_skipped_and_reported` (Review Focus 4). Input for `patterns`: `path 403 ^/ok\nbody 403 x\npath 403 (`. The result has one rule and two issues, at lines 2 and 3. The allow input `999.1.1.1\n10.0.0.1` gives one entry and one issue at line 1.
  - `rate_disabled_when_requests_not_positive`
  - `login_disabled_when_max_failures_not_positive`
- [ ] **Step 2:** Run `cargo test -p speedyspec-firewall`. Expect a FAIL.
- [ ] **Step 3:** Implement the items above.
- [ ] **Step 4:** Run `cargo test -p speedyspec-firewall`. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(firewall): request facts, ip sets, pattern rules, settings and the firewall's ports`.

### Task 3: Rules, the firewall chain and the login guard

**Files:**
- Create in `crates/speedyspec-firewall-app/`: `Cargo.toml` and `src/{lib.rs, rules.rs, firewall.rs, login.rs, cache.rs}`
- Test: `crates/speedyspec-firewall-app/tests/{firewall.rs, login.rs, doubles.rs}`. `doubles.rs` holds `MemoryCounters`, `MemoryBans`, `FixedClock`, `FailingStores` and `RecordingLog`.

**Interfaces:**
- Consumes: everything from Task 2.
- Produces:
  - `trait RequestRule { fn id(&self) -> &'static str; fn check(&self, facts: &RequestFacts) -> Result<Verdict, FirewallError>; }`
  - The rules, in this order:
    1. `AllowListRule(IpSet)`: `Allow` on a match.
    2. `BanRule(Rc<dyn BanStore>)`: key `ip:<client>`. A ban gives a `Block` with status 403.
    3. `BlockListRule(IpSet)`: a match gives 403.
    4. `PatternRules(Vec<PatternRule>)`: the first match gives that rule's status. A 429 here has `retry_after = None`.
    5. `XmlRpcRule`: path `/xmlrpc.php` gives 403.
    6. `UserEnumerationRule`, active when `!has_auth`. It gives 403 when either:
       - the query holds an `author` parameter whose value is all digits, or
       - the path is `/wp-json/wp/v2/users` or starts with `/wp-json/wp/v2/users/`.
    7. `LoginLockoutRule { policy, bans }`: for `POST` to a path in `signin_paths` when `bans.banned("login:<client>")` is set. It gives 429 with `retry_after = lockout`.
    8. `RateLimitRule { limit, counters, bans }`:
       - Calls `counters.hit("rate:<client>", window)`.
       - When the count is greater than `requests`: if `ban_for` is set, it calls `bans.ban("ip:<client>", ban_for, "rate limit")`.
       - It gives 429 with `retry_after = ban_for.unwrap_or(window)`.
  - `Firewall::new(rules: Vec<Rc<dyn RequestRule>>, log: Rc<dyn FirewallLog>)` with `check(&self, facts: &RequestFacts) -> Verdict`:
    - It runs the rules in order. `Allow` returns `Allow` at once. The first `Block` wins.
    - A rule's `Err` is logged as `"<rule id>: <error>"`, and that rule counts as `Continue`.
  - `Firewall::from_settings(settings: &FirewallSettings, counters: Rc<dyn CounterStore>, bans: Rc<dyn BanStore>, log: Rc<dyn FirewallLog>) -> Firewall`:
    - It builds only the rules the settings turn on.
    - When `!settings.enabled`, it has no rules and every request gets `Continue`.
  - `FirewallCache::new(source: Rc<dyn SettingsSource>, counters, bans, log)` has three methods:
    - `current(&self) -> Option<Rc<Firewall>>`
    - `settings(&self) -> Option<Rc<FirewallSettings>>`
    - `login_guard(&self) -> Option<LoginGuard>`, which is `None` when the firewall is disabled or login protection is off. Task 6's hook test turns `speedyspec_firewall_enabled` on first.

    Each call loads `RawSettings`. It rebuilds, and logs each `SettingsIssue`, only when the raw settings differ from the last ones. A load error is logged and gives `None`.
  - `LoginGuard::new(policy: LoginPolicy, counters: Rc<dyn CounterStore>, bans: Rc<dyn BanStore>)` with `record_failure(&self, client: IpAddr) -> Result<(), FirewallError>`:
    - It calls `counters.hit("login:<client>", window)`.
    - When the count reaches `max_failures`, it calls `bans.ban("login:<client>", lockout, "login failures")`.

- [ ] **Step 1: Write the failing tests.**
  - `blocks_listed_ip_with_403_and_no_store_reason`
  - `allow_list_wins_over_everything` (Review Focus 3). The client is in both allow and block, is banned, and `rate.requests = 1`. Three checks give `Allow` each time, and `MemoryCounters` holds no key.
  - `rate_limit_blocks_after_limit_and_bans`. With requests 2, window 60 s and ban 300 s, checks 1 and 2 continue. Check 3 gives 429 with `retry_after = 300 s`, and `banned("ip:<client>")` is `Some`.
  - `rate_limit_without_ban_retries_after_window`
  - `pattern_rule_blocks_with_its_status`
  - `xmlrpc_blocked_only_when_enabled`
  - `user_enumeration_blocked_for_anonymous_only`. `?author=2` without auth gives 403. With a `wordpress_logged_in_x` cookie it continues. `?author=bob` continues.
  - `login_guard_locks_after_max_failures`. With max 3 and lockout 600 s, two failures leave the client not banned and three leave it banned. Then a `POST` to `/wp-login.php` gives 429 with retry 600 s, and a `GET` to the same path continues.
  - `store_errors_fail_open` (Review Focus 2). `FailingStores` errors on every call. Rate and ban rules are on. The result is `Continue`, and `RecordingLog` holds the two rule errors.
  - `disabled_settings_continue_everything`
  - `cache_rebuilds_only_when_raw_settings_change`. A `SettingsSource` double counts the builds through the issue log. Two calls with unchanged settings log one issue. Changed settings log again.
- [ ] **Step 2:** Run `cargo test -p speedyspec-firewall-app`. Expect a FAIL.
- [ ] **Step 3:** Implement the items above.
- [ ] **Step 4:** Run `cargo test -p speedyspec-firewall-app`. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(firewall): the rule chain, its settings cache and the login guard`.

### Task 4: Store adapters over the Caching Store

**Files:**
- Create in `crates/speedyspec-firewall-infra-cache/`: `Cargo.toml` and `src/{lib.rs, counters.rs, bans.rs}`
- Test: `crates/speedyspec-firewall-infra-cache/tests/backends.rs`

**Interfaces:**
- Consumes:
  - `speedyspec_cache::CachingStore`:
    - `add(group, key, value: &[u8], expiry: Option<Duration>) -> Result<bool, CacheError>`
    - `increment(group, key, by) -> Result<Option<i64>, CacheError>`
    - `set`, `get`, `delete`
  - `speedyspec_firewall::{CounterStore, BanStore, Clock, FirewallError}`.
- Produces:
  - `CacheCounterStore::new(cache: Rc<dyn CachingStore>, clock: Rc<dyn Clock>)` implements `CounterStore`:
    - group `speedyspec-firewall`
    - key `<key>:<window index>`, where window index = Unix seconds of `clock.now()` / `window.as_secs().max(1)`
    - `add(key, b"0", Some(window * 2))`, then `increment(key, 1)`
    - on `None`, it repeats `add` and `increment` once; a second `None` is an `Err`
  - `CacheBanStore::new(cache: Rc<dyn CachingStore>)` implements `BanStore`:
    - group `speedyspec-firewall`
    - key `ban:<key>`
    - the value is the reason as UTF-8, with expiry `for_`
  - `SystemClock` implements `Clock`.
  - Write in the crate docs that `KvCache`'s non-atomic increment can undercount under concurrency, which is acceptable for rate limiting.

- [ ] **Step 1: Write the failing tests.** Write each check once, as a function taking `Rc<dyn CachingStore>`. Run it against three backends:
  - `MemoryCache::new()`
  - `KvCache::new(Rc::new(MemoryNoSqlStore::new()), "fw")`
  - `FileCache::new(&MemoryDirectStore::new(), "fw")`

  The database backend (`SqlCache`) is covered by `speedyspec-cache-conformance`, because the adapters use only the `CachingStore` contract.
  - `counts_within_window_and_resets_in_next`. Three hits give 1, 2 and 3. Advancing a fixed clock by one window gives 1.
  - `ban_reason_round_trips`. After `ban("ip:x", 60 s, "rate limit")`, `banned("ip:x")` is `Some("rate limit")`. A wrapper store records that `set` received an expiry of 60 s.
  - `lift_removes_ban`
  - `evicted_counter_is_recreated` (Review Focus 5). A wrapper store returns `None` from the first `increment`. The hit returns 1, and the wrapper saw two `add` calls.
  - `#[ignore = "needs Valkey: set SPEEDYSPEC_TEST_VALKEY_URL"] valkey_counts`, which uses the Valkey store the way the workspace's existing Valkey tests do.
- [ ] **Step 2:** Run `cargo test -p speedyspec-firewall-infra-cache`. Expect a FAIL.
- [ ] **Step 3:** Implement the items above.
- [ ] **Step 4:** Run `cargo test -p speedyspec-firewall-infra-cache`. Expect a PASS. Also run `SPEEDYSPEC_TEST_VALKEY_URL=redis://127.0.0.1:36379 cargo test -p speedyspec-firewall-infra-cache -- --ignored` with `valkey/valkey:8` running on port 36379. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(firewall): counters and bans on the caching store`.

### Task 5: Settings from the options API

**Files:**
- Create in `crates/speedyspec-firewall-infra-options/`: `Cargo.toml` and `src/lib.rs`
- Test: `crates/speedyspec-firewall-infra-options/tests/options.rs`

**Interfaces:**
- Consumes: `speedyspec_wp_options::Options::get::<T>(name) -> Result<Option<T>, OptionsError>`, with `T` being `String`, `i64` or `bool`. Never glob-import this crate, because its entity is named `Option`.
- Produces: `OptionsSettingsSource::new(options: Options)` implements `SettingsSource`. A missing option gets its default. An `OptionsError` becomes a `FirewallError`.

The option names and defaults, verbatim:

| Option | Type | Default |
|---|---|---|
| `speedyspec_firewall_enabled` | bool | `false` |
| `speedyspec_firewall_allow` | string, IP or CIDR per line | `""` |
| `speedyspec_firewall_block` | string, IP or CIDR per line | `""` |
| `speedyspec_firewall_patterns` | string, `<field> <status> <regex>` per line | `""` |
| `speedyspec_firewall_rate_requests` | int | `0` (off) |
| `speedyspec_firewall_rate_window` | int, seconds | `60` |
| `speedyspec_firewall_rate_ban` | int, seconds | `0` (no ban) |
| `speedyspec_firewall_login_max_failures` | int | `5` |
| `speedyspec_firewall_login_window` | int, seconds | `900` |
| `speedyspec_firewall_login_lockout` | int, seconds | `900` |
| `speedyspec_firewall_signin_paths` | string, path per line | `"/wp-login.php"` |
| `speedyspec_firewall_block_xmlrpc` | bool | `false` |
| `speedyspec_firewall_block_user_enumeration` | bool | `false` |

- [ ] **Step 1: Write the failing tests.** Build the options with `OptionsApi::new(OptionsDeps::rust(hooks, cache, Rc::new(MemoryOptionRepository::new())))`, using the in-memory hook store and object cache the way `speedyspec-wp-options-memory`'s tests build them.
  - `missing_options_give_defaults`, checked against the table above
  - `stored_options_are_read`. Set the allow list with `update_option("speedyspec_firewall_allow", "10.0.0.0/8")` and the rate with `update_option("speedyspec_firewall_rate_requests", 30)`.
- [ ] **Step 2:** Run `cargo test -p speedyspec-firewall-infra-options`. Expect a FAIL.
- [ ] **Step 3:** Implement the items above.
- [ ] **Step 4:** Run `cargo test -p speedyspec-firewall-infra-options`. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(firewall): read the firewall's settings through the options api`.

### Task 6: The stage, the hooks and the composition root

**Files:**
- Create in `crates/speedyspec-firewall-presentation/`: `Cargo.toml` and `src/{lib.rs, stage.rs, hooks.rs, compose.rs}`
- Test: `crates/speedyspec-firewall-presentation/tests/{stage.rs, hooks.rs}`

**Interfaces:**
- Consumes:
  - Task 1's `NativeStage`, `StageRequest` and `NativeResponse`
  - Tasks 3 to 5
  - `speedyspec_wp_hooks::{HookApi, Callback, HookName, HookValue}`:
    - `HookApi::add_filter(&self, hook: &HookName, callback: Callback, priority: &HookValue, accepted_args: i64) -> Result<bool, HookError>`, which actions also use, as in WordPress
    - `Callback::rust(id, impl Fn(&[HookValue]) -> Result<HookValue, HookError>)`
- Produces:
  - `CurrentClient`, a newtype over `Rc<Cell<Option<IpAddr>>>` with `set`, `get` and `clear`.
  - `FirewallStage::new(cache: Rc<FirewallCache>, current: CurrentClient, log: Rc<dyn FirewallLog>)` implements `NativeStage` with name `"firewall"`:
    - It clears `current`, builds `RequestFacts` from the params and sets `current` to the client.
    - If `cache.current()` is `None` or the facts are an `Err`, it returns `Continue`. A facts error is logged.
    - Otherwise it maps the verdict: `Block` becomes a `Respond` with the exact 403 or 429 response from Global Constraints, and anything else becomes `Continue`.
  - `register_hooks(hooks: &HookApi, cache: Rc<FirewallCache>, current: CurrentClient, log: Rc<dyn FirewallLog>) -> Result<(), HookError>` registers two callbacks:
    - On `wp_login_failed`, priority 10, 1 accepted arg: when `cache.login_guard()` and the current client are both present, it calls `record_failure(client)` and returns `HookValue::Null`. Errors are logged and swallowed.
    - On `xmlrpc_enabled`, priority 99, 1 arg: it returns `HookValue::Bool(false)` when `block_xmlrpc` is on, and otherwise returns the incoming value unchanged.
  - `FirewallComposition::build(options: Options, cache: Rc<dyn CachingStore>, hooks: &HookApi, log: Rc<dyn FirewallLog>) -> Result<Rc<FirewallStage>, HookError>` wires `SystemClock`, the cache adapters, `OptionsSettingsSource`, `FirewallCache`, `CurrentClient` and the hooks. It's built once per worker, and it's the one thing the runtime calls.
  - `StderrLog` implements `FirewallLog`. It writes `speedyspec-firewall: <message>` to stderr.

- [ ] **Step 1: Write the failing tests.**
  - `blocked_request_gets_exact_403`: asserts the status, the headers in order and the body bytes.
  - `rate_limited_request_gets_429_with_retry_after`. With a ban of 90 s, it asserts `Retry-After: 90`.
  - `missing_remote_addr_continues_and_logs`
  - `allowed_request_continues`
  - `login_failed_hook_locks_out_after_max`. Run the stage on a request from `192.0.2.9`, then `do_action("wp_login_failed", [Str("admin"), Null])` five times. The next `POST /wp-login.php` from `192.0.2.9` gets 429.
  - `xmlrpc_enabled_filter_follows_setting`
- [ ] **Step 2:** Run `cargo test -p speedyspec-firewall-presentation`. Expect a FAIL.
- [ ] **Step 3:** Implement the items above.
- [ ] **Step 4:** Run `cargo test -p speedyspec-firewall-presentation`. Expect a PASS.
- [ ] **Step 5:** Commit: `feat(firewall): the firewall as a native stage, its hooks and its composition root`.

### Task 7: The wiring request, and checking the whole branch

**Files:**
- Create: `docs/firewall-stage-wiring-request.md` in the `speedyspec-server` worktree
- Create: `crates/speedyspec-firewall/README.md`, covering the settings table, fail-open behaviour and patterns

**The wiring request asks the runtime's owner to:**
1. Build a `StageChain` once per worker in `crates/speedyspec-php-runtime/src/worker.rs`, holding the stage from `FirewallComposition::build(...)`. This needs the worker's `Options`, `HookApi` and persistent `CachingStore`, which are already requested by the database session's wiring notes.
2. Call `chain.answer(&StageRequest { params })` after `next_request()` and before `php.serve`. On `Respond`, write the response through the FastCGI responder and skip PHP.
3. Run the chain for every PHP app: blog, front-end API and admin API.
4. Add the admin API's sign-in path to `speedyspec_firewall_signin_paths` once that endpoint exists. Make the auth port fire `wp_login_failed` with the username.

It also lists what's deferred, each with its reason:
- body patterns, because a stage must not consume the body;
- PHP-registered rules, and the spec's hook for adding request rules, because they wait for the hook registry built at worker start;
- `authenticate` enforcement, because the sign-in endpoint and auth port don't exist yet;
- any defects found in other crates.

- [ ] **Step 1:** Write both documents.
- [ ] **Step 2:** Run `cargo test --workspace`. Expect a PASS. Run `cargo clippy --workspace --all-targets -- -D warnings`. Expect no warnings in the new crates.
- [ ] **Step 3:** Run `cargo tree -p speedyspec-firewall` and check that it lists only `ipnet` and `regex` and their dependencies, which confirms the dependency direction.
- [ ] **Step 4:** Commit: `docs(firewall): the runtime wiring request and the firewall's readme`.
