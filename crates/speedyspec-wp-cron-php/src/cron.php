// Evaluated without an opening tag. No strict_types: WordPress's own files are not strict, so arguments coerce as they
// do there.

namespace SpeedySpec\WpCron
{
    /**
     * Turns a native function's answer into WordPress's: an error marker becomes a WP_Error.
     *
     *  A merge marker is `wp_clear_scheduled_hook()`'s: one new WP_Error that merges each unschedule error. `false`
     *  when WP_Error isn't loaded, as the other ported areas do: there is nothing to return the error in.
     *
     * @param mixed $result
     *   The native answer.
     * @return mixed
     *   WordPress's return value.
     */
    function result($result)
    {
        if (is_array($result) && isset($result['__speedyspec_error']) && true === $result['__speedyspec_error'])
        {
            return error($result);
        }
        if (is_array($result) && isset($result['__speedyspec_merge']) && true === $result['__speedyspec_merge'])
        {
            if (!class_exists('WP_Error', false))
            {
                return false;
            }
            $error = new \WP_Error();
            foreach ($result['errors'] as $each)
            {
                $each = result($each);
                if ($each instanceof \WP_Error)
                {
                    $error->merge_from($each);
                }
            }
            return $error;
        }
        return $result;
    }

    /**
     * Builds the WP_Error an error marker describes.
     *
     * @param array $marker
     *   The code and the translated message.
     * @return \WP_Error|false
     *   The error.
     */
    function error(array $marker)
    {
        if (!class_exists('WP_Error', false))
        {
            return false;
        }
        return new \WP_Error($marker['code'], $marker['message']);
    }

    /**
     * Reports a failed step of a due event as `wp-cron.php` does: `error_log()` of the translated message with the
     *  hook, the error's code and message and the event as JSON. Returns the WP_Error for the step's error action.
     *
     * @param string $step
     *   `reschedule` or `unschedule`.
     * @param mixed $hook
     *   The event's hook.
     * @param mixed $error
     *   The step's answer: an error marker, or a WP_Error a filter returned.
     * @param array $event
     *   The event as the cron array holds it.
     * @return mixed
     *   The WP_Error.
     */
    function report_error($step, $hook, $error, $event)
    {
        $error = result($error);
        if (!($error instanceof \WP_Error))
        {
            return $error;
        }
        $format = 'reschedule' === $step
            ? 'Cron reschedule event error for hook: %1$s, Error code: %2$s, Error message: %3$s, Data: %4$s'
            : 'Cron unschedule event error for hook: %1$s, Error code: %2$s, Error message: %3$s, Data: %4$s';
        if (function_exists('__'))
        {
            /* translators: 1: Hook name, 2: Error code, 3: Error message, 4: Event data. */
            $format = __($format);
        }
        error_log(
            sprintf(
                $format,
                $hook,
                $error->get_error_code(),
                $error->get_error_message(),
                function_exists('wp_json_encode') ? wp_json_encode($event) : json_encode($event)
            )
        );
        return $error;
    }

    /**
     * Casts an array to the stdClass event WordPress hands to the cron filters.
     *
     * @param array $fields
     *   The event's fields, in WordPress's order.
     * @return object
     *   The event.
     */
    function to_object($fields)
    {
        return (object) $fields;
    }

    /**
     * Reads a property of an event a filter returned: `[true, $value]` when it is set, `[false, null]` otherwise.
     *
     * @param mixed $object
     *   The event.
     * @param string $name
     *   The property.
     * @return array
     *   Whether it is set, and its value.
     */
    function property($object, $name)
    {
        if (is_object($object) && isset($object->$name))
        {
            return array(true, $object->$name);
        }
        return array(false, null);
    }

    /**
     * Tells whether a filter's answer is a WP_Error, through WordPress's own `is_wp_error()` when it is loaded.
     *
     * @param mixed $thing
     *   The answer.
     * @return bool
     *   Whether it is a WP_Error.
     */
    function is_wp_error($thing)
    {
        if (function_exists('is_wp_error'))
        {
            return \is_wp_error($thing);
        }
        return class_exists('WP_Error', false) && $thing instanceof \WP_Error;
    }

