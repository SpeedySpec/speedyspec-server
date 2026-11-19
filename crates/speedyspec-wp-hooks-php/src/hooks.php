declare(strict_types=1);

namespace SpeedySpec\WpHooks {
    /**
     * Marks the proxies that stand in for WordPress's hook globals.
     */
    interface HookGlobal extends \ArrayAccess, \IteratorAggregate, \Countable
    {
    }

    /**
     * Stands in for `$wp_filter`: hook names to `WP_Hook` objects, kept in the Rust hook store.
     */
    final class FilterGlobal implements HookGlobal
    {
        public function offsetExists(mixed $offset): bool
        {
            return Native\global_has('filter', $offset);
        }

        public function offsetGet(mixed $offset): mixed
        {
            return Native\global_get('filter', $offset);
        }

        public function offsetSet(mixed $offset, mixed $value): void
        {
            Native\global_set('filter', $offset, $value);
        }

        public function offsetUnset(mixed $offset): void
        {
            Native\global_unset('filter', $offset);
        }

        public function getIterator(): \Iterator
        {
            return new \ArrayIterator(Native\global_all('filter'));
        }

        public function count(): int
        {
            return Native\global_count('filter');
        }
    }

    /**
     * Stands in for `$wp_actions` or `$wp_filters`: hook names to run counts, kept in the Rust hook store.
     */
    final class CountGlobal implements HookGlobal
    {
        public function __construct(private string $kind)
        {
        }

        public function offsetExists(mixed $offset): bool
        {
            return Native\global_has($this->kind, $offset);
        }

        public function offsetGet(mixed $offset): mixed
        {
            return Native\global_get($this->kind, $offset);
        }

        public function offsetSet(mixed $offset, mixed $value): void
        {
            Native\global_set($this->kind, $offset, $value);
        }

        public function offsetUnset(mixed $offset): void
        {
            Native\global_unset($this->kind, $offset);
        }

        public function getIterator(): \Iterator
        {
            return new \ArrayIterator(Native\global_all($this->kind));
        }

        public function count(): int
        {
            return Native\global_count($this->kind);
        }
    }

    /**
     * Uses `$priority` as an array key, as WordPress's `WP_Hook` does, so PHP raises the same notices (a float with
     * a fraction is "Implicit conversion from float ... to int loses precision").
     *
     * @internal
     */
    function use_as_key(mixed $priority): void
    {
        if (is_float($priority)) {
            $keys = array();
            $keys[$priority] = true;
        }
    }

    /**
     * Writes back every `WP_Hook::$callbacks` view PHP may have changed, before a hook function reads the store.
     *
     * @internal
     */
    function write_back_views(): void
    {
        if (\WP_Hook::$speedyspec_views) {
            \WP_Hook::speedyspec_write_back_all();
        }
    }

    /**
     * Installs the hook globals, importing any hooks set before the request's PHP code ran.
     *
     * @internal
     */
    function install_globals(): void
    {
        $preinitialized = isset($GLOBALS['wp_filter']) && is_array($GLOBALS['wp_filter'])
            ? \WP_Hook::build_preinitialized_hooks($GLOBALS['wp_filter'])
            : [];
        $GLOBALS['wp_filter'] = new FilterGlobal();
        foreach ($preinitialized as $name => $hook) {
            $GLOBALS['wp_filter'][$name] = $hook;
        }
        $GLOBALS['wp_actions'] = new CountGlobal('actions');
        $GLOBALS['wp_filters'] = new CountGlobal('filters');
        // A real array, kept equal to the Rust store's stack wherever PHP can see it.
        $GLOBALS['wp_current_filter'] = array();
    }
}

namespace {
    use SpeedySpec\WpHooks\Native;

    /**
     * Holds one hook's callbacks; its state lives in the Rust hook store, so this object is a handle.
     */
    #[\AllowDynamicProperties]
    final class WP_Hook implements Iterator, ArrayAccess, Countable
    {
        /**
         * The callbacks by priority; reads and writes go to the Rust hook.
         *
         * @var array<int|string, array<int|string, array>>
         */
        public $callbacks = array();

        /**
         * The priorities in order, read from the Rust hook.
         *
         * @var array<int|string>
         */
        protected array $priorities {
            get => Native\hook_priorities($this->speedyspec_hook);
        }

        private int $speedyspec_hook;

        private int $speedyspec_position = 0;

        /**
         * The `callbacks` array last handed out (by reference), written back before this hook is used again.
         */
        private ?array $speedyspec_view = null;

        /**
         * Hooks with a `callbacks` view handed out.
         *
         * @internal
         * @var array<int, WP_Hook>
         */
        public static array $speedyspec_views = array();

        public function __construct()
        {
            unset($this->callbacks);
            $this->speedyspec_hook = Native\hook_new($this);
        }

