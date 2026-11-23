// Evaluated without an opening tag. No strict_types: WordPress's own files are not strict, so arguments coerce as they
// do there.

namespace SpeedySpec\WpMedia
{
    /**
     * Reads a file's audio or video metadata natively, as wp_read_audio_metadata() and wp_read_video_metadata() do.
     *
     *  A path through a stream wrapper (an offload plugin's s3://) is copied into a temporary file first, at most the
     *  reader's byte limit; a larger file is read as unreadable, as the native reader would treat it.
     *
     * @param string $kind
     *   "audio" or "video".
     * @param string $file
     *   The path WordPress was given.
     * @return array|false|mixed
     *   What the filter returned, or false when the file doesn't exist.
     */
    function read($kind, $file)
    {
        if (!file_exists($file)) {
            return false;
        }
        $file = (string) $file;
        if (!preg_match('#^[a-z][a-z0-9+.-]*://#i', $file) || stripos($file, 'file://') === 0) {
            // The native reader resolves paths against the process, not PHP's working directory: hand it the
            // canonical local path, as PHP resolved it.
            $local = stripos($file, 'file://') === 0 ? substr($file, 7) : $file;
            $real = realpath($local);
            return Native\read_metadata($kind, $real === false ? $local : $real, $file, false);
        }
        $max = Native\max_bytes();
        $temp = tempnam(sys_get_temp_dir(), 'speedyspec-media-');
        $in = @fopen($file, 'rb');
        $out = $temp === false ? false : fopen($temp, 'wb');
        $copied = ($in && $out) ? stream_copy_to_stream($in, $out, $max + 1) : false;
        if ($in) {
            fclose($in);
        }
        if ($out) {
            fclose($out);
        }
        try {
            $too_large = $copied === false || $copied > $max;
            return Native\read_metadata($kind, $temp, $file, $too_large);
        } finally {
            if ($temp !== false) {
                @unlink($temp);
            }
        }
    }
}

namespace SpeedySpec\WpMedia
{
    /**
     * Reads a file's first bytes the way PHP would open it, stream wrappers included; an empty string when it can't.
     *
     * @param string $file
     *   The path WordPress was given.
     * @param int $length
     *   How many bytes.
     * @return string
     *   The bytes read.
     */
    function head($file, $length)
    {
        $bytes = @file_get_contents((string) $file, false, null, 0, $length);
        return $bytes === false ? '' : $bytes;
    }

    /**
     * Gives the canonical local path the native reader can open, or null for a stream-wrapper path.
     *
     * @param string $file
     *   The path WordPress was given.
     * @return string|null
     *   The local path.
     */
    function local($file)
    {
        $file = (string) $file;
        if (preg_match('#^[a-z][a-z0-9+.-]*://#i', $file) && stripos($file, 'file://') !== 0) {
            return null;
        }
        $local = stripos($file, 'file://') === 0 ? substr($file, 7) : $file;
        $real = realpath($local);
        return $real === false ? null : $real;
    }
}

namespace
{
    if (!function_exists('wp_get_image_mime')) {
        function wp_get_image_mime($file)
        {
            $type = \SpeedySpec\WpMedia\Native\image_type(\SpeedySpec\WpMedia\head($file, 64));
            return $type ? $type['mime'] : false;
        }
    }

    if (!function_exists('wp_getimagesize')) {
        function wp_getimagesize($filename, ?array &$image_info = null)
        {
            $type = \SpeedySpec\WpMedia\Native\image_type(\SpeedySpec\WpMedia\head($filename, 64));
            // PHP's own getimagesize() for every type it reads correctly, and for anything not sniffed: its answer
            // (and $image_info) are then exactly WordPress's.
            if (!$type || $type['fast']) {
                $debug = defined('WP_DEBUG') && WP_DEBUG && !defined('WP_RUN_CORE_TESTS');
                if (2 === func_num_args()) {
                    $info = $debug ? getimagesize($filename, $image_info) : @getimagesize($filename, $image_info);
                } else {
                    $info = $debug ? getimagesize($filename) : @getimagesize($filename);
                }
                if (!empty($info) && !(empty($info[0]) && empty($info[1]))) {
                    return $info;
                }
                // WordPress falls back only for WebP, AVIF and HEIC (its own parsers or Imagick); every other type
                // PHP couldn't size is false, without a probe child.
                if (!$type || !in_array($type['mime'], array('image/webp', 'image/avif', 'image/heif'), true)) {
                    return false;
                }
            } elseif (2 === func_num_args()) {
                // WordPress's getimagesize() call always resets $image_info; a reader-path type has no segments.
                $image_info = array();
            }
            $local = \SpeedySpec\WpMedia\local($filename);
            if ($local === null) {
                return false;
            }
            return \SpeedySpec\WpMedia\Native\read_image_size($local, $type['imagetype'], $type['mime']);
        }
    }

