<?php
/**
 * The tests of tests/phpunit/tests/media.php (wordpress-develop trunk ce78d2d6eb) that call wp_read_audio_metadata(),
 * wp_read_video_metadata() and wp_get_media_creation_timestamp(), copied unchanged into a class of their own: the rest
 * of that file needs a database and the whole of WordPress.
 *
 * @group media
 */
class Tests_Media_Metadata extends WP_UnitTestCase {

	/**
	 * @ticket 35218
	 */
	public function test_wp_get_media_creation_timestamp_video_asf() {
		$metadata = array(
			'fileformat' => 'asf',
			'asf'        => array(
				'file_properties_object' => array(
					'creation_date_unix' => 123,
				),
			),
		);

		$this->assertSame( 123, wp_get_media_creation_timestamp( $metadata ) );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_get_media_creation_timestamp_video_matroska() {
		$metadata = array(
			'fileformat' => 'matroska',
			'matroska'   => array(
				'comments' => array(
					'creation_time' => array(
						'2015-12-24T17:40:09Z',
					),
				),
			),
		);

		$this->assertSame( 1450978809, wp_get_media_creation_timestamp( $metadata ) );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_get_media_creation_timestamp_video_quicktime() {
		$metadata = array(
			'fileformat' => 'quicktime',
			'quicktime'  => array(
				'moov' => array(
					'subatoms' => array(
						array(
							'creation_time_unix' => 1450978805,
						),
					),
				),
			),
		);

		$this->assertSame( 1450978805, wp_get_media_creation_timestamp( $metadata ) );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_get_media_creation_timestamp_video_webm() {
		$metadata = array(
			'fileformat' => 'webm',
			'matroska'   => array(
				'info' => array(
					array(
						'DateUTC_unix' => 1265680539,
					),
				),
			),
		);

		$this->assertSame( 1265680539, wp_get_media_creation_timestamp( $metadata ) );
	}

	/**
	 * Test created timestamp is properly read from an MP4 file.
	 *
	 * This MP4 video file has an AAC audio track, so it can be used to test
	 *`wp_read_audio_metadata()`.
	 *
	 * @ticket 42017
	 */
	public function test_wp_read_audio_metadata_adds_creation_date_with_mp4() {
		$video    = DIR_TESTDATA . '/uploads/small-video.mp4';
		$metadata = wp_read_audio_metadata( $video );

		$this->assertSame( 1269120551, $metadata['created_timestamp'] );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_read_video_metadata_adds_creation_date_with_quicktime() {
		$video    = DIR_TESTDATA . '/uploads/small-video.mov';
		$metadata = wp_read_video_metadata( $video );

		$this->assertSame( 1269120551, $metadata['created_timestamp'] );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_read_video_metadata_adds_creation_date_with_mp4() {
		$video    = DIR_TESTDATA . '/uploads/small-video.mp4';
		$metadata = wp_read_video_metadata( $video );

		$this->assertSame( 1269120551, $metadata['created_timestamp'] );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_read_video_metadata_adds_creation_date_with_mkv() {
		$video    = DIR_TESTDATA . '/uploads/small-video.mkv';
		$metadata = wp_read_video_metadata( $video );

		$this->assertSame( 1269120551, $metadata['created_timestamp'] );
	}

	/**
	 * @ticket 35218
	 */
	public function test_wp_read_video_metadata_adds_creation_date_with_webm() {
		$video    = DIR_TESTDATA . '/uploads/small-video.webm';
		$metadata = wp_read_video_metadata( $video );

		$this->assertSame( 1269120551, $metadata['created_timestamp'] );
	}

}
