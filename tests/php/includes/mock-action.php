<?php
/*
 * Copied unchanged from WordPress's own tests (wordpress-develop, tests/phpunit/includes/utils.php, class MockAction).
 *
 * WordPress is free software, released under the GNU General Public License, version 2 or later.
 * Copyright 2011-2026 by the WordPress contributors. See https://wordpress.org/about/license/.
 */

/**
 * Helper class for testing code that involves actions and filters.
 *
 * Typical use:
 *
 *     $ma = new MockAction();
 *     add_action( 'foo', array( &$ma, 'action' ) );
 *
 * @since UT (3.7.0)
 *
 * @phpstan-type Hook_Event array{
 *     action?: non-empty-string,
 *     filter?: non-empty-string,
 *     hook_name: string|false,
 *     tag: string|false,
 *     args: list<mixed>,
 * }
 */
class MockAction {

	/**
	 * @var list<Hook_Event>
	 */
	public $events;

	/**
	 * @var bool
	 */
	public $debug;

	/**
	 * PHP5 constructor.
	 *
	 * @since UT (3.7.0)
	 *
	 * @param bool|int $debug
	 */
	public function __construct( $debug = 0 ) {
		$this->reset();
		$this->debug = (bool) $debug;
	}

	/**
	 * @since UT (3.7.0)
	 */
	public function reset(): void {
		$this->events = array();
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @global array<non-empty-string, non-negative-int> $wp_actions
	 * @return string|false
	 */
	public function current_filter() {
		global $wp_actions;

		if ( is_callable( 'current_filter' ) ) {
			$current_filter = current_filter();
		} else {
			$current_filter   = array_key_last( $wp_actions );
			$current_filter ??= false;
		}

		return $current_filter;
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @template T
	 * @param T $arg
	 * @return T
	 */
	public function action( $arg ) {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'action'    => __FUNCTION__,
			'hook_name' => $current_filter,
			'tag'       => $current_filter, // Back compat.
			'args'      => func_get_args(),
		);

		return $arg;
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @template T
	 * @param T $arg
	 * @return T
	 */
	public function action2( $arg ) {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'action'    => __FUNCTION__,
			'hook_name' => $current_filter,
			'tag'       => $current_filter, // Back compat.
			'args'      => func_get_args(),
		);

		return $arg;
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @template T
	 * @param T $arg
	 * @return T
	 */
	public function filter( $arg ) {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'filter'    => __FUNCTION__,
			'hook_name' => $current_filter,
			'tag'       => $current_filter, // Back compat.
			'args'      => func_get_args(),
		);

		return $arg;
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @template T
	 * @param T $arg
	 * @return T
	 */
	public function filter2( $arg ) {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'filter'    => __FUNCTION__,
			'hook_name' => $current_filter,
			'tag'       => $current_filter, // Back compat.
			'args'      => func_get_args(),
		);

		return $arg;
	}

	/**
	 * @since UT (3.7.0)
	 *
	 * @no-named-arguments
	 * @param string $arg
	 * @return non-empty-string
	 */
	public function filter_append( $arg ): string {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'filter'    => __FUNCTION__,
			'hook_name' => $current_filter,
			'tag'       => $current_filter, // Back compat.
			'args'      => func_get_args(),
		);

		return $arg . '_append';
	}

	/**
	 * Does not return the result, so it's safe to use with the 'all' filter.
	 *
	 * @since UT (3.7.0)
	 *
	 * @no-named-arguments
	 * @param string $hook_name
	 * @param mixed ...$args
	 */
	public function filterall( string $hook_name, ...$args ): void {
		$current_filter = $this->current_filter();

		if ( $this->debug ) {
			dmp( __FUNCTION__, $current_filter );
		}

		$this->events[] = array(
			'filter'    => __FUNCTION__,
			'hook_name' => $hook_name,
			'tag'       => $hook_name, // Back compat.
			'args'      => $args,
		);
	}

	/**
	 * Returns a list of all the actions, hook names and args.
	 *
	 * @since UT (3.7.0)
	 *
	 * @return list<Hook_Event>
	 */
	public function get_events(): array {
		return $this->events;
	}

	/**
	 * Returns a count of the number of times the action was called since the last reset.
	 *
	 * @since UT (3.7.0)
	 *
	 * @param string $hook_name Hook name.
	 * @return non-negative-int
	 */
	public function get_call_count( $hook_name = '' ): int {
		if ( $hook_name ) {
			$count = 0;

			foreach ( $this->events as $e ) {
				if (
					( isset( $e['action'] ) && $e['action'] === $hook_name ) ||
					( isset( $e['filter'] ) && $e['filter'] === $hook_name )
				) {
					++$count;
				}
			}

			return $count;
		}

		return count( $this->events );
	}

	/**
	 * Returns an array of the hook names that triggered calls to this action.
	 *
	 * @since 6.1.0
	 *
	 * @return list<string|false>
	 */
	public function get_hook_names(): array {
		$out = array();

		foreach ( $this->events as $e ) {
			$out[] = $e['hook_name'];
		}

		return $out;
	}

	/**
	 * Returns an array of the hook names that triggered calls to this action.
	 *
	 * @since UT (3.7.0)
	 * @since 6.1.0 Turned into an alias for ::get_hook_names().
	 *
	 * @return list<string|false>
	 */
	public function get_tags(): array {
		return $this->get_hook_names();
	}

	/**
	 * Returns an array of args passed in calls to this action.
	 *
	 * @since UT (3.7.0)
	 *
	 * @return list<list<mixed>>
	 */
	public function get_args(): array {
		$out = array();

		foreach ( $this->events as $e ) {
			$out[] = $e['args'];
		}

		return $out;
	}
}
