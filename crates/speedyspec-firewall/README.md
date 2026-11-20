# speedyspec-firewall

A Rust firewall that runs before PHP. It replaces the request-filtering and login-protection parts of plugins like
Wordfence and Solid Security.

## Crates

| Crate | Role |
|---|---|
| `speedyspec-native-stage` | The contract for answering a request in Rust before PHP runs |
| `speedyspec-firewall` | Domain: request facts, IP sets, pattern rules, settings, verdicts, and the ports |
| `speedyspec-firewall-app` | The rule chain, the settings cache, and the login guard |
| `speedyspec-firewall-infra-cache` | Counters and bans on any Caching Store: memory, database, Valkey, ReDB or file |
| `speedyspec-firewall-infra-options` | Settings read through the options API |
| `speedyspec-firewall-presentation` | The native stage, the hook callbacks, and `FirewallComposition::build` |

## Rules, in order

1. **Allow list:** a match lets the request through and skips every later rule.
2. **Active ban:** 403.
3. **Block list:** 403.
4. **Patterns** on the path, query or user agent: the rule's own status, 403 or 429.
5. **XML-RPC** (`/xmlrpc.php`): 403.
6. **User enumeration**, for visitors who are not signed in (`?author=<digits>`, `/wp-json/wp/v2/users`): 403.
7. **Login lockout:** a `POST` to a sign-in path while locked out gets 429.
8. **Rate limit:** 429, with an optional ban.

## Settings

Each setting is its own option:

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

In `patterns`, the field is `path`, `query` or `ua`, and the status is `403` or `429`. Blank lines and lines starting
with `#` are ignored in every list. A malformed line is skipped and logged, and every valid line still applies.

## Failing open

If the firewall can't do its job, the request goes through. A store that fails, settings that can't be read, or a
request with no client address all let the request through and log a warning to stderr, prefixed
`speedyspec-firewall:`. An outage never locks the site's owner out.

## Hooks

- `wp_login_failed` (priority 10) counts a failed sign-in for the request's client. When the failures reach
  `login_max_failures` within `login_window`, the client is locked out for `login_lockout`.
- `xmlrpc_enabled` (priority 99) returns `false` when XML-RPC is blocked.

## Patterns used

- **Chain of Responsibility:** the rules
- **Specification:** IP sets and pattern rules
- **Strategy:** the store backends
- **Adapter:** the cache and options adapters, and the stage
- **Observer:** the hook callbacks
- **Factory:** the composition root
- **Memoization:** the settings cache

## Known limits

- On Valkey and ReDB, two workers counting at the same moment can undercount a rate limit by one, because the
  key-value cache's `increment` reads and then writes.
- These are deferred: body patterns, rules added by PHP plugins, and `authenticate` enforcement. See
  `docs/firewall-stage-wiring-request.md`.