    if (!function_exists('wp_get_webp_info')) {
        function wp_get_webp_info($filename)
        {
            if ('image/webp' !== wp_get_image_mime($filename)) {
                return array('width' => false, 'height' => false, 'type' => false);
            }
            return \SpeedySpec\WpMedia\Native\webp_info(\SpeedySpec\WpMedia\head($filename, 40));
        }
    }

    if (!function_exists('wp_get_avif_info')) {
        function wp_get_avif_info($filename)
        {
            $none = array('width' => false, 'height' => false, 'bit_depth' => false, 'num_channels' => false);
            if ('image/avif' !== wp_get_image_mime($filename)) {
                return $none;
            }
            // PHP 8.5's getimagesize() reads AVIF, bit depth and channel count included: WordPress's numbers exactly.
            $size = @getimagesize($filename);
            if (!empty($size[0]) && !empty($size[1])) {
                return array(
                    'width'        => $size[0],
                    'height'       => $size[1],
                    'bit_depth'    => $size['bits'] ?? false,
                    'num_channels' => $size['channels'] ?? false,
                );
            }
            $local = \SpeedySpec\WpMedia\local($filename);
            return $local === null ? $none : \SpeedySpec\WpMedia\Native\avif_info($local);
        }
    }

    if (!function_exists('wp_read_audio_metadata')) {
        function wp_read_audio_metadata($file)
        {
            return \SpeedySpec\WpMedia\read('audio', $file);
        }
    }

    if (!function_exists('wp_read_video_metadata')) {
        function wp_read_video_metadata($file)
        {
            return \SpeedySpec\WpMedia\read('video', $file);
        }
    }

    if (!function_exists('wp_add_id3_tag_data')) {
        function wp_add_id3_tag_data(&$metadata, $data)
        {
            // Only the keys WordPress's function sets are assigned, in its order, so the rest of $metadata (objects
            // included) is untouched, and a $metadata nothing is copied into stays as it was.
            foreach (\SpeedySpec\WpMedia\Native\id3_tag_changes($data) as $key => $value) {
                $metadata[$key] = $value;
            }
        }
    }

    if (!function_exists('wp_get_media_creation_timestamp')) {
        function wp_get_media_creation_timestamp($metadata)
        {
            return \SpeedySpec\WpMedia\Native\wp_get_media_creation_timestamp($metadata);
        }
    }

    if (!function_exists('wp_get_mime_types')) {
        function wp_get_mime_types()
        {
            // WordPress's list with every format the media reader reads added after its kind's rows.
            return apply_filters('mime_types', \SpeedySpec\WpMedia\Native\mime_types());
        }
    }

    if (!function_exists('wp_get_ext_types')) {
        function wp_get_ext_types()
        {
            return apply_filters('ext2type', \SpeedySpec\WpMedia\Native\ext_types());
        }
    }

    // The small algorithms over these lists are WordPress's own: plugins filter the lists and add keys with regex
    // syntax that only PCRE reads exactly, and the capability check is PHP's.
    if (!function_exists('get_allowed_mime_types')) {
        function get_allowed_mime_types($user = null)
        {
            $t = wp_get_mime_types();

            unset($t['swf'], $t['exe']);
            if (function_exists('current_user_can')) {
                $unfiltered = $user ? user_can($user, 'unfiltered_html') : current_user_can('unfiltered_html');
            }

            if (empty($unfiltered)) {
                unset($t['htm|html'], $t['js']);
            }

            return apply_filters('upload_mimes', $t, $user);
        }
    }

    if (!function_exists('wp_ext2type')) {
        function wp_ext2type($ext)
        {
            $ext = strtolower($ext);

            $ext2type = wp_get_ext_types();
            foreach ($ext2type as $type => $exts) {
                if (in_array($ext, $exts, true)) {
                    return $type;
                }
            }
            return null;
        }
    }

    if (!function_exists('wp_get_default_extension_for_mime_type')) {
        function wp_get_default_extension_for_mime_type($mime_type)
        {
            $extensions = explode('|', array_search($mime_type, wp_get_mime_types(), true));

            if (empty($extensions[0])) {
                return false;
            }

            return $extensions[0];
        }
    }

    if (!function_exists('wp_check_filetype')) {
        function wp_check_filetype($filename, $mimes = null)
        {
            if (empty($mimes)) {
                $mimes = get_allowed_mime_types();
            }
            $type = false;
            $ext  = false;

            foreach ($mimes as $ext_preg => $mime_match) {
                $ext_preg = '!\.(' . $ext_preg . ')$!i';
                if (preg_match($ext_preg, $filename, $ext_matches)) {
                    $type = $mime_match;
                    $ext  = $ext_matches[1];
                    break;
                }
            }

            return compact('ext', 'type');
        }
    }

