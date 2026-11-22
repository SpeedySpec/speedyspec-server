<?php

/**
 * The file signature tests of tests/phpunit/tests/file.php (wordpress-develop trunk ce78d2d6eb), unchanged. The
 * rest of that class tests file helpers this port does not touch.
 *
 * @group file
 */
class Tests_File_fileSignature extends WP_UnitTestCase {


	/**
	 * @ticket 47186
	 */
	public function test_file_signature_functions_as_expected() {
		$file = wp_tempnam();
		file_put_contents( $file, 'WordPress' );

		// The signature of 'WordPress' after SHA384 hashing, for verification against the key within self::filter_trust_plus85Tq_key().
		$expected_signature = 'PmNv0b1ziwJAsVhjdpjd4+PQZidZWSlBm5b+GbbwE9m9HVKDFhEyvyRTHkRYOLypB8P2YvbW7CoOMZqGh8mEAA==';

		add_filter( 'wp_trusted_keys', array( $this, 'filter_trust_plus85Tq_key' ) );

		// Measure how long the call takes.
		$timer_start = microtime( 1 );
		$verify      = verify_file_signature( $file, $expected_signature, 'WordPress' );
		$timer_end   = microtime( 1 );
		$time_taken  = ( $timer_end - $timer_start );

		unlink( $file );
		remove_filter( 'wp_trusted_keys', array( $this, 'filter_trust_plus85Tq_key' ) );

		// verify_file_signature() should intentionally never take more than 10s to run.
		$this->assertLessThan( 10, $time_taken, 'verify_file_signature() took longer than 10 seconds.' );

		// Check to see if the system parameters prevent signature verifications.
		if ( is_wp_error( $verify ) && 'signature_verification_unsupported' === $verify->get_error_code() ) {
			$this->markTestSkipped( 'This system does not support Signature Verification.' );
		}

		$this->assertNotWPError( $verify );
		$this->assertTrue( $verify );
	}

	/**
	 * @ticket 47186
	 */
	public function test_file_signature_expected_failure() {
		$file = wp_tempnam();
		file_put_contents( $file, 'WordPress' );

		// Test an invalid signature.
		$expected_signature = base64_encode( str_repeat( 'A', SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES ) );
		$verify             = verify_file_signature( $file, $expected_signature, 'WordPress' );
		unlink( $file );

		if ( is_wp_error( $verify ) && 'signature_verification_unsupported' === $verify->get_error_code() ) {
			$this->markTestSkipped( 'This system does not support Signature Verification.' );
		}

		$this->assertWPError( $verify );
		$this->assertSame( 'signature_verification_failed', $verify->get_error_code() );
	}

	public function filter_trust_plus85Tq_key( $keys ) {
		// A static once-off key used to verify verify_file_signature() works as expected.
		$keys[] = '+85TqMhxQVAYVW4BSCVkJQvZH4q7z8I9lePbvngvf7A=';

		return $keys;
	}
}
