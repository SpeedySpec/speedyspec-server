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
            $type = \SpeedySpec\WpMedia\Native\image_type(\SpeedySpec\WpMedia\head($file, 32));
            return $type ? $type['mime'] : false;
        }
    }

    if (!function_exists('wp_getimagesize')) {
        function wp_getimagesize($filename, ?array &$image_info = null)
        {
            $type = \SpeedySpec\WpMedia\Native\image_type(\SpeedySpec\WpMedia\head($filename, 32));
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
                if (!$type) {
                    return false;
                }
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
}