    if (!function_exists('wp_check_filetype_and_ext')) {
        function wp_check_filetype_and_ext($file, $filename, $mimes = null)
        {
            // WordPress 7.1.5's function, with two changes: the getimagesize_mimes_to_exts default names the image
            // types the media reader adds, and a local file's content type comes from Native\content_mime(), which
            // is fileinfo's answer unless fileinfo files audio as video (or video as audio) and the reader confirms
            // the claimed kind.
            $proper_filename = false;

            $wp_filetype = wp_check_filetype($filename, $mimes);
            $ext         = $wp_filetype['ext'];
            $type        = $wp_filetype['type'];

            if (!file_exists($file)) {
                return compact('ext', 'type', 'proper_filename');
            }

            $real_mime = false;

            if ($type && str_starts_with($type, 'image/')) {
                $real_mime = wp_get_image_mime($file);

                $heic_images_extensions = array(
                    'heif',
                    'heics',
                    'heifs',
                );

                if ($real_mime && ($real_mime !== $type || in_array($ext, $heic_images_extensions, true))) {
                    $mime_to_ext = apply_filters(
                        'getimagesize_mimes_to_exts',
                        \SpeedySpec\WpMedia\Native\image_mimes_to_exts()
                    );

                    if (!empty($mime_to_ext[$real_mime])) {
                        $filename_parts = explode('.', $filename);

                        array_pop($filename_parts);
                        $filename_parts[] = $mime_to_ext[$real_mime];
                        $new_filename     = implode('.', $filename_parts);

                        if ($new_filename !== $filename) {
                            $proper_filename = $new_filename;
                        }

                        $wp_filetype = wp_check_filetype($new_filename, $mimes);
                        $ext         = $wp_filetype['ext'];
                        $type        = $wp_filetype['type'];
                    } else {
                        $real_mime = false;
                    }
                }
            }

            if ($type && !$real_mime && extension_loaded('fileinfo')) {
                $local = \SpeedySpec\WpMedia\local($file);
                if ($local !== null) {
                    $real_mime = \SpeedySpec\WpMedia\Native\content_mime($local, $type);
                } else {
                    $finfo     = finfo_open(FILEINFO_MIME_TYPE);
                    $real_mime = finfo_file($finfo, $file);
                }

                $google_docs_types = array(
                    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
                    'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
                );

                foreach ($google_docs_types as $google_docs_type) {
                    // Google Docs exports some files with the MIME type doubled.
                    if (2 === substr_count($real_mime, $google_docs_type)) {
                        $real_mime = $google_docs_type;
                    }
                }

                $nonspecific_types = array(
                    'application/octet-stream',
                    'application/encrypted',
                    'application/CDFV2-encrypted',
                    'application/zip',
                );

                if (in_array($real_mime, $nonspecific_types, true)) {
                    if (!in_array(substr($type, 0, strcspn($type, '/')), array('application', 'video', 'audio'), true)) {
                        $type = false;
                        $ext  = false;
                    }
                } elseif (str_starts_with($real_mime, 'video/') || str_starts_with($real_mime, 'audio/')) {
                    if (substr($real_mime, 0, strcspn($real_mime, '/')) !== substr($type, 0, strcspn($type, '/'))) {
                        $type = false;
                        $ext  = false;
                    }
                } elseif ('text/plain' === $real_mime) {
                    if (!in_array(
                        $type,
                        array(
                            'text/plain',
                            'text/csv',
                            'application/csv',
                            'text/richtext',
                            'text/tsv',
                            'text/vtt',
                        ),
                        true
                    )
                    ) {
                        $type = false;
                        $ext  = false;
                    }
                } elseif ('application/csv' === $real_mime) {
                    if (!in_array(
                        $type,
                        array(
                            'text/csv',
                            'text/plain',
                            'application/csv',
                        ),
                        true
                    )
                    ) {
                        $type = false;
                        $ext  = false;
                    }
                } elseif ('text/rtf' === $real_mime) {
                    if (!in_array(
                        $type,
                        array(
                            'text/rtf',
                            'text/plain',
                            'application/rtf',
                        ),
                        true
                    )
                    ) {
                        $type = false;
                        $ext  = false;
                    }
                } else {
                    if ($type !== $real_mime) {
                        $type = false;
                        $ext  = false;
                    }
                }
            }

            if ($type) {
                $allowed = get_allowed_mime_types();

                if (!in_array($type, $allowed, true)) {
                    $type = false;
                    $ext  = false;
                }
            }

            return apply_filters('wp_check_filetype_and_ext', compact('ext', 'type', 'proper_filename'), $file, $filename, $mimes, $real_mime);
        }
    }
}