        /**
         * Wraps an existing Rust hook.
         *
         * @internal
         */
        public static function speedyspec_wrap(int $id): WP_Hook
        {
            $hook = (new ReflectionClass(self::class))->newInstanceWithoutConstructor();
            $bind = function (int $id): void {
                unset($this->callbacks);
                $this->speedyspec_hook = $id;
            };
            Closure::bind($bind, $hook, self::class)($id);
            return $hook;
        }

        public function __clone()
        {
            // The original's pending view is written back first, so the copy has its changes.
            self::speedyspec_write_back_all();
            $this->speedyspec_view = null;
            $this->speedyspec_hook = Native\hook_clone($this, $this->speedyspec_hook);
        }

        public function &__get(string $name): mixed
        {
            if ($name === 'callbacks') {
                $this->speedyspec_write_back();
                $this->speedyspec_view = Native\hook_callbacks($this->speedyspec_hook);
                self::$speedyspec_views[spl_object_id($this)] = $this;
                return $this->speedyspec_view;
            }
            trigger_error('Undefined property: WP_Hook::$' . $name, E_USER_WARNING);
            $missing = null;
            return $missing;
        }

        /**
         * Writes this hook's `callbacks` view back to the Rust hook, when PHP changed it.
         *
         * @internal
         */
        public function speedyspec_write_back(): void
        {
            if ($this->speedyspec_view === null) {
                return;
            }
            $view = $this->speedyspec_view;
            $this->speedyspec_view = null;
            unset(self::$speedyspec_views[spl_object_id($this)]);
            if ($view !== Native\hook_callbacks($this->speedyspec_hook)) {
                Native\hook_replace_callbacks($this->speedyspec_hook, $view);
            }
        }

        /**
         * @internal
         */
        public static function speedyspec_write_back_all(): void
        {
            foreach (self::$speedyspec_views as $hook) {
                $hook->speedyspec_write_back();
            }
        }

        public function __set(string $name, mixed $value): void
        {
            if ($name === 'callbacks') {
                Native\hook_replace_callbacks($this->speedyspec_hook, $value);
                return;
            }
            $this->$name = $value;
        }

        public function __isset(string $name): bool
        {
            return $name === 'callbacks';
        }

        public function __unset(string $name): void
        {
            if ($name === 'callbacks') {
                Native\hook_replace_callbacks($this->speedyspec_hook, array());
            }
        }

        public function add_filter($hook_name, $callback, $priority, $accepted_args)
        {
            $this->speedyspec_write_back();
            \SpeedySpec\WpHooks\use_as_key($priority);
            Native\hook_add_filter($this->speedyspec_hook, $hook_name, $callback, $priority, (int) $accepted_args);
        }

        public function remove_filter($hook_name, $callback, $priority)
        {
            $this->speedyspec_write_back();
            \SpeedySpec\WpHooks\use_as_key($priority);
            return Native\hook_remove_filter($this->speedyspec_hook, $hook_name, $callback, $priority);
        }

        public function has_filter($hook_name = '', $callback = false, $priority = false)
        {
            $this->speedyspec_write_back();
            return Native\hook_has_filter($this->speedyspec_hook, $hook_name, $callback, $priority);
        }

        public function has_filters()
        {
            $this->speedyspec_write_back();
            return Native\hook_has_filters($this->speedyspec_hook);
        }

        public function remove_all_filters($priority = false)
        {
            $this->speedyspec_write_back();
            Native\hook_remove_all_filters($this->speedyspec_hook, $priority);
        }

        public function apply_filters($value, $args)
        {
            $this->speedyspec_write_back();
            return Native\hook_apply_filters($this->speedyspec_hook, $value, $args);
        }

        public function do_action($args)
        {
            $this->speedyspec_write_back();
            Native\hook_do_action($this->speedyspec_hook, $args);
        }

        public function do_all_hook(&$args)
        {
            $this->speedyspec_write_back();
            Native\hook_do_all_hook($this->speedyspec_hook, $args);
        }

        public function current_priority()
        {
            $this->speedyspec_write_back();
            return Native\hook_current_priority($this->speedyspec_hook);
        }

        public static function build_preinitialized_hooks($filters)
        {
            /** @var WP_Hook[] $normalized */
            $normalized = array();

            foreach ($filters as $hook_name => $callback_groups) {
                if ($callback_groups instanceof WP_Hook) {
                    $normalized[$hook_name] = $callback_groups;
                    continue;
                }

                $hook = new WP_Hook();

                // Loop through callback groups.
                foreach ($callback_groups as $priority => $callbacks) {
                    // Loop through callbacks.
                    foreach ($callbacks as $cb) {
                        $hook->add_filter($hook_name, $cb['function'], $priority, $cb['accepted_args']);
                    }
                }

                $normalized[$hook_name] = $hook;
            }

            return $normalized;
        }

