# Wiring request: run the firewall before PHP

To: the owner of `crates/speedyspec-php-runtime` (the PHP runtime)
From: the native firewall work, branch `feat/native-firewall`
Spec: `wordpress-develop/docs/superpowers/specs/2026-11-19-native-plugin-features-design.md`, "Runtime request stages"

The firewall is built and tested in its own crates. It is not live until the runtime calls it. This request asks
for that call. Nothing in the runtime was changed by the firewall work.

## What to wire

1. **Build the stage chain once per worker** in `crates/speedyspec-php-runtime/src/worker.rs`, at worker start,
   next to the database and options wiring already requested by the database port's notes:

   ```rust
   let firewall = speedyspec_firewall_presentation::FirewallComposition::build(
       speedyspec_wp_options::Options::new(options_api.clone()), // the worker's OptionsApi
       persistent_cache.clone(),                                  // the worker's Rc<dyn CachingStore>
       &hook_api,                                                 // the worker's HookApi
       std::rc::Rc::new(speedyspec_firewall_presentation::StderrLog),
   )?;
   let mut chain = speedyspec_native_stage::StageChain::new();
   chain.push(firewall);
   ```

   The persistent cache is the one `PersistentCacheSelector` picks for the site. The firewall's counters and bans
   live in its `speedyspec-firewall` group. Without a persistent cache, pass a `MemoryCache`: the firewall then
   works per worker, which is weaker but safe.

2. **Ask the chain before PHP**, after `responder.next_request()` and before `php.serve(&mut request, &script)`:

   ```rust
   match chain.answer(&StageRequest { params: request.params() }) {
       StageOutcome::Respond(response) => {
           // Write response.status, response.headers and response.body through the FastCGI responder,
           // end the request, and do not start PHP.
       }
       StageOutcome::Continue => { /* serve with PHP as today */ }
   }
   ```

   The stage reads only the params, never the body, so PHP still gets the whole body on `Continue`.

3. **Run the chain for every PHP app:** the blog, the front-end API and the admin API.

4. **Sign-in:**
   - When the admin API's sign-in endpoint exists, add its path to the `speedyspec_firewall_signin_paths` option
     (one path per line).
   - Make the auth port fire `wp_login_failed` with the username, as WordPress does. The firewall's callback counts
     the failure against the client the stage saw on the same request.

## What the firewall needs from the request

- `REMOTE_ADDR`: the client address, as the server already resolves it through its trusted proxies. Without it,
  the firewall lets the request through and logs a warning.
- `REQUEST_METHOD`, `REQUEST_URI`, `HTTP_USER_AGENT`, `HTTP_COOKIE` and `HTTP_AUTHORIZATION`, when present.

## Deferred, and why

- **Body patterns:** a stage must not read the body that PHP still needs. This waits for a decision on buffering
  the body in the runtime.
- **Rules added by PHP plugins, and the spec's hook for adding request rules:** these wait for the hook registry
  built at worker start ("PHP callbacks on native paths" in the spec).
- **`authenticate` enforcement:** there is no ported auth API or sign-in endpoint to attach it to yet. Until then,
  lockouts are enforced at the request stage, for `POST`s to the sign-in paths.
- **Defects in other crates:** none were found.
