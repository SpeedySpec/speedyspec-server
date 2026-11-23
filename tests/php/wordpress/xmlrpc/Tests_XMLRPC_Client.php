<?php
/*
 * Copied from WordPress (wordpress-develop branch 7.1, the code of the pinned 7.1.5 release),
 * tests/phpunit/tests/xmlrpc/client.php (renamed for PHPUnit 11, which needs the class name), for SpeedySpec's XML-RPC conformance suite.
 * GPL-2.0-or-later, as WordPress.
 *
 * Changes: none: the file is unchanged.
 */
require_once ABSPATH . WPINC . '/class-IXR.php';
require_once ABSPATH . WPINC . '/class-wp-http-ixr-client.php';

/**
 * @group xmlrpc
 */
class Tests_XMLRPC_Client extends WP_XMLRPC_UnitTestCase {

	/**
	 * @ticket 26947
	 */
	public function test_ixr_client_allows_query_strings() {
		$client = new IXR_Client( 'http://example.com/server.php?this-is-needed=true#not-this' );
		$this->assertSame( 'example.com', $client->server );
		$this->assertSame( 80, $client->port );
		$this->assertSame( '/server.php?this-is-needed=true', $client->path );
	}

	/**
	 * @ticket 64635
	 */
	public function test_ixr_client_can_handle_missing_host() {
		$client = new IXR_Client( '/no-host-here' );
		$this->assertSame( '', $client->server );
	}

	/**
	 * @ticket 26947
	 */
	public function test_wp_ixr_client_allows_query_strings() {
		$client = new WP_HTTP_IXR_Client( 'http://example.com/server.php?this-is-needed=true#not-this' );
		$this->assertSame( 'example.com', $client->server );
		$this->assertFalse( $client->port );
		$this->assertSame( '/server.php?this-is-needed=true', $client->path );
	}

	/**
	 * @ticket 40784
	 */
	public function test_wp_ixr_client_can_handle_protocolless_urls() {
		$client = new WP_HTTP_IXR_Client( '//example.com/server.php' );
		$this->assertSame( '', $client->scheme );
		$this->assertSame( 'example.com', $client->server );
	}

	/**
	 * @ticket 40784
	 */
	public function test_wp_ixr_client_can_handle_relative_urls() {
		$client = new WP_HTTP_IXR_Client( '/server.php' );
		$this->assertSame( '', $client->scheme );
		$this->assertSame( '', $client->server );
		$this->assertSame( '/server.php', $client->path );
	}

	/**
	 * @ticket 40784
	 */
	public function test_wp_ixr_client_can_handle_invalid_urls() {
		$client = new WP_HTTP_IXR_Client( '' );
		$this->assertSame( '', $client->scheme );
		$this->assertSame( '', $client->server );
		$this->assertSame( '/', $client->path );
	}
}
