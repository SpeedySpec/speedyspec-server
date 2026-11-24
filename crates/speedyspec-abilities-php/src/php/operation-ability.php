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
