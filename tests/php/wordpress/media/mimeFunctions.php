<?php
/**
 * The MIME list and filetype tests of tests/phpunit/tests/functions.php (wordpress-develop trunk ce78d2d6eb), copied
 * unchanged into a class of their own: the rest of that file needs the whole of WordPress.
 *
 * @group functions
 */
class Tests_Functions_MimeFunctions extends WP_UnitTestCase {

	/**
	 * @ticket 21594
	 */
	public function test_get_allowed_mime_types() {
		$mimes = get_allowed_mime_types();

		$this->assertIsArray( $mimes );
		$this->assertNotEmpty( $mimes );

		add_filter( 'upload_mimes', '__return_empty_array' );
		$mimes = get_allowed_mime_types();
		$this->assertIsArray( $mimes );
		$this->assertEmpty( $mimes );

		remove_filter( 'upload_mimes', '__return_empty_array' );
		$mimes = get_allowed_mime_types();
		$this->assertIsArray( $mimes );
		$this->assertNotEmpty( $mimes );
	}

	/**
	 * @ticket 21594
	 */
	public function test_wp_get_mime_types() {
		$mimes = wp_get_mime_types();

		$this->assertIsArray( $mimes );
		$this->assertNotEmpty( $mimes );

		add_filter( 'mime_types', '__return_empty_array' );
		$mimes = wp_get_mime_types();
		$this->assertIsArray( $mimes );
		$this->assertEmpty( $mimes );

		remove_filter( 'mime_types', '__return_empty_array' );
		$mimes = wp_get_mime_types();
		$this->assertIsArray( $mimes );
		$this->assertNotEmpty( $mimes );

		// 'upload_mimes' should not affect wp_get_mime_types().
		add_filter( 'upload_mimes', '__return_empty_array' );
		$mimes = wp_get_mime_types();
		$this->assertIsArray( $mimes );
		$this->assertNotEmpty( $mimes );

		remove_filter( 'upload_mimes', '__return_empty_array' );
		$mimes2 = wp_get_mime_types();
		$this->assertIsArray( $mimes2 );
		$this->assertNotEmpty( $mimes2 );
		$this->assertSame( $mimes2, $mimes );
	}

	/**
	 * @ticket 35987
	 * @ticket 65819
	 * @covers ::wp_ext2type
	 */
	public function test_wp_ext2type() {
		$extensions = wp_get_ext_types();

		$this->assertNotEmpty( $extensions );

		// Literal mappings stay independent of the current registry contents.
		$this->assertSame( 'image', wp_ext2type( 'jpg' ) );
		$this->assertSame( 'image', wp_ext2type( 'PNG' ) );
		$this->assertSame( 'audio', wp_ext2type( 'mp3' ) );
		$this->assertSame( 'video', wp_ext2type( 'mp4' ) );
		$this->assertSame( 'document', wp_ext2type( 'pdf' ) );

		foreach ( $extensions as $type => $extension_list ) {
			foreach ( $extension_list as $extension ) {
				$this->assertSame( $type, wp_ext2type( $extension ) );
				$this->assertSame( $type, wp_ext2type( strtoupper( $extension ) ) );
			}
		}

		$this->assertNull( wp_ext2type( 'unknown_format' ) );
	}

	/**
	 * @ticket 39550
	 * @dataProvider data_wp_check_filetype_and_ext
	 * @requires extension fileinfo
	 */
	public function test_wp_check_filetype_and_ext( $file, $filename, $expected ) {
		$this->assertSame( $expected, wp_check_filetype_and_ext( $file, $filename ) );
	}

