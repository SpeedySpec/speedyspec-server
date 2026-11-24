# speedyspec-abilities-php

The Abilities transport's PHP adapter: `WP_Ability::execute()` runs through the Rust Abilities server for every
ability, and every operation is an ability.

## What stays WordPress's own

These stay WordPress's:
- `WP_Abilities_Registry`, `WP_Ability_Categories_Registry` and `WP_Ability_Category`. They are `final`, WordPress's
  tests construct them and reset their private `$instance` through Reflection, and they only store abilities.
- `abilities-api.php`'s functions.
- The `wp-abilities/v1` REST controllers. They are ordinary `register_rest_route` routes, served in Rust through
  `speedyspec-rest-php`'s legacy routes, and they call the shimmed `execute()`.

## The `WP_Ability` shim

- **The class:** `WP_Ability` extends `SpeedySpec_TrunkAbility`, WordPress 7.1.5's class renamed. The class names it
  reports or compares (`self::class` in the callback checks, `__METHOD__`, `__CLASS__`) are pinned to `WP_Ability`,
  so it reads exactly as WordPress's does.
- **What it changes:**
  - it overrides only `execute()`, which goes to Rust;
  - it adds `speedyspec_step()`, through which Rust runs the object's own `normalize_input`, `validate_input`,
    `check_permissions`, `do_execute` and `validate_output`, so a subclass's overrides run;
  - it adds no properties, so abilities compare with `==` as WordPress's do.
- **Declaration:** it is declared at request start. WordPress's own `class-wp-ability.php` must not be loaded.
- **Legacy abilities (LegacyHandlerAdapter):** every ability that is not an operation (from `wp_register_ability`,
  a subclass, or one built directly) is a request-scoped operation `legacy-abilities/{slug}-{hash}`, restricted to
  `abilities`, whose handler is the ability's `do_execute`.
- **Values:** plain data crosses as neutral values. A value with objects in it, such as an object a filter or an
  ability returns, stays PHP's and comes back as the same value. Plain data that uses the stand-in's key is
  escaped, so a client can't forge one. A `WP_Error` from PHP comes back as the same object, every code of it kept.
  An operation's own error gets a `status` from its category, and an internal one a generic message.

## Operations as abilities

At request start, where WordPress's Hook API is native, `Adapter::hook()` adds two callbacks at `PHP_INT_MIN`:
- on `wp_abilities_api_categories_init`, the default (vendor) categories that are missing are registered;
- on `wp_abilities_api_init`, every operation exposed on `abilities` is registered through `wp_register_ability()`
  as a `SpeedySpec\Abilities\OperationAbility`, with its exported JSON Schemas and its metadata.

Executing an `OperationAbility` runs `OperationSteps`, with WordPress's current user as the caller. Its
`normalize_input()`, `validate_input()` and `check_permissions()` are the operation's steps too, so a caller of one
step alone gets the operation's answer. WordPress's REST run controller checks permission that way before it
executes. A `wp_ability_permission_result` grant is final: the operation's own check does not run again inside
execute.

Default categories register last on `wp_abilities_api_categories_init` (`PHP_INT_MAX`), so a plugin's own category of
its vendor's slug wins; operations register first on `wp_abilities_api_init`.

```php
use SpeedySpec\Abilities\Abilities;

final class Acme_Abilities {
    public static function publish(): void {
        Abilities::publish( 'acme/delete-report' )->category( 'reports' )->label( 'Delete report' )->public( true );
    }
    public static function categories(): void {
        wp_register_ability_category( 'reports', [ 'label' => 'Reports', 'description' => 'Report abilities.' ] );
    }
}
add_action( 'wp_operations_init', [ Acme_Abilities::class, 'publish' ] );
add_action( 'wp_abilities_api_categories_init', [ Acme_Abilities::class, 'categories' ] );
```

`publish()` also takes `description()`, `showInRest()`, `annotations()` and `meta()`. A category it names is the
owner's to register; WordPress's registry refuses the ability otherwise. A bad slug or an unknown annotation throws
`SpeedySpec\Operation\RegistrationException`.

## Hooks

`PhpAbilityHooks` fires WordPress's ability hooks with WordPress's arguments, the `WP_Ability` itself included, and
its coercions:
- a `wp_ability_permission_result` that is neither a boolean nor a `WP_Error` is `false`;
- `false` from the validate filters is `Invalid input.` or `Invalid output.`.

A hook that throws fails closed.

## Conformance

WordPress's six database-free Abilities API test files, copied from wordpress-develop's 7.1 branch into
`tests/php/wordpress/abilities`, run against the shim through `tests/wordpress.rs`: 201 tests, none expected to fail.
One test, `test_execute_with_different_callbacks`, is never collected, because its data provider uses `$this`, which
PHPUnit 11 cannot run. `wpRegisterCoreAbilities.php` and the three REST controller suites need a database and are not
copied.
