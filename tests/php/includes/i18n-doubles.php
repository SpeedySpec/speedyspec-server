<?php

declare(strict_types=1);

/**
 * In-memory stand-ins for the WordPress code the locale tests reach outside the translation layer: users and their
 * meta, the admin screen, theme switching and option writes. None of it is ported yet, and WordPress's own versions
 * need a database install.
 *
 * WordPress's test case rolls the database back after every test; `speedyspec_doubles_snapshot()` and
 * `speedyspec_doubles_restore()` do the same for this state (WP_UnitTestCase calls them). Users made in
 * `wpSetUpBeforeClass` are made before the first snapshot, so they last for the class, as in WordPress.
 */

/**
 * The doubles' state: users by id, options written by tests, and the active theme.
 *
 * @return array{users: array<int, array<string, mixed>>, options: array<string, mixed>, next_user: int, current_user: int, stylesheet: string}
 *     The state, by reference.
 */
function &speedyspec_doubles(): array
{
    static $state = array(
        'users' => array(),
        'options' => array(),
        'next_user' => 1,
        'current_user' => 0,
        'stylesheet' => 'default',
    );
    return $state;
}

$GLOBALS['speedyspec_doubles_snapshots'] = array();

/**
 * Saves the doubles' state, as WordPress's test case starts a transaction.
 */
function speedyspec_doubles_snapshot(): void
{
    $GLOBALS['speedyspec_doubles_snapshots'][] = speedyspec_doubles();
}

/**
 * Restores the last saved state, as WordPress's test case rolls back.
 */
function speedyspec_doubles_restore(): void
{
    $state = &speedyspec_doubles();
    $saved = array_pop($GLOBALS['speedyspec_doubles_snapshots']);
    if (null !== $saved)
    {
        $state = $saved;
    }
}

/**
 * A user, with the fields the locale code reads.
 */
#[AllowDynamicProperties]
class WP_User
{
    /** @var int */
    public $ID = 0;

    /**
     * @param int $id
     *     The user's id; 0 is no user.
     */
    public function __construct($id = 0)
    {
        $this->ID = (int) $id;
    }

    /**
     * Reads meta as WordPress's magic getter does (`locale` is user meta).
     *
     * @param string $key
     *     The field.
     * @return mixed
     *     The meta value, or '' when unset.
     */
    public function __get($key)
    {
        return speedyspec_doubles()['users'][$this->ID]['meta'][$key] ?? '';
    }

    /**
     * @param string $key
     *     The field.
     * @return bool
     *     Whether the meta is set.
     */
    public function __isset($key)
    {
        return isset(speedyspec_doubles()['users'][$this->ID]['meta'][$key]);
    }

    /**
     * @return bool
     *     Whether the user exists.
     */
    public function exists()
    {
        return isset(speedyspec_doubles()['users'][$this->ID]);
    }
}

/**
 * Creates a user, as WordPress's `wp_insert_user()` (the test factory calls it).
 *
 * @param array<string, mixed> $args
 *     `role`, `locale` and any other meta.
 * @return int
 *     The new user's id.
 */
function wp_insert_user($args)
{
    $state = &speedyspec_doubles();
    $id = $state['next_user']++;
    $meta = $args;
    unset($meta['role']);
    $state['users'][$id] = array('role' => $args['role'] ?? 'subscriber', 'meta' => $meta);
    return $id;
}

/**
 * @param string $field
 *     Only `id` is supported.
 * @param int|string $value
 *     The id.
 * @return WP_User|false
 *     The user, or false when there is none.
 */
function get_user_by($field, $value)
{
    $id = (int) $value;
    return ('id' === strtolower((string) $field) && isset(speedyspec_doubles()['users'][$id])) ? new WP_User($id) : false;
}

/**
 * @param int $user_id
 *     The user's id.
 * @return WP_User|false
 *     The user, or false.
 */
function get_userdata($user_id)
{
    return get_user_by('id', $user_id);
}

