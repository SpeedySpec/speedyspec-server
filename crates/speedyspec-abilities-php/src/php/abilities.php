declare(strict_types=1);

namespace SpeedySpec\Abilities;

/**
 * Publishes operations as abilities with metadata of their own.
 */
final class Abilities
{
    /**
     * Starts the ability metadata of an operation.
     *
     * Every operation is an ability by default; this changes what it is published with:
     * `Abilities::publish( 'acme/get-report' )->category( 'reports' )->public( true )`.
     *
     * @param string $operation
     *   The operation's name.
     * @return AbilityPublisher
     *   The publisher that stores the metadata.
     */
    public static function publish(string $operation): AbilityPublisher
    {
        return new AbilityPublisher($operation);
    }
}

/**
 * Stores one operation's ability metadata, each call merged into what it stored before.
 */
final class AbilityPublisher
{
    private array $binding = [];

    public function __construct(private readonly string $operation)
    {
    }

    /**
     * Names the ability category, which its owner registers.
     *
     * @param string $slug
     *   The category's slug.
     * @return self
     *   This publisher.
     */
    public function category(string $slug): self
    {
        return $this->set('category', $slug);
    }

    /**
     * Sets the ability's label.
     *
     * @param string $label
     *   The label.
     * @return self
     *   This publisher.
     */
    public function label(string $label): self
    {
        return $this->set('label', $label);
    }

    /**
     * Sets the ability's description.
     *
     * @param string $description
     *   The description.
     * @return self
     *   This publisher.
     */
    public function description(string $description): self
    {
        return $this->set('description', $description);
    }

    /**
     * Shows or hides the ability in the `wp-abilities/v1` REST API.
     *
     * @param bool $show
     *   Whether it is shown.
     * @return self
     *   This publisher.
     */
    public function showInRest(bool $show): self
    {
        return $this->set('show_in_rest', $show);
    }

    /**
     * Marks the ability public or not.
     *
     * @param bool $public
     *   Whether it is public.
     * @return self
     *   This publisher.
     */
    public function public(bool $public): self
    {
        return $this->set('public', $public);
    }

    /**
     * Sets annotations, over the defaults the operation's traits give.
     *
     * @param array $annotations
     *   `readonly`, `destructive` and `idempotent`, each a boolean.
     * @return self
     *   This publisher.
     */
    public function annotations(array $annotations): self
    {
        return $this->set('annotations', $annotations + ($this->binding['annotations'] ?? []));
    }

    /**
     * Adds meta keys.
     *
     * @param array $meta
     *   The keys and their values.
     * @return self
     *   This publisher.
     */
    public function meta(array $meta): self
    {
        return $this->set('meta', $meta + ($this->binding['meta'] ?? []));
    }

    /**
     * Stores the binding with one field changed.
     *
     * @param string $key
     *   The field.
     * @param mixed $value
     *   Its value.
     * @return self
     *   This publisher.
     */
    private function set(string $key, mixed $value): self
    {
        $binding       = $this->binding;
        $binding[$key] = $value;
        Native\publish($this->operation, $binding);
        $this->binding = $binding;
        return $this;
    }
}

namespace SpeedySpec\Abilities\Internal;

/**
 * Between WordPress's ability hooks and the Rust Abilities server.
 *
 * Results cross as `[ 'value', $value ]`, `[ 'error', $code, $message, $data ]` or `[ 'run' ]`.
 *
 * @internal
 */
final class Adapter
{
    /**
     * Describes a WP_Error.
     *
     * @param mixed $value
     *   The value.
     * @return array|null
     *   `[ code, message, data ]`, or null when the value is not a WP_Error.
     */
    public static function describe_error(mixed $value): ?array
    {
        if (! class_exists('WP_Error', false) || ! $value instanceof \WP_Error) {
            return null;
        }
        return [(string) $value->get_error_code(), (string) $value->get_error_message(), $value->get_error_data()];
    }

    /**
     * Builds a WP_Error.
     *
     * @param string $code
     *   The code.
     * @param string $message
     *   The message.
     * @param mixed $data
     *   The data, or null for none.
     * @return mixed
     *   The WP_Error, or the parts when WP_Error is not loaded.
     */
    public static function wp_error(string $code, string $message, mixed $data): mixed
    {
        if (! class_exists('WP_Error', false)) {
            return [$code, $message, $data];
        }
        return null === $data ? new \WP_Error($code, $message) : new \WP_Error($code, $message, $data);
    }

    /**
     * Describes a filter's result.
     *
     * @param mixed $value
     *   The result.
     * @return array
     *   The error, or the value.
     */
    private static function outcome(mixed $value): array
    {
        $error = self::describe_error($value);
        return null === $error ? ['value', $value] : ['error', ...$error];
    }

    /**
     * Adds the hooks that register operations as abilities, before any other callback of the init actions.
     */
    public static function hook(): void
    {
        if (! function_exists('add_action') || ! function_exists('has_action')) {
            return;
        }
        foreach ([
            'wp_abilities_api_categories_init' => 'register_categories',
            'wp_abilities_api_init'            => 'register_operations',
        ] as $hook => $method) {
            $callback = [self::class, $method];
            if (false === has_action($hook, $callback)) {
                add_action($hook, $callback, PHP_INT_MIN);
            }
        }
    }

