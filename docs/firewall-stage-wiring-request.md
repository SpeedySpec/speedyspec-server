# Wiring request: run the firewall before PHP

To: the owner of `crates/speedyspec-php-runtime` (the PHP runtime)
From: the native firewall work, branch `feat/native-firewall`
Spec: `wordpress-develop/docs/superpowers/specs/2026-11-19-native-plugin-features-design.md`, "Runtime request stages"

The firewall is built and tested in its own crates. It isn't live until the runtime calls it. This request asks for
that call. The firewall work changed nothing in the runtime. Line numbers below refer to
`crates/speedyspec-php-runtime/src/worker.rs` as of `6ea3887`.

## Prerequisites

The firewall needs two things that `run_worker` does not have yet. The database port's wiring notes already ask
for both:

- **The worker's options API.** An `Rc<OptionsApi>` built from the worker's `NativeWpdb`, object cache and `HookApi`
  (`OptionsArea::api()` in `speedyspec-wp-options-php`).
- **A `Rc<dyn CachingStore>` for the firewall's counters and bans.** The spec ("Shared state") makes the database
  the default store for these. Use `PersistentCacheSelector` to pick the routed engine:
  - the SQL cache (`wp_speedyspec_cache`) by default;
  - Valkey, ReDB or files when the operator routes it there.

  With `[cluster]`, use Valkey or the database. A file store is per replica. Only if no store can be built, pass a
  `MemoryCache`: the firewall then works per worker, which is weaker but safe.

## What to wire

1. **Take the `HookApi` before the hooks area is moved.** `let hooks = WpHooksArea::new(...)` (line ~201) is moved
   into `areas` (line ~209). Take `let hook_api = hooks.api();` before that move.

2. **Compose the firewall once, at worker start, before the first request.** The firewall's hook callbacks must
   land in the hook store's worker-lifetime partition. Registered during a request, they would be dropped when the
   request ends. `run_worker` returns `i32`, so handle the error there, not with `?`:

   ```rust
   let scope = if is_admin_api(&spec.app) { FirewallScope::Admin } else { FirewallScope::Public };
   let firewall = match FirewallComposition::build(
       Options::new(options_api.clone()),
       firewall_store.clone(),
       &hook_api,
       scope,
       Rc::new(StderrLog),
   ) {
       Ok(stage) => stage,
       Err(err) => {
           log("php_worker_firewall_failed", &spec, &err.to_string()); // or the worker's own error log
           return 1;
       }
   };
   let mut chain = StageChain::new();
   chain.push(firewall);
   ```

   Use `FirewallScope::Admin` for the admin API app and `FirewallScope::Public` for the blog and the front-end
   API. The admin scope leaves out the rate limit and IP bans, so the block editor's bursts can never lock the
   site's owner out. How the runtime knows which `spec.app` is the admin API is the runtime's call; a field on
   `WorkerSpec` would do.

3. **Ask the chain for every request, inside the existing loop.** Do this after `FastCgiRequest` is built
   (line ~268) and before the `match inside`, so that the report and `end_request` still run afterwards:

   ```rust
   let outcome = chain.answer(&StageRequest { params: &request.params });
   let (poisoned, peak) = match (outcome, inside) {
       (StageOutcome::Respond(response), _) => {
           let headers: Vec<String> =
               response.headers.iter().map(|(name, value)| format!("{name}: {value}")).collect();
           request.send_headers(response.status, &headers);
           request.write(&response.body);
           (false, 0)
       }
       (StageOutcome::Continue, Some(script)) => { /* php.serve as today */ }
       (StageOutcome::Continue, None) => { /* the 404 as today */ }
   };
   ```

   `WebRequest::send_headers` takes `"Name: value"` strings, hence the conversion. The stage reads only the
   params, never the body, so PHP still gets the whole body on `Continue`.

4. **Ask the chain on every request, without exception.** The firewall's `wp_login_failed` callback counts a
   failure against the client the stage saw last. A request that runs PHP without asking the chain would charge
   the previous request's client.

5. **Sign-in:**
   - Once the admin API's sign-in endpoint exists, add its path to the `speedyspec_firewall_signin_paths` option
     (one path per line).
   - Make the auth port fire `wp_login_failed` with the username, as WordPress does.

## What the firewall reads from the request

- `REMOTE_ADDR`: the client address, as the server already resolves it through its trusted proxies. Without it,
  the firewall lets the request through and logs a warning.
- `REQUEST_METHOD`, `REQUEST_URI`, `HTTP_USER_AGENT`, `HTTP_COOKIE`, `HTTP_AUTHORIZATION` and `HTTP_X_WP_NONCE`,
  when present.

## Deferred, and why

- **Body patterns:** a stage must not read the body that PHP still needs. This waits for a decision on buffering
  the body in the runtime.
- **Rules added by PHP plugins, and the spec's hook for adding request rules:** these wait for the hook registry
  built at worker start ("PHP callbacks on native paths" in the spec).
- **`authenticate` enforcement:** there is no ported auth API or sign-in endpoint to attach it to yet. Until then,
  lockouts are enforced at the request stage, for `POST`s to the sign-in paths.
- **Defects in other crates:** none were found.
