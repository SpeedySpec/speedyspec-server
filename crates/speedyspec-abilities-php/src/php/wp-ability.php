declare(strict_types=1);

/**
 * WordPress's ability, as a shim over the Rust Abilities server.
 *
 * `execute()` runs in Rust, in WordPress's hook order; this ability's own steps (and a subclass's overrides of them)
 * do the work through {@link WP_Ability::speedyspec_step()}. Everything else is WordPress's, from
 * SpeedySpec_TrunkAbility, a renamed copy of WordPress's class. The shim adds no properties, so abilities compare as
 * WordPress's do.
 */
class WP_Ability extends SpeedySpec_TrunkAbility
{
    /**
     * Executes the ability through the Rust Abilities server.
     *
     * @param mixed $input
     *   The input.
     * @return mixed
     *   The result, or a WP_Error.
     */
    public function execute($input = null)
    {
        return \SpeedySpec\Abilities\Native\execute($this, $input);
    }

    /**
     * Runs one of this ability's steps for the Rust Abilities server, so a subclass's override of it runs.
     *
     * @param string $step
     *   `normalize_input`, `validate_input`, `check_permissions`, `do_execute` or `validate_output`.
     * @param mixed ...$args
     *   The step's arguments.
     * @return mixed
     *   What the step returns.
     */
    public function speedyspec_step(string $step, mixed ...$args): mixed
    {
        switch ($step) {
            case 'normalize_input':
                return $this->normalize_input(...$args);
            case 'validate_input':
                return $this->validate_input(...$args);
            case 'check_permissions':
                return $this->check_permissions(...$args);
            case 'do_execute':
                return $this->do_execute(...$args);
            case 'validate_output':
                return $this->validate_output(...$args);
        }
        throw new InvalidArgumentException(sprintf('"%s" is not an ability step.', $step));
    }
}
