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
         * Upgrades a cron info array from before WordPress 2.1.
         */
        function _upgrade_cron_array($cron)
        {
            return N\_upgrade_cron_array($cron);
        }
    }
}
