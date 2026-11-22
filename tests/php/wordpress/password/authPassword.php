<?php

/**
 * The database-free password tests of tests/phpunit/tests/auth.php (wordpress-develop trunk 842b8d8e5c), unchanged. The
 * rest of that class signs users in, which needs a database. The data providers are static, as PHPUnit 11 requires.
 *
 * Expected to fail, on purpose: the phpass and MD5 tests (SpeedySpec verifies neither), and the three that expect
 * bcrypt as the default (SpeedySpec hashes with argon2id).
 *
 * @group pluggable
 * @group auth
 */
class Tests_Auth_authPassword extends WP_UnitTestCase {

	/**
	 * @var PasswordHash
	 */
	protected static $wp_hasher;

	public static function set_up_before_class() {
		parent::set_up_before_class();

		require_once ABSPATH . WPINC . '/class-phpass.php';
		self::$wp_hasher = new PasswordHash( 8, true );
	}

	/**
	 * Test wp_hash_password trims whitespace
	 *
	 * This is similar to test_password_trimming but tests the "lower level"
	 * wp_hash_password function
	 *
	 * @ticket 24973
	 * @dataProvider data_passwords_with_whitespace
	 */
	public function test_wp_hash_password_trimming( $password_with_whitespace, $expected_password ) {
		$this->assertTrue( wp_check_password( $expected_password, wp_hash_password( $password_with_whitespace ) ) );
	}

	public static function data_passwords_with_whitespace() {
		return array(
			'leading whitespace'  => array( ' pass with leading whitespace', 'pass with leading whitespace' ),
			'trailing whitespace' => array( 'pass with trailing whitespace ', 'pass with trailing whitespace' ),
			'both whitespace'     => array( ' pass with whitespace ', 'pass with whitespace' ),
			'new line'            => array( "pass with new line \n", 'pass with new line' ),
			'vertical tab'        => array( "pass with vertical tab o_O\x0B", 'pass with vertical tab o_O' ),
		);
	}

