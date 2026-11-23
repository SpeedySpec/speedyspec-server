<?php
/*
 * Copied from WordPress (wordpress-develop branch 7.1, the code of the pinned 7.1.5 release),
 * tests/phpunit/tests/xmlrpc/basic.php (renamed for PHPUnit 11, which needs the class name), for SpeedySpec's XML-RPC conformance suite.
 * GPL-2.0-or-later, as WordPress.
 *
 * Changes: test bodies are unchanged; the tests that need the database or the factory are removed:
 *   test_login_pass_ok
 *   test_login_pass_bad
 *   test_multicall_invalidates_all_calls_after_invalid_call (covered by speedyspec-xmlrpc-php's
 *   tests/legacy.rs, a_failed_login_in_a_legacy_method_blocks_an_operation_login_later_in_the_multicall)
 * and the file's requires of wp-admin/includes/admin.php (which needs the database) and class-wp-xmlrpc-server.php
 * (wp_xmlrpc_server is SpeedySpec's shim, which WordPress's class would redeclare).
 */

require_once ABSPATH . WPINC . '/class-IXR.php';

/**
 * @group xmlrpc
 */
class Tests_XMLRPC_Basic extends WP_XMLRPC_UnitTestCase {
	public function test_enabled() {
		$result = $this->myxmlrpcserver->wp_getOptions( array( 1, 'username', 'password' ) );

		$this->assertIXRError( $result );
		// If disabled, 405 would result.
		$this->assertSame( 403, $result->code );
	}
	/**
	 * @ticket 36586
	 */
	public function test_isStruct_on_non_numerically_indexed_array() {
		$value = new IXR_Value( array( '0.0' => 100 ) );

		$return  = "<struct>\n";
		$return .= "  <member><name>0.0</name><value><int>100</int></value></member>\n";
		$return .= '</struct>';

		$this->assertXmlStringEqualsXmlString( $return, $value->getXML() );
	}

	public function test_disabled() {
		add_filter( 'xmlrpc_enabled', '__return_false' );
		$testcase_xmlrpc_server = new wp_xmlrpc_server();
		$result                 = $testcase_xmlrpc_server->wp_getOptions( array( 1, 'username', 'password' ) );

		$this->assertIXRError( $result );
		$this->assertSame( 405, $result->code );
	}
}