	public static function data_wp_check_filetype_and_ext() {
		$data = array(
			// Standard image.
			array(
				DIR_TESTDATA . '/images/canola.jpg',
				'canola.jpg',
				array(
					'ext'             => 'jpg',
					'type'            => 'image/jpeg',
					'proper_filename' => false,
				),
			),
			// Image with wrong extension.
			array(
				DIR_TESTDATA . '/images/test-image-mime-jpg.png',
				'test-image-mime-jpg.png',
				array(
					'ext'             => 'jpg',
					'type'            => 'image/jpeg',
					'proper_filename' => 'test-image-mime-jpg.jpg',
				),
			),
			// Image without extension.
			array(
				DIR_TESTDATA . '/images/test-image-no-extension',
				'test-image-no-extension',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Valid non-image file with an image extension.
			array(
				DIR_TESTDATA . '/formatting/big5.txt',
				'big5.jpg',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Non-image file not allowed.
			array(
				DIR_TESTDATA . '/export/crazy-cdata.xml',
				'crazy-cdata.xml',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Non-image file not allowed even if it's named like one.
			array(
				DIR_TESTDATA . '/export/crazy-cdata.xml',
				'crazy-cdata.jpg',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Non-image file not allowed if it's named like something else.
			array(
				DIR_TESTDATA . '/export/crazy-cdata.xml',
				'crazy-cdata.doc',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Non-image file not allowed even if it's named like one.
			array(
				DIR_TESTDATA . '/export/crazy-cdata.xml',
				'crazy-cdata.jpg',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
			// Non-image file not allowed if it's named like something else.
			array(
				DIR_TESTDATA . '/export/crazy-cdata.xml',
				'crazy-cdata.doc',
				array(
					'ext'             => false,
					'type'            => false,
					'proper_filename' => false,
				),
			),
		);

		// Test a few additional file types on single sites.
		if ( ! is_multisite() ) {
			$data = array_merge(
				$data,
				array(
					// Standard non-image file.
					array(
						DIR_TESTDATA . '/formatting/big5.txt',
						'big5.txt',
						array(
							'ext'             => 'txt',
							'type'            => 'text/plain',
							'proper_filename' => false,
						),
					),
					// Google Docs file for which finfo_file() returns a duplicate mime type.
					array(
						DIR_TESTDATA . '/uploads/double-mime-type.docx',
						'double-mime-type.docx',
						array(
							'ext'             => 'docx',
							'type'            => 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
							'proper_filename' => false,
						),
					),
					// Non-image file with wrong sub-type.
					array(
						DIR_TESTDATA . '/uploads/pages-to-word.docx',
						'pages-to-word.docx',
						array(
							'ext'             => 'docx',
							'type'            => 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
							'proper_filename' => false,
						),
					),
					// FLAC file.
					array(
						DIR_TESTDATA . '/uploads/small-audio.flac',
						'small-audio.flac',
						array(
							'ext'             => 'flac',
							'type'            => 'audio/flac',
							'proper_filename' => false,
						),
					),
					/*
					 * Audio wrapped in an MP4/ISO-BMFF container (e.g. .m4a from a phone
					 * recorder) is detected by fileinfo as video/mp4, while the extension
					 * maps to audio/mpeg. The differing major types must not cause a rejection.
					 */
					array(
						DIR_TESTDATA . '/uploads/audio-in-video-container.m4a',
						'audio-in-video-container.m4a',
						array(
							'ext'             => 'm4a',
							'type'            => 'audio/mpeg',
							'proper_filename' => false,
						),
					),
					/*
					 * Audio wrapped in an 3GPP TS 26.244 container (e.g. .m4a from a phone
					 * recorder) is detected by fileinfo as video/3gpp, while the extension
					 * maps to audio/mpeg. The differing major types must not cause a rejection.
					 */
					array(
						DIR_TESTDATA . '/uploads/audio-only-3gp4.m4a',
						'audio-only-3gp4.m4a',
						array(
							'ext'             => 'm4a',
							'type'            => 'audio/mpeg',
							'proper_filename' => false,
						),
					),
					// Assorted text/* sample files
					array(
						DIR_TESTDATA . '/uploads/test.vtt',
						'test.vtt',
						array(
							'ext'             => 'vtt',
							'type'            => 'text/vtt',
							'proper_filename' => false,
						),
					),
					array(
						DIR_TESTDATA . '/uploads/test.csv',
						'test.csv',
						array(
							'ext'             => 'csv',
							'type'            => 'text/csv',
							'proper_filename' => false,
						),
					),
					// RTF files.
					array(
						DIR_TESTDATA . '/uploads/test.rtf',
						'test.rtf',
						array(
							'ext'             => 'rtf',
							'type'            => 'application/rtf',
							'proper_filename' => false,
						),
					),
				)
			);
		}

		return $data;
	}

	/**
	 * @ticket 39550
	 * @group ms-excluded
	 * @requires extension fileinfo
	 */
	public function test_wp_check_filetype_and_ext_with_filtered_svg() {
		$file     = DIR_TESTDATA . '/uploads/video-play.svg';
		$filename = 'video-play.svg';

		$expected = array(
			'ext'             => 'svg',
			'type'            => 'image/svg+xml',
			'proper_filename' => false,
		);

		add_filter(
			'upload_mimes',
			static function ( $mimes ) {
				$mimes['svg'] = 'image/svg+xml';
				return $mimes;
			}
		);

		$this->assertSame( $expected, wp_check_filetype_and_ext( $file, $filename ) );
	}

	/**
	 * @ticket 39550
	 * @group ms-excluded
	 * @requires extension fileinfo
	 */
	public function test_wp_check_filetype_and_ext_with_filtered_woff() {
		$file     = DIR_TESTDATA . '/uploads/dashicons.woff';
		$filename = 'dashicons.woff';

		$woff_mime_type = 'application/font-woff';

		/*
		 * As of PHP 8.1.12, which includes libmagic/file update to version 5.42,
		 * the expected mime type for WOFF files is 'font/woff'.
		 *
		 * See https://github.com/php/php-src/issues/8805.
		 */
		if ( PHP_VERSION_ID >= 80112 ) {
			$woff_mime_type = 'font/woff';
		}

		$expected = array(
			'ext'             => 'woff',
			'type'            => $woff_mime_type,
			'proper_filename' => false,
		);

		add_filter(
			'upload_mimes',
			static function ( $mimes ) use ( $woff_mime_type ) {
				$mimes['woff'] = $woff_mime_type;
				return $mimes;
			}
		);

		$this->assertSame( $expected, wp_check_filetype_and_ext( $file, $filename ) );
	}

	/**
	 * Tests that TrueType fonts are accepted whether font/ttf or font/sfnt is expected.
	 *
	 * libmagic 5.47 changed the type reported for TrueType fonts from font/sfnt to font/ttf.
	 * Expecting each type in turn ensures that whichever one libmagic reports, a file is also
	 * accepted when the other is expected.
	 *
	 * @ticket 66419
	 * @group ms-excluded
	 * @requires extension fileinfo
	 *
	 * @dataProvider data_wp_check_filetype_and_ext_with_ttf_mime_types
	 *
	 * @covers ::wp_check_filetype_and_ext
	 *
	 * @param string $mime_type The mime type expected for TTF files.
	 */
	public function test_wp_check_filetype_and_ext_accepts_either_ttf_mime_type( $mime_type ) {
		$file     = DIR_TESTDATA . '/fonts/OpenSans-Regular.ttf';
		$filename = 'OpenSans-Regular.ttf';

		$filter = static function ( $mimes ) use ( $mime_type ) {
			$mimes['ttf'] = $mime_type;
			return $mimes;
		};
		add_filter( 'upload_mimes', $filter );

		$expected = array(
			'ext'             => 'ttf',
			'type'            => $mime_type,
			'proper_filename' => false,
		);

		$this->assertSame( $expected, wp_check_filetype_and_ext( $file, $filename ) );

		remove_filter( 'upload_mimes', $filter );
	}

	/**
	 * Data provider.
	 *
	 * @return array[]
	 */
	public static function data_wp_check_filetype_and_ext_with_ttf_mime_types() {
		return array(
			'font/ttf'  => array( 'font/ttf' ),
			'font/sfnt' => array( 'font/sfnt' ),
		);
	}

	/**
	 * Tests that a font file of another format is rejected when named with a .ttf extension.
	 *
	 * @ticket 66419
	 * @group ms-excluded
	 * @requires extension fileinfo
	 *
	 * @covers ::wp_check_filetype_and_ext
	 */
	public function test_wp_check_filetype_and_ext_rejects_other_font_formats_as_ttf() {
		$file     = DIR_TESTDATA . '/fonts/OpenSans-Regular.woff';
		$filename = 'OpenSans-Regular.ttf';

		$filter = static function ( $mimes ) {
			$mimes['ttf'] = 'font/ttf';
			return $mimes;
		};
		add_filter( 'upload_mimes', $filter );

		$expected = array(
			'ext'             => false,
			'type'            => false,
			'proper_filename' => false,
		);

		$this->assertSame( $expected, wp_check_filetype_and_ext( $file, $filename ) );

		remove_filter( 'upload_mimes', $filter );
	}

	/**
	 * @ticket 53668
	 * @ticket 65819
	 * @covers ::wp_get_default_extension_for_mime_type
	 */
	public function test_wp_get_default_extension_for_mime_type() {
		$this->assertSame( 'jpg', wp_get_default_extension_for_mime_type( 'image/jpeg' ), 'jpg not returned as default extension for "image/jpeg"' );
		$this->assertNotSame( 'jpeg', wp_get_default_extension_for_mime_type( 'image/jpeg' ), 'jpeg should not be returned as default extension for "image/jpeg"' );
		$this->assertSame( 'png', wp_get_default_extension_for_mime_type( 'image/png' ), 'png not returned as default extension for "image/png"' );
		$this->assertSame( 'pdf', wp_get_default_extension_for_mime_type( 'application/pdf' ), 'pdf not returned as default extension for "application/pdf"' );
		$this->assertSame( 'mp3', wp_get_default_extension_for_mime_type( 'audio/mpeg' ), 'mp3 not returned as default extension for "audio/mpeg"' );
		$this->assertFalse( wp_get_default_extension_for_mime_type( 'wibble/wobble' ), 'false not returned for unrecognized mime type' );
		$this->assertFalse( wp_get_default_extension_for_mime_type( '' ), 'false not returned when empty string as mime type supplied' );
		$this->assertFalse( wp_get_default_extension_for_mime_type( '   ' ), 'false not returned when empty string as mime type supplied' );
		$this->assertFalse( wp_get_default_extension_for_mime_type( 123 ), 'false not returned when int as mime type supplied' );
		$this->assertFalse( wp_get_default_extension_for_mime_type( null ), 'false not returned when null as mime type supplied' );
	}
}
