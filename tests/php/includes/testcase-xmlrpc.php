<?php
// Copied from WordPress (wordpress-develop branch 7.1), tests/phpunit/includes/testcase-xmlrpc.php. GPL-2.0-or-later.
// Changes: the requires of wp-admin/includes/admin.php (which needs the database) and class-wp-xmlrpc-server.php are
// removed: wp_xmlrpc_server is SpeedySpec's shim, which WordPress's class would redeclare. The rest is unchanged.
require_once ABSPATH . WPINC . '/class-IXR.php';

abstract class WP_XMLRPC_UnitTestCase extends WP_UnitTestCase {
	/**
	 * @var wp_xmlrpc_server
	 */
	protected $myxmlrpcserver;

	public function set_up() {
		parent::set_up();

		add_filter( 'pre_option_enable_xmlrpc', '__return_true' );

		$this->myxmlrpcserver = new wp_xmlrpc_server();
	}

	public function tear_down() {
		remove_filter( 'pre_option_enable_xmlrpc', '__return_true' );

		$this->remove_added_uploads();

		parent::tear_down();
	}

	protected static function make_user_by_role( $role ) {
		return self::factory()->user->create(
			array(
				'user_login' => $role,
				'user_pass'  => $role,
				'role'       => $role,
			)
		);
	}
}