/**
 * @return WP_User
 *     The current user (id 0 when none).
 */
function wp_get_current_user()
{
    return new WP_User(speedyspec_doubles()['current_user']);
}

/**
 * @param int $id
 *     The user to make current.
 * @param string $name
 *     Unused.
 * @return WP_User
 *     The current user.
 */
function wp_set_current_user($id, $name = '')
{
    $state = &speedyspec_doubles();
    $state['current_user'] = (int) $id;
    return wp_get_current_user();
}

/**
 * @return int
 *     The current user's id.
 */
function get_current_user_id()
{
    return speedyspec_doubles()['current_user'];
}

/**
 * @param int $user_id
 *     The user.
 * @param string $meta_key
 *     The key.
 * @param mixed $meta_value
 *     The value.
 * @param mixed $prev_value
 *     Unused.
 * @return bool
 *     True.
 */
function update_user_meta($user_id, $meta_key, $meta_value, $prev_value = '')
{
    $state = &speedyspec_doubles();
    $state['users'][(int) $user_id]['meta'][$meta_key] = $meta_value;
    return true;
}

/**
 * @param int $user_id
 *     The user.
 * @param string $key
 *     The key, or '' for all.
 * @param bool $single
 *     Whether to return one value.
 * @return mixed
 *     The meta.
 */
function get_user_meta($user_id, $key = '', $single = false)
{
    $meta = speedyspec_doubles()['users'][(int) $user_id]['meta'] ?? array();
    if ('' === $key)
    {
        return $meta;
    }
    if (!array_key_exists($key, $meta))
    {
        return $single ? '' : array();
    }
    return $single ? $meta[$key] : array($meta[$key]);
}

/**
 * The admin screen, as far as `is_admin()` asks it.
 */
final class WP_Screen
{
    /** @var string */
    public $id;

    /**
     * @param string $id
     *     The screen id.
     */
    public function __construct($id)
    {
        $this->id = (string) $id;
    }

    /**
     * @param string|null $admin
     *     Unused.
     * @return bool
     *     Whether the screen is in the admin (every screen but `front`).
     */
    public function in_admin($admin = null)
    {
        return 'front' !== $this->id;
    }
}

/**
 * @param string $hook_name
 *     The screen id.
 */
function set_current_screen($hook_name = '')
{
    $GLOBALS['current_screen'] = new WP_Screen($hook_name);
}

/**
 * Switches the active theme (a directory under `tests/php/data/themedir1`).
 *
 * @param string $stylesheet
 *     The theme's directory name.
 */
function switch_theme($stylesheet)
{
    $state = &speedyspec_doubles();
    $state['stylesheet'] = (string) $stylesheet;
}

/**
 * @return string
 *     The active theme's directory.
 */
function get_template_directory()
{
    return DIR_TESTDATA . '/themedir1/' . speedyspec_doubles()['stylesheet'];
}

/**
 * @return string
 *     The active theme's directory.
 */
function get_stylesheet_directory()
{
    return get_template_directory();
}

/**
 * @param string $option
 *     The option.
 * @param mixed $default_value
 *     Returned when unset.
 * @return mixed
 *     The option, as WordPress's test install answers it.
 */
function get_option($option, $default_value = false)
{
    $options = speedyspec_doubles()['options']
        + array('blog_charset' => 'UTF-8', 'siteurl' => 'http://example.org', 'home' => 'http://example.org');
    return array_key_exists($option, $options) ? $options[$option] : $default_value;
}

/**
 * @param string $option
 *     The option.
 * @param mixed $value
 *     The value.
 * @param bool|null $autoload
 *     Unused.
 * @return bool
 *     True.
 */
function update_option($option, $value, $autoload = null)
{
    $state = &speedyspec_doubles();
    $state['options'][$option] = $value;
    return true;
}

/**
 * @param string $option
 *     The option.
 * @return bool
 *     True.
 */
function delete_option($option)
{
    $state = &speedyspec_doubles();
    unset($state['options'][$option]);
    return true;
}
