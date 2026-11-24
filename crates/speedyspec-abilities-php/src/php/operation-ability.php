declare(strict_types=1);

namespace SpeedySpec\Abilities;

/**
 * An operation, registered as an ability.
 *
 * Executing it runs the operation's own steps in Rust (its schema, permission check and handler), with WordPress's
 * ability hooks firing in WordPress's order.
 */
final class OperationAbility extends \WP_Ability
{
    /**
     * Normalizes input as the operation does, firing `wp_ability_normalize_input`.
     *
     * @param mixed $input
     *   The input.
     * @return mixed
     *   The input, or a WP_Error.
     */
    public function normalize_input($input = null)
    {
        return Native\operation_step($this, 'normalize_input', $input);
    }

    /**
     * Validates input against the operation's schema, firing `wp_ability_validate_input`.
     *
     * @param mixed $input
     *   The input.
     * @return true|\WP_Error
     *   True, or why the input is invalid.
     */
    public function validate_input($input = null)
    {
        return Native\operation_step($this, 'validate_input', $input);
    }

    /**
     * Checks the operation's permission for the current user, firing `wp_ability_permission_result`.
     *
     * @param mixed $input
     *   The input.
     * @return bool|\WP_Error
     *   Whether the current user may execute it.
     */
    public function check_permissions($input = null)
    {
        return Native\operation_step($this, 'check_permissions', $input);
    }

    /**
     * Stands in for the callbacks WordPress's registration requires; an operation ability never calls it.
     *
     * @return \WP_Error
     *   Always an error.
     */
    public static function unused(): \WP_Error
    {
        return new \WP_Error('ability_operation', 'An operation ability runs through its operation.');
    }
}