	/**
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_phpass_hash() {
		$password = 'password';
		$hash     = self::$wp_hasher->HashPassword( $password );
		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * Ensure wp_check_password() remains compatible with an increase to the default bcrypt cost.
	 *
	 * The test verifies this by reducing the cost used to generate the hash, therefore mimicing a hash
	 * which was generated prior to the default cost being increased.
	 *
	 * Notably the bcrypt cost was increased in PHP 8.4: https://wiki.php.net/rfc/bcrypt_cost_2023 .
	 *
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_hash_with_increased_bcrypt_cost() {
		$password = 'password';

		// Reducing the cost mimics an increase to the default cost.
		add_filter( 'wp_hash_password_options', array( $this, 'reduce_hash_cost' ) );
		$hash = wp_hash_password( $password, PASSWORD_BCRYPT );
		remove_filter( 'wp_hash_password_options', array( $this, 'reduce_hash_cost' ) );

		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
		$this->assertTrue( wp_password_needs_rehash( $hash ) );
	}

	/**
	 * Ensure wp_check_password() remains compatible with a reduction of the default bcrypt cost.
	 *
	 * The test verifies this by increasing the cost used to generate the hash, therefore mimicing a hash
	 * which was generated prior to the default cost being reduced.
	 *
	 * A reduction of the cost is unlikely to occur but is fully supported.
	 *
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_hash_with_reduced_bcrypt_cost() {
		$password = 'password';

		// Increasing the cost mimics a reduction of the default cost.
		add_filter( 'wp_hash_password_options', array( $this, 'increase_hash_cost' ) );
		$hash = wp_hash_password( $password, PASSWORD_BCRYPT );
		remove_filter( 'wp_hash_password_options', array( $this, 'increase_hash_cost' ) );

		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
		$this->assertTrue( wp_password_needs_rehash( $hash ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_wp_hash_with_default_bcrypt_cost() {
		$password = 'password';

		$hash = wp_hash_password( $password, PASSWORD_BCRYPT );

		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
		$this->assertFalse( wp_password_needs_rehash( $hash ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_plain_bcrypt_hash_with_default_bcrypt_cost() {
		$password = 'password';

		$hash = password_hash( $password, PASSWORD_BCRYPT );

		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
		$this->assertTrue( wp_password_needs_rehash( $hash ) );
	}

	/**
	 * Ensure wp_check_password() is compatible with Argon2i hashes.
	 *
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_argon2i_hash() {
		if ( ! defined( 'PASSWORD_ARGON2I' ) ) {
			$this->markTestSkipped( 'Argon2i is not supported.' );
		}

		$password = 'password';
		$hash     = password_hash( trim( $password ), PASSWORD_ARGON2I );
		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * Ensure wp_check_password() is compatible with Argon2id hashes.
	 *
	 * @requires PHP >= 7.3
	 *
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_argon2id_hash() {
		if ( ! defined( 'PASSWORD_ARGON2ID' ) ) {
			$this->markTestSkipped( 'Argon2id is not supported.' );
		}

		$password = 'password';
		$hash     = password_hash( trim( $password ), PASSWORD_ARGON2ID );
		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_wp_check_password_supports_md5_hash() {
		$password = 'password';
		$hash     = md5( $password );
		$this->assertTrue( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_wp_check_password_does_not_support_plain_text() {
		$password = 'password';
		$hash     = $password;
		$this->assertFalse( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * @ticket 21022
	 *
	 * @dataProvider data_empty_values
	 * @param mixed $value
	 */
	public function test_wp_check_password_does_not_support_empty_hash( $value ) {
		$password = 'password';
		$hash     = $value;
		$this->assertFalse( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	/**
	 * @ticket 21022
	 *
	 * @dataProvider data_empty_values
	 * @param mixed $value
	 */
	public function test_wp_check_password_does_not_support_empty_password( $value ) {
		$password = $value;
		$hash     = $value;
		$this->assertFalse( wp_check_password( $password, $hash ) );
		$this->assertSame( 1, did_filter( 'check_password' ) );
	}

	public static function data_empty_values() {
		return array(
			// String zero:
			array( '0' ),
			// Zero-length string:
			array( '' ),
			// Null byte character:
			array( "\0" ),
			// Asterisk values:
			array( '*' ),
			array( '*0' ),
			array( '*1' ),
		);
	}

	/**
	 * @ticket 21022
	 */
	public function test_password_rehashing_requirement_can_be_filtered() {
		$filter_count_before = did_filter( 'password_needs_rehash' );

		wp_password_needs_rehash( '$hash' );

		$this->assertSame( $filter_count_before + 1, did_filter( 'password_needs_rehash' ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_password_hashing_algorithm_can_be_filtered() {
		$password = 'password';

		$filter_count_before = did_filter( 'wp_hash_password_algorithm' );

		$wp_hash = wp_hash_password( $password );

		wp_check_password( $password, $wp_hash );
		wp_password_needs_rehash( $wp_hash );

		$this->assertSame( $filter_count_before + 2, did_filter( 'wp_hash_password_algorithm' ) );
	}

	/**
	 * @ticket 21022
	 */
	public function test_password_hashing_options_can_be_filtered() {
		$password = 'password';

		add_filter(
			'wp_hash_password_options',
			static function ( $options ) {
				$options['cost'] = 5;
				return $options;
			}
		);

		$filter_count_before = did_filter( 'wp_hash_password_options' );

		$wp_hash      = wp_hash_password( $password );
		$valid        = wp_check_password( $password, $wp_hash );
		$needs_rehash = wp_password_needs_rehash( $wp_hash );
		$info         = password_get_info( substr( $wp_hash, 3 ) );
		$cost         = $info['options']['cost'];

		$this->assertTrue( $valid );
		$this->assertFalse( $needs_rehash );
		$this->assertSame( $filter_count_before + 2, did_filter( 'wp_hash_password_options' ) );
		$this->assertSame( 5, $cost );
	}

	/**
	 * @ticket 21022
	 */
	public function test_password_checks_support_wp_hasher_fallback() {
		global $wp_hasher;

		$filter_count_before = did_filter( 'wp_hash_password_options' );

		$password = 'password';

		// Ensure the global $wp_hasher is set.
		$wp_hasher = new WP_Fake_Hasher();

		$hasher_hash  = $wp_hasher->HashPassword( $password );
		$wp_hash      = wp_hash_password( $password );
		$valid        = wp_check_password( $password, $wp_hash );
		$needs_rehash = wp_password_needs_rehash( $wp_hash );

		// Reset the global $wp_hasher.
		$wp_hasher = null;

		$this->assertSame( $hasher_hash, $wp_hash );
		$this->assertTrue( $valid );
		$this->assertFalse( $needs_rehash );
		$this->assertSame( 1, did_filter( 'check_password' ) );
		$this->assertSame( $filter_count_before, did_filter( 'wp_hash_password_options' ) );
	}

	public function reduce_hash_cost( array $options ): array {
		$options['cost'] = self::get_default_bcrypt_cost() - 1;
		return $options;
	}

	public function increase_hash_cost( array $options ): array {
		$options['cost'] = self::get_default_bcrypt_cost() + 1;
		return $options;
	}

	private static function get_default_bcrypt_cost(): int {
		return 5;
	}
}
