# speedyspec-abilities-contracts

The Abilities transport's contracts, apart from any implementation, so each part can be replaced.

## Value objects

- `AbilityBinding`: the metadata an operation is published with as an ability:
  - `label`, `description` and `category`;
  - `show_in_rest` and `public`;
  - `Annotations { readonly, destructive, idempotent }`;
  - further `meta`.

  Every field is optional; `merge()` fills the unset ones from the default mapping's. Categories follow WordPress's
  slug rule. Stored as a map under the `abilities` transport.
- `is_ability_name` and `is_category_slug`: WordPress's name and slug rules. Operation names already follow the
  ability-name rule.
- `AbilityOutcome` and `AbilityResult`: an ability's output or step result, with a `WP_Error` as an operation error.

## Ports

| Port | What it decides | Implementations |
|---|---|---|
| `AbilityMapping` (Strategy) | an operation's default ability metadata | `DefaultAbilityMapping` |
| `AbilityHooks` | one method per WordPress ability hook, `wp_ability_invoked` through `wp_after_execute_ability`, plus `_doing_it_wrong` | `NoAbilityHooks`, `PhpAbilityHooks` |
| `AbilitySteps` (Strategy) | how one ability normalizes, validates, authorizes, runs and checks its output | `OperationSteps` (Rust), `PhpAbilitySteps` (a `WP_Ability`'s own methods) |