    /**
     * Registers the default categories of the operations exposed as abilities that are not registered yet.
     */
    public static function register_categories(): void
    {
        foreach (\SpeedySpec\Abilities\Native\operation_categories() as $slug) {
            if (! wp_has_ability_category($slug)) {
                wp_register_ability_category(
                    $slug,
                    ['label' => $slug, 'description' => sprintf('Abilities of %s.', $slug)]
                );
            }
        }
    }

    /**
     * Registers every operation exposed as an ability.
     */
    public static function register_operations(): void
    {
        $unused = [\SpeedySpec\Abilities\OperationAbility::class, 'unused'];
        foreach (\SpeedySpec\Abilities\Native\operation_abilities() as [$name, $args]) {
            $args['execute_callback']    = $unused;
            $args['permission_callback'] = $unused;
            $args['ability_class']       = \SpeedySpec\Abilities\OperationAbility::class;
            wp_register_ability($name, $args);
        }
    }

    /**
     * Runs one of an ability's steps.
     *
     * @param object $ability
     *   The WP_Ability.
     * @param string $step
     *   The step.
     * @param array $args
     *   Its arguments.
     * @return array
     *   The error, or the value.
     */
    public static function step(object $ability, string $step, array $args): array
    {
        return self::outcome($ability->speedyspec_step($step, ...$args));
    }

    /**
     * Fires `wp_ability_invoked`.
     */
    public static function invoked(string $name, mixed $input, mixed $ability): void
    {
        do_action('wp_ability_invoked', $name, $input, $ability);
    }

    /**
     * Applies `wp_pre_execute_ability` with WordPress's sentinel.
     *
     * @return array
     *   `[ 'run' ]` when the sentinel came back, otherwise the result.
     */
    public static function pre_execute(string $name, mixed $input, mixed $ability): array
    {
        $sentinel = class_exists('WP_Filter_Sentinel') ? new \WP_Filter_Sentinel() : new \stdClass();
        $pre      = apply_filters('wp_pre_execute_ability', $sentinel, $name, $input, $ability);
        return $pre === $sentinel ? ['run'] : self::outcome($pre);
    }

    /**
     * Applies `wp_ability_normalize_input`.
     */
    public static function normalize_input(string $name, mixed $input, mixed $ability): array
    {
        return self::outcome(apply_filters('wp_ability_normalize_input', $input, $name, $ability));
    }

    /**
     * Applies `wp_ability_validate_input` as `WP_Ability::validate_input()` does.
     */
    public static function validate_input(string $name, mixed $is_valid, mixed $input): array
    {
        $validity = apply_filters('wp_ability_validate_input', $is_valid, $input, $name);
        if (false === $validity) {
            return ['error', 'ability_invalid_input', __('Invalid input.'), null];
        }
        if (is_wp_error($validity) && $validity->has_errors()) {
            return self::outcome($validity);
        }
        return ['value', true];
    }

    /**
     * Applies `wp_ability_permission_result` as `WP_Ability::check_permissions()` does.
     */
    public static function permission_result(string $name, mixed $permission, mixed $input, mixed $ability): array
    {
        $result = apply_filters('wp_ability_permission_result', $permission, $name, $input, $ability);
        if (! is_bool($result) && ! is_wp_error($result)) {
            $result = false;
        }
        return self::outcome($result);
    }

    /**
     * Fires `wp_before_execute_ability`.
     */
    public static function before_execute(string $name, mixed $input, mixed $ability): void
    {
        do_action('wp_before_execute_ability', $name, $input, $ability);
    }

    /**
     * Applies `wp_ability_execute_result`.
     */
    public static function execute_result(string $name, mixed $result, mixed $input, mixed $ability): array
    {
        return self::outcome(apply_filters('wp_ability_execute_result', $result, $name, $input, $ability));
    }

    /**
     * Applies `wp_ability_validate_output` as `WP_Ability::validate_output()` does.
     */
    public static function validate_output(string $name, mixed $is_valid, mixed $output): array
    {
        $validity = apply_filters('wp_ability_validate_output', $is_valid, $output, $name);
        if (false === $validity) {
            return ['error', 'ability_invalid_output', __('Invalid output.'), null];
        }
        if (is_wp_error($validity) && $validity->has_errors()) {
            return self::outcome($validity);
        }
        return ['value', true];
    }

    /**
     * Fires `wp_after_execute_ability`.
     */
    public static function after_execute(string $name, mixed $input, mixed $result, mixed $ability): void
    {
        do_action('wp_after_execute_ability', $name, $input, $result, $ability);
    }

    /**
     * Reports a misuse as WordPress's `_doing_it_wrong()` does, the message escaped as WordPress escapes it.
     */
    public static function doing_it_wrong(string $function, string $message, string $version): void
    {
        if (function_exists('_doing_it_wrong')) {
            _doing_it_wrong($function, function_exists('esc_html') ? esc_html($message) : htmlspecialchars($message), $version);
        }
    }
}
