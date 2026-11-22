<?php
/**
 * The wp_get_webp_info() and wp_get_avif_info() tests of tests/phpunit/tests/image/editor.php (wordpress-develop
 * trunk ce78d2d6eb), copied unchanged into a class of their own: the rest of that file tests the image editors.
 *
 * @group image
 */
class Tests_Image_WebpAvifInfo extends WP_UnitTestCase {

	/**
	 * Test wp_get_webp_info.
	 *
	 * @ticket 35725
	 * @dataProvider data_wp_get_webp_info
	 *
	 */
	public function test_wp_get_webp_info( $file, $expected ) {
		$file_data = wp_get_webp_info( $file );
		$this->assertSame( $expected, $file_data );
	}

	/**
	 * Data provider for test_wp_get_webp_info().
	 */
	public static function data_wp_get_webp_info() {
		return array(
			// Standard JPEG.
			array(
				DIR_TESTDATA . '/images/test-image.jpg',
				array(
					'width'  => false,
					'height' => false,
					'type'   => false,
				),
			),
			// Standard GIF.
			array(
				DIR_TESTDATA . '/images/test-image.gif',
				array(
					'width'  => false,
					'height' => false,
					'type'   => false,
				),
			),
			// Animated WebP.
			array(
				DIR_TESTDATA . '/images/webp-animated.webp',
				array(
					'width'  => 100,
					'height' => 100,
					'type'   => 'animated-alpha',
				),
			),
			// Lossless WebP.
			array(
				DIR_TESTDATA . '/images/webp-lossless.webp',
				array(
					'width'  => 1200,
					'height' => 675,
					'type'   => 'lossless',
				),
			),
			// Lossy WebP.
			array(
				DIR_TESTDATA . '/images/webp-lossy.webp',
				array(
					'width'  => 1200,
					'height' => 675,
					'type'   => 'lossy',
				),
			),
			// Transparent WebP.
			array(
				DIR_TESTDATA . '/images/webp-transparent.webp',
				array(
					'width'  => 1200,
					'height' => 675,
					'type'   => 'animated-alpha',
				),
			),
		);
	}

	/**
	 * Test wp_get_avif_info.
	 *
	 * @ticket 51228
	 *
	 * @dataProvider data_wp_get_avif_info
	 *
	 * @param string $file     The path to the AVIF file for testing.
	 * @param array  $expected The expected AVIF file information.
	 */
	public function test_wp_get_avif_info( $file, $expected ) {
		$file_data = wp_get_avif_info( $file );
		$this->assertSame( $expected, $file_data );
	}

	/**
	 * Data provider for test_wp_get_avif_info().
	 */
	public static function data_wp_get_avif_info() {
		return array(
			// Standard JPEG.
			array(
				DIR_TESTDATA . '/images/test-image.jpg',
				array(
					'width'        => false,
					'height'       => false,
					'bit_depth'    => false,
					'num_channels' => false,
				),
			),
			// Standard GIF.
			array(
				DIR_TESTDATA . '/images/test-image.gif',
				array(
					'width'        => false,
					'height'       => false,
					'bit_depth'    => false,
					'num_channels' => false,
				),
			),
			// Animated AVIF.
			array(
				DIR_TESTDATA . '/images/avif-animated.avif',
				array(
					'width'        => 150,
					'height'       => 150,
					'bit_depth'    => 8,
					'num_channels' => 4,
				),
			),
			// Lossless AVIF.
			array(
				DIR_TESTDATA . '/images/avif-lossless.avif',
				array(
					'width'        => 400,
					'height'       => 400,
					'bit_depth'    => 8,
					'num_channels' => 3,
				),
			),
			// Lossy AVIF.
			array(
				DIR_TESTDATA . '/images/avif-lossy.avif',
				array(
					'width'        => 400,
					'height'       => 400,
					'bit_depth'    => 8,
					'num_channels' => 3,
				),
			),
			// Transparent AVIF.
			array(
				DIR_TESTDATA . '/images/avif-transparent.avif',
				array(
					'width'        => 128,
					'height'       => 128,
					'bit_depth'    => 12,
					'num_channels' => 4,
				),
			),
			array(
				DIR_TESTDATA . '/images/color_grid_alpha_nogrid.avif',
				array(
					'width'        => 80,
					'height'       => 80,
					'bit_depth'    => 8,
					'num_channels' => 4,
				),
			),
			array(
				DIR_TESTDATA . '/images/avif-alpha-grid2x1.avif',
				array(
					'width'        => 199,
					'height'       => 200,
					'bit_depth'    => 8,
					'num_channels' => 4,
				),
			),
			array(
				DIR_TESTDATA . '/images/colors_hdr_p3.avif',
				array(
					'width'        => 200,
					'height'       => 200,
					'bit_depth'    => 10,
					'num_channels' => 3,
				),
			),
		);
	}
}
