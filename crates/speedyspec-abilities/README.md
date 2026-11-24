# speedyspec-abilities

The Abilities transport: every operation is an ability, and `WP_Ability::execute()` runs in Rust in WordPress's hook
order.

Patterns:
- Strategy: `DefaultAbilityMapping`, and the `AbilitySteps` behind each ability.
- Template Method: `AbilitiesServer::execute`.

## Default metadata

`DefaultAbilityMapping` publishes an operation with:

- **category:** its vendor as a slug (`acme/get-report` → `acme`; `-my--plugin-/x` → `my-plugin`);
- **visibility:** `show_in_rest: true` (every operation is reachable on every transport by default, and its permission
  check still applies) and `public: false`;
- **annotations:**

  | Operation | readonly | destructive | idempotent |
  |---|---|---|---|
  | query | true | false | true |
  | command | false | its `destructive` trait, or a Delete action | its `idempotent` trait, or an Update or Delete action |

- **label and description:** its labels, else its name.

Subscriptions are not abilities. `Abilities::publish` (in `speedyspec-abilities-php`) overrides any of these. These
defaults are Claude's calls and open to override.

## The server

`AbilitiesServer::execute(name, input, steps, ability)` follows WordPress 7.1.5's `WP_Ability::execute()`:

1. `wp_ability_invoked`.
2. `wp_pre_execute_ability`, whose return short-circuits the call unless it is the sentinel, even when it is `null`,
   `false` or a `WP_Error`.
3. normalize, then validate.
4. Check permissions. Anything but `true` gives `ability_invalid_permissions`
   `Ability "{name}" does not have necessary permission.`; a `WP_Error` is reported through `_doing_it_wrong`, never
   told to the caller.
5. `wp_before_execute_ability`.
6. Execute, then validate the output.
7. `wp_after_execute_ability`.

The step filters fire inside the steps, as they fire inside WordPress's methods. A hook that fails fails the call
closed (`ability_hook_failed`).

`OperationSteps` are an operation's steps in Rust:
- its input schema, through `Dispatcher::validate`; a failure is `ability_invalid_input` with WordPress's
  `Reason:` text;
- its permission check, through `Dispatcher::authorize`, where a refusal is `false`;
- its handler, where an invalid output is `ability_invalid_output`.

No input is an empty object for an object-input operation.

`errors::wp_error` builds WordPress's ability errors so they round-trip to the same `WP_Error`.