        #[ReturnTypeWillChange]
        public function offsetExists($offset)
        {
            $this->speedyspec_write_back();
            return Native\hook_offset_exists($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function offsetGet($offset)
        {
            $this->speedyspec_write_back();
            return Native\hook_offset_get($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function offsetSet($offset, $value)
        {
            $this->speedyspec_write_back();
            Native\hook_offset_set($this->speedyspec_hook, $offset, $value);
        }

        #[ReturnTypeWillChange]
        public function offsetUnset($offset)
        {
            $this->speedyspec_write_back();
            Native\hook_offset_unset($this->speedyspec_hook, $offset);
        }

        #[ReturnTypeWillChange]
        public function current()
        {
            $this->speedyspec_write_back();
            $keys = Native\hook_priorities($this->speedyspec_hook);
            if (!array_key_exists($this->speedyspec_position, $keys)) {
                return false;
            }
            return Native\hook_offset_get($this->speedyspec_hook, $keys[$this->speedyspec_position]);
        }

        #[ReturnTypeWillChange]
        public function next()
        {
            ++$this->speedyspec_position;
            return $this->current();
        }

        #[ReturnTypeWillChange]
        public function key()
        {
            $this->speedyspec_write_back();
            $keys = Native\hook_priorities($this->speedyspec_hook);
            return $keys[$this->speedyspec_position] ?? null;
        }

        #[ReturnTypeWillChange]
        public function valid()
        {
            return $this->key() !== null;
        }

        #[ReturnTypeWillChange]
        public function rewind()
        {
            $this->speedyspec_position = 0;
        }

        #[ReturnTypeWillChange]
        public function count()
        {
            $this->speedyspec_write_back();
            return count(Native\hook_priorities($this->speedyspec_hook));
        }
    }

    function add_filter($hook_name, $callback, $priority = 10, $accepted_args = 1)
    {
        \SpeedySpec\WpHooks\write_back_views();
        \SpeedySpec\WpHooks\use_as_key($priority);
        return Native\add_filter($hook_name, $callback, $priority, (int) $accepted_args);
    }

    function apply_filters($hook_name, $value, ...$args)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\apply_filters($hook_name, $value, ...$args);
    }

    function apply_filters_ref_array($hook_name, $args)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\apply_filters_ref_array($hook_name, $args);
    }

    function has_filter($hook_name, $callback = false, $priority = false)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\has_filter($hook_name, $callback, $priority);
    }

    function remove_filter($hook_name, $callback, $priority = 10)
    {
        \SpeedySpec\WpHooks\write_back_views();
        \SpeedySpec\WpHooks\use_as_key($priority);
        return Native\remove_filter($hook_name, $callback, $priority);
    }

    function remove_all_filters($hook_name, $priority = false)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\remove_all_filters($hook_name, $priority);
    }

    function current_filter()
    {
        return Native\current_filter();
    }

    function doing_filter($hook_name = null)
    {
        return Native\doing_filter($hook_name);
    }

    function did_filter($hook_name)
    {
        return Native\did_filter($hook_name);
    }

    function add_action($hook_name, $callback, $priority = 10, $accepted_args = 1)
    {
        \SpeedySpec\WpHooks\write_back_views();
        \SpeedySpec\WpHooks\use_as_key($priority);
        return Native\add_filter($hook_name, $callback, $priority, (int) $accepted_args);
    }

    function do_action($hook_name, ...$arg)
    {
        \SpeedySpec\WpHooks\write_back_views();
        Native\do_action($hook_name, ...$arg);
    }

    function do_action_ref_array($hook_name, $args)
    {
        \SpeedySpec\WpHooks\write_back_views();
        Native\do_action_ref_array($hook_name, $args);
    }

    function has_action($hook_name, $callback = false, $priority = false)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\has_filter($hook_name, $callback, $priority);
    }

    function remove_action($hook_name, $callback, $priority = 10)
    {
        \SpeedySpec\WpHooks\write_back_views();
        \SpeedySpec\WpHooks\use_as_key($priority);
        return Native\remove_filter($hook_name, $callback, $priority);
    }

    function remove_all_actions($hook_name, $priority = false)
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\remove_all_filters($hook_name, $priority);
    }

    function current_action()
    {
        return Native\current_filter();
    }

    function doing_action($hook_name = null)
    {
        return Native\doing_filter($hook_name);
    }

    function did_action($hook_name)
    {
        return Native\did_action($hook_name);
    }

    function apply_filters_deprecated($hook_name, $args, $version, $replacement = '', $message = '')
    {
        \SpeedySpec\WpHooks\write_back_views();
        return Native\apply_filters_deprecated($hook_name, $args, $version, $replacement, $message);
    }

    function do_action_deprecated($hook_name, $args, $version, $replacement = '', $message = '')
    {
        \SpeedySpec\WpHooks\write_back_views();
        Native\do_action_deprecated($hook_name, $args, $version, $replacement, $message);
    }

    function _wp_call_all_hook($args)
    {
        \SpeedySpec\WpHooks\write_back_views();
        Native\call_all_hook($args);
    }

    function _wp_filter_build_unique_id($hook_name, $callback, $priority): ?string
    {
        return Native\unique_id($callback);
    }
}