    /**
     * The current network's active sites, for the server's per-site cron calls (the WP_Site objects stay in PHP).
     *
     * @return list<array{0: int, 1: string, 2: string}>
     *   Each site's id, domain and path, in id order.
     */
    function network_sites()
    {
        $sites = array();
        $query = array(
            'fields'     => '',
            'number'     => 0,
            'spam'       => 0,
            'deleted'    => 0,
            'archived'   => 0,
            'network_id' => \get_current_network_id(),
            'orderby'    => 'id',
            'order'      => 'ASC',
        );
        foreach ((array) \get_sites($query) as $site)
        {
            // A `the_sites` filter may hand back anything: only a site with an id is called.
            if (!is_object($site) || (int) $site->blog_id <= 0)
            {
                continue;
            }
            $sites[] = array((int) $site->blog_id, (string) $site->domain, (string) $site->path);
        }
        return $sites;
    }
}

namespace
{
    use SpeedySpec\WpCron\Native as N;

    if (!function_exists('wp_schedule_single_event'))
    {
        /**
         * Schedules an event to run only once.
         */
        function wp_schedule_single_event($timestamp, $hook, $args = array(), $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\wp_schedule_single_event($timestamp, $hook, $args, $wp_error));
        }

        /**
         * Schedules a recurring event.
         */
        function wp_schedule_event($timestamp, $recurrence, $hook, $args = array(), $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\wp_schedule_event($timestamp, $recurrence, $hook, $args, $wp_error));
        }

        /**
         * Reschedules a recurring event.
         */
        function wp_reschedule_event($timestamp, $recurrence, $hook, $args = array(), $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\wp_reschedule_event($timestamp, $recurrence, $hook, $args, $wp_error));
        }

        /**
         * Unschedules a previously scheduled event.
         */
        function wp_unschedule_event($timestamp, $hook, $args = array(), $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\wp_unschedule_event($timestamp, $hook, $args, $wp_error));
        }

        /**
         * Unschedules all events attached to the hook with the specified arguments.
         *
         *  Before WordPress 3.0 the arguments were passed one by one rather than as an array; that call still works,
         *  with a deprecation notice, and needs `func_get_args()`, so it is handled here.
         */
        function wp_clear_scheduled_hook($hook, $args = array(), $wp_error = false)
        {
            if (!is_array($args))
            {
                _deprecated_argument(
                    __FUNCTION__,
                    '3.0.0',
                    __('This argument has changed to an array to match the behavior of the other cron functions.')
                );
                $args     = array_slice(func_get_args(), 1);
                $wp_error = false;
            }
            return \SpeedySpec\WpCron\result(N\wp_clear_scheduled_hook($hook, $args, $wp_error));
        }

        /**
         * Unschedules all events attached to the hook.
         */
        function wp_unschedule_hook($hook, $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\wp_unschedule_hook($hook, $wp_error));
        }

        /**
         * Retrieves a scheduled event.
         */
        function wp_get_scheduled_event($hook, $args = array(), $timestamp = null)
        {
            return N\wp_get_scheduled_event($hook, $args, $timestamp);
        }

        /**
         * Retrieves the timestamp of the next scheduled event for the given hook.
         */
        function wp_next_scheduled($hook, $args = array())
        {
            return N\wp_next_scheduled($hook, $args);
        }

        /**
         * Retrieves supported event recurrence schedules.
         */
        function wp_get_schedules()
        {
            return N\wp_get_schedules();
        }

        /**
         * Retrieves the name of the recurrence schedule for an event.
         */
        function wp_get_schedule($hook, $args = array())
        {
            return N\wp_get_schedule($hook, $args);
        }

        /**
         * Retrieves cron jobs ready to be run.
         */
        function wp_get_ready_cron_jobs()
        {
            return N\wp_get_ready_cron_jobs();
        }

        /**
         * Retrieves the cron array, without its version.
         */
        function _get_cron_array()
        {
            return N\_get_cron_array();
        }

        /**
         * Updates the cron option with the new cron array.
         */
        function _set_cron_array($cron, $wp_error = false)
        {
            return \SpeedySpec\WpCron\result(N\_set_cron_array($cron, $wp_error));
        }

        /**
         * Asks for the cron runner when an event is due, instead of WordPress's loopback request to `wp-cron.php`.
         *
         *  WordPress's checks run natively (a cron request already running, the `doing_cron` lock, no ready events,
         *  the first one ahead). When they pass, `doing_cron` is set and `cron_request` is filtered with WordPress's
         *  array, but no request is sent: the server's scheduler is told to call the cron endpoint at once.
         *  `ALTERNATE_WP_CRON` has no effect.
         */
        function spawn_cron($gmt_time = 0)
        {
            $doing_wp_cron = N\spawn_cron(
                $gmt_time,
                defined('DOING_CRON') || isset($_GET['doing_wp_cron']),
                defined('WP_CRON_LOCK_TIMEOUT') ? WP_CRON_LOCK_TIMEOUT : 60
            );
            if (false === $doing_wp_cron)
            {
                return false;
            }
            $cron_url = add_query_arg('doing_wp_cron', $doing_wp_cron, site_url('wp-cron.php'));
            /** This filter is documented in wp-includes/cron.php */
            apply_filters(
                'cron_request',
                array(
                    'url'  => $cron_url,
                    'key'  => $doing_wp_cron,
                    'args' => array(
                        'timeout'   => 0.01,
                        'blocking'  => false,
                        /** This filter is documented in wp-includes/class-wp-http-streams.php */
                        'sslverify' => apply_filters('https_local_ssl_verify', false, $cron_url),
                    ),
                ),
                $doing_wp_cron
            );
            return true;
        }

        /**
         * Registers `_wp_cron()` to run on the `shutdown` action, as WordPress does.
         */
        function wp_cron(): void
        {
            if (defined('ALTERNATE_WP_CRON') && ALTERNATE_WP_CRON)
            {
                if (did_action('wp_loaded'))
                {
                    _wp_cron();
                }
                else
                {
                    add_action('wp_loaded', '_wp_cron', 20);
                }
            }
            elseif (doing_action('shutdown'))
            {
                _wp_cron();
            }
            else
            {
                add_action('shutdown', '_wp_cron');
            }
        }

        /**
         * Asks for the cron runner when an event is due, as WordPress's `_wp_cron()` (its body, over the native
         *  `spawn_cron()`).
         */
        function _wp_cron()
        {
            // Prevent infinite loops caused by lack of wp-cron.php.
            if (str_contains($_SERVER['REQUEST_URI'], '/wp-cron.php')
                || (defined('DISABLE_WP_CRON') && DISABLE_WP_CRON))
            {
                return 0;
            }
            $crons = wp_get_ready_cron_jobs();
            if (empty($crons))
            {
                return 0;
            }
            $gmt_time = microtime(true);
            $keys     = array_keys($crons);
            if (isset($keys[0]) && $keys[0] > $gmt_time)
            {
                return 0;
            }
            $schedules = wp_get_schedules();
            $results   = array();
            foreach ($crons as $timestamp => $cronhooks)
            {
                if ($timestamp > $gmt_time)
                {
                    break;
                }
                foreach ((array) $cronhooks as $hook => $args)
                {
                    if (isset($schedules[$hook]['callback'])
                        && !call_user_func($schedules[$hook]['callback']))
                    {
                        continue;
                    }
                    $results[] = spawn_cron($gmt_time);
                    break 2;
                }
            }
            if (in_array(false, $results, true))
            {
                return false;
            }
            return count($results);
        }

        /**
         * Upgrades a cron info array from before WordPress 2.1.
         */
        function _upgrade_cron_array($cron)
        {
            return N\_upgrade_cron_array($cron);
        }
    }
}
