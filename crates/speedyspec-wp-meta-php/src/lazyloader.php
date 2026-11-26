declare(strict_types=1);

namespace
{
    use SpeedySpec\WpMeta\Native as N;

    /**
     * Lazy-loads object metadata, as WordPress 7.2-RC2's class does, over the native lazy loader.
     *
     *  Queued ids live in Rust, one loader per instance. The class hooks `lazyload_meta_callback` on the type's
     *  `get_{$type}_metadata` filter (10, 5) before the queued-objects action fires, and unhooks it when the queue is
     *  loaded or reset.
     */
    #[AllowDynamicProperties]
    class WP_Metadata_Lazyloader
    {
        /**
         * Lazy-loading settings by object type: the filter to hook and the callback to hook there.
         *
         * @var array<string, array{filter: string, callback: callable}>
         */
        protected $settings = array();

        /**
         * The number of this instance's native loader.
         *
         * @var int
         */
        private $speedyspec_loader;

        /**
         * Sets up the lazy-loaded types: term, comment and blog.
         */
        public function __construct()
        {
            $this->settings = array(
                'term' => array(
                    'filter' => 'get_term_metadata',
                    'callback' => array($this, 'lazyload_meta_callback'),
                ),
                'comment' => array(
                    'filter' => 'get_comment_metadata',
                    'callback' => array($this, 'lazyload_meta_callback'),
                ),
                'blog' => array(
                    'filter' => 'get_blog_metadata',
                    'callback' => array($this, 'lazyload_meta_callback'),
                ),
            );
            $this->speedyspec_loader = N\lazy_new();
        }

        /**
         * Adds objects to the metadata lazy-load queue.
         *
         * @return void|WP_Error
         *   A `WP_Error` (`invalid_object_type`) when the type cannot be lazy-loaded.
         */
        public function queue_objects($object_type, $object_ids)
        {
            if (!isset($this->settings[$object_type]))
            {
                return new WP_Error('invalid_object_type', __('Invalid object type.'));
            }
            $type_settings = $this->settings[$object_type];
            add_filter($type_settings['filter'], $type_settings['callback'], 10, 5);
            N\lazy_queue($this->speedyspec_loader, $object_type, $object_ids, $this);
        }

        /**
         * Resets the lazy-load queue for a given object type and unhooks its callback.
         *
         * @return void|WP_Error
         *   A `WP_Error` (`invalid_object_type`) when the type cannot be lazy-loaded.
         */
        public function reset_queue($object_type)
        {
            if (!isset($this->settings[$object_type]))
            {
                return new WP_Error('invalid_object_type', __('Invalid object type.'));
            }
            $type_settings = $this->settings[$object_type];
            N\lazy_reset($this->speedyspec_loader, $object_type);
            remove_filter($type_settings['filter'], $type_settings['callback']);
        }

        /**
         * Lazy-loads term meta for queued terms.
         *
         * @deprecated 6.3.0 Use WP_Metadata_Lazyloader::lazyload_meta_callback() instead.
         *
         * @return mixed
         *   `$check`, unchanged.
         */
        public function lazyload_term_meta($check)
        {
            _deprecated_function(__METHOD__, '6.3.0', 'WP_Metadata_Lazyloader::lazyload_meta_callback');
            return $this->lazyload_meta_callback($check, 0, '', false, 'term');
        }

        /**
         * Lazy-loads comment meta for queued comments.
         *
         * @deprecated 6.3.0 Use WP_Metadata_Lazyloader::lazyload_meta_callback() instead.
         *
         * @return mixed
         *   `$check`, unchanged.
         */
        public function lazyload_comment_meta($check)
        {
            _deprecated_function(__METHOD__, '6.3.0', 'WP_Metadata_Lazyloader::lazyload_meta_callback');
            return $this->lazyload_meta_callback($check, 0, '', false, 'comment');
        }

        /**
         * Lazy-loads meta for queued objects on the first read of the type: every queued id, plus `$object_id`
         *  when it is not queued, then resets the queue.
         *
         * @return mixed
         *   `$check`, unchanged.
         */
        public function lazyload_meta_callback($check, $object_id, $meta_key, $single, $meta_type)
        {
            if (!N\lazy_pending($this->speedyspec_loader, $meta_type))
            {
                return $check;
            }
            N\lazy_load($this->speedyspec_loader, $meta_type, $object_id);
            // No need to run again for this set of objects.
            $this->reset_queue($meta_type);
            return $check;
        }
    }
}
