<?php
/**
 * Phase 3's recorded calls: WP_Locale, locale switching, script translations, translate_settings_using_i18n_schema,
 * wp_get_list_item_separator() and wp_get_word_count_type(). Every hook these fire is logged with its arguments, so
 * the order of gettext calls and actions is recorded too.
 *
 * `$languages` is WP_LANG_DIR (tests/php/data/languages); `$data` holds the corpus's own fixtures.
 */

/**
 * Logs every call to `$hooks` into `$log` and passes the first argument through.
 *
 * @param list<string> $hooks
 *     The hooks to log.
 * @param array<int, mixed> $log
 *     Where each `[hook, args]` goes.
 * @return list<array{string, Closure}>
 *     What to hand to speedyspec_i18n_unlog().
 */
function speedyspec_i18n_log(array $hooks, array &$log): array {
    $added = array();
    foreach ($hooks as $hook) {
        $f = function (...$args) use ($hook, &$log) {
            $log[] = array($hook, speedyspec_i18n_portable($args));
            return $args[0] ?? null;
        };
        add_filter($hook, $f, 10, 10);
        $added[] = array($hook, $f);
    }
    return $added;
}

/**
 * The value with this checkout's path replaced by `{root}`, so recordings do not depend on where they were made.
 *
 * @param mixed $value
 *     Any value.
 * @return mixed
 *     The value, with every string under the checkout made relative.
 */
function speedyspec_i18n_portable($value) {
    if (is_string($value)) {
        return str_replace(dirname(__DIR__, 2), '{root}', $value);
    }
    if (is_array($value)) {
        return array_map('speedyspec_i18n_portable', $value);
    }
    return $value;
}

/**
 * @param list<array{string, Closure}> $added
 *     What speedyspec_i18n_log() returned.
 */
function speedyspec_i18n_unlog(array $added): void {
    foreach ($added as [$hook, $f]) {
        remove_filter($hook, $f, 10);
    }
}

/**
 * A WP_Locale's state: every property, the globals it registers, and its getters.
 *
 * @param WP_Locale $l
 *     The locale.
 * @return array<string, mixed>
 *     The state.
 */
function speedyspec_i18n_locale_state($l): array {
    return array(
        get_object_vars($l),
        array($GLOBALS['weekday'], $GLOBALS['weekday_initial'], $GLOBALS['weekday_abbrev'], $GLOBALS['month'], $GLOBALS['month_abbrev']),
        array($l->is_rtl(), $l->get_month(1), $l->get_month('13'), $l->get_month_genitive(2), $l->get_weekday(0),
            $l->get_meridiem('am'), $l->get_list_item_separator(), $l->get_word_count_type()),
    );
}

function speedyspec_i18n_locale_calls(string $languages, string $data, callable $r): void {
    $gettext = array('gettext', 'gettext_with_context');

    foreach (array('en_US', 'de_DE', 'es_ES', 'ja_JP') as $locale) {
        $r("locale:$locale", function () use ($locale, $gettext) {
            load_default_textdomain($locale);
            $log = array();
            $added = speedyspec_i18n_log($gettext, $log);
            $l = new WP_Locale();
            speedyspec_i18n_unlog($added);
            $state = speedyspec_i18n_locale_state($l);
            unload_textdomain('default', true);
            return array($state, $log);
        });
    }
    $r('locale:rtl', function () use ($data) {
        load_textdomain('default', "$data/rtl.mo", 'he_IL');
        $state = speedyspec_i18n_locale_state(new WP_Locale());
        unload_textdomain('default', true);
        return $state;
    });
    $r('locale:text-direction-global', function () {
        $GLOBALS['text_direction'] = 'rtl';
        $state = speedyspec_i18n_locale_state(new WP_Locale());
        unset($GLOBALS['text_direction']);
        return $state;
    });
    $r('locale:subclass', function () {
        $l = new class extends WP_Locale {
            public function init() { parent::init(); $this->dynamic = 'yes'; }
            public function get_word_count_type() { return 'characters_including_spaces'; }
        };
        return array(get_object_vars($l), $l->word_count_type);
    });

    $r('switch:sequence', function () {
        global $wp_locale_switcher;
        $saved = $wp_locale_switcher;
        $wp_locale_switcher = new WP_Locale_Switcher();
        $wp_locale_switcher->init();
        $log = array();
        $added = speedyspec_i18n_log(array('switch_locale', 'restore_previous_locale', 'change_locale'), $log);
        try {
        $state = function ($ret) {
            global $wp_locale_switcher;
            return array($ret, is_locale_switched(), $wp_locale_switcher->get_switched_locale(),
                $wp_locale_switcher->get_switched_user_id(), determine_locale(), get_locale(),
                // Only the domains switching touches, in $l10n's order (the replay runs after other checks).
                array_values(array_intersect(array_keys((array) ($GLOBALS['l10n'] ?? array())), array('default', 'internationalized-plugin'))),
                $GLOBALS['wp_locale']->get_month(1),
                __('This is a dummy plugin', 'internationalized-plugin'));
        };
        $steps = array(
            'filters' => array(has_filter('locale', array($wp_locale_switcher, 'filter_locale')), has_filter('determine_locale', array($wp_locale_switcher, 'filter_locale'))),
            'plain' => $state(null),
            'es' => $state(switch_to_locale('es_ES')),
            'de' => $state(switch_to_locale('de_DE')),
            'de-again' => $state(switch_to_locale('de_DE')),
            'missing' => $state(switch_to_locale('foo_BAR')),
            'restore' => $state(restore_previous_locale()),
            'restore-current' => $state(restore_current_locale()),
            'restore-empty' => $state(restore_previous_locale()),
            'restore-current-empty' => $state(restore_current_locale()),
        );
        $inner = function ($locale) use (&$inner) {
            remove_action('change_locale', $inner, 5);
            switch_to_locale('de_DE');
        };
        add_action('change_locale', $inner, 5);
        $steps['nested'] = $state(switch_to_locale('es_ES'));
        $steps['nested-restore'] = $state(restore_previous_locale());
        $steps['nested-restore-current'] = $state(restore_current_locale());
        return array($steps, $log);
        } finally {
            restore_current_locale();
            speedyspec_i18n_unlog($added);
            remove_filter('locale', array($wp_locale_switcher, 'filter_locale'));
            remove_filter('determine_locale', array($wp_locale_switcher, 'filter_locale'));
            $wp_locale_switcher = $saved;
            unload_textdomain('internationalized-plugin');
            unload_textdomain('default', true);
        }
    });
    $r('switch:replaced-object', function () {
        global $wp_locale_switcher;
        $saved = $wp_locale_switcher;
        $wp_locale_switcher = new class {
            public function switch_to_locale($locale) { return "switched:$locale"; }
            public function restore_previous_locale() { return 'restored'; }
            public function restore_current_locale() { return 'current'; }
            public function is_switched() { return 'maybe'; }
            public function switch_to_user_locale($user_id) { return "user:$user_id"; }
        };
        $out = array(switch_to_locale('de_DE'), restore_previous_locale(), restore_current_locale(), is_locale_switched(), switch_to_user_locale(7));
        $wp_locale_switcher = null;
        $out[] = array(switch_to_locale('de_DE'), restore_previous_locale(), restore_current_locale(), switch_to_user_locale(7));
        $wp_locale_switcher = $saved;
        return $out;
    });

    $scripts = array(
        array('test-example-root', '/wp-includes/js/script.js', 'default', null),
        array('test-example-cdn', 'https://my-cdn.com/wordpress/wp-includes/js/script.js', 'default', array('load_script_textdomain_relative_path', function ($relative, $src) { return preg_match('#^https://my-cdn.com/wordpress/(.*)$#', $src, $m) ? $m[1] : $relative; }, 2)),
        array('test-example-subdir', '/wp/wp-includes/js/script.js', 'default', array('site_url', function ($u) { return $u . '/wp'; }, 1)),
        array('plugin-example-1', 'https://plugins.example.com/my-plugin/js/script.js', 'internationalized-plugin', array('plugins_url', function () { return 'https://plugins.example.com'; }, 1)),
        array('plugin-example-2', 'https://content.example.com/plugins/my-plugin/js/script.js', 'internationalized-plugin', array('content_url', function () { return 'https://content.example.com'; }, 1)),
        array('no-content-host', 'https://content.example.com/plugins/my-plugin/js/script.js', 'internationalized-plugin', array('content_url', function () { return '/'; }, 1)),
        array('no-plugins-host', 'https://plugins.example.com/my-plugin/js/script.js', 'internationalized-plugin', array('plugins_url', function () { return '/'; }, 1)),
        array('no-site-host', '/wp/wp-includes/js/script.js', 'default', array('site_url', function () { return '/wp'; }, 1)),
        array('custom-plugin-dir', '/wp-content/mods/my-plugin/js/script.js', 'internationalized-plugin', array('plugins_url', function () { return 'https://example.com/wp-content/mods'; }, 1)),
        array('theme-script', '/wp-content/themes/my-theme/js/script.js', 'internationalized-theme', null),
        array('min-script', '/wp-includes/js/script.min.js', 'default', null),
        array('script-handle', '/wp-includes/js/other.js', 'admin', null),
        array('unparseable', 'http:///example.com', 'default', null),
        array('no-path', 'https://example.com', 'default', null),
        array('external', 'https://elsewhere.example.net/lib.js', 'default', null),
    );
    $script_hooks = array('pre_load_script_translations', 'load_script_translation_file', 'load_script_translations', 'load_script_textdomain_relative_path');
    foreach ($scripts as [$handle, $src, $domain, $filter]) {
        $r("script:$handle", function () use ($handle, $src, $domain, $filter, $languages, $script_hooks) {
            if ($filter) {
                add_filter($filter[0], $filter[1], 10, $filter[2]);
            }
            wp_register_script($handle, $src, array(), null);
            $log = array();
            $added = speedyspec_i18n_log($script_hooks, $log);
            $with_path = load_script_textdomain($handle, $domain, $languages);
            $without_path = load_script_textdomain($handle, $domain);
            speedyspec_i18n_unlog($added);
            if ($filter) {
                remove_filter($filter[0], $filter[1], 10);
            }
            wp_deregister_script($handle);
            return array($with_path, $without_path, $log);
        });
    }
    $r('script:unregistered', function () { return load_script_textdomain('never-registered'); });
    $r('script:module', function () use ($languages, $script_hooks) {
        wp_register_script_module('speedyspec-module', '/wp-includes/js/script.js');
        $log = array();
        $added = speedyspec_i18n_log($script_hooks, $log);
        $out = array(load_script_module_textdomain('speedyspec-module', 'default', $languages), load_script_module_textdomain('never-registered'));
        speedyspec_i18n_unlog($added);
        wp_deregister_script_module('speedyspec-module');
        return array($out, $log);
    });
    foreach (array('false' => false, 'empty' => '', 'array' => array('a' => 1), 'string' => '{"x":1}') as $name => $value) {
        $r("script:pre-$name", function () use ($value, $languages) {
            $f = function () use ($value) { return $value; };
            add_filter('pre_load_script_translations', $f);
            try {
                return load_script_translations("$languages/en_US-813e104eb47e13dd4cc5af844c618754.json", 'h', 'default');
            } finally {
                remove_filter('pre_load_script_translations', $f);
            }
        });
        $r("script:file-$name", function () use ($value, $languages) {
            $f = function () use ($value) { return $value; };
            add_filter('load_script_translation_file', $f);
            try {
                return load_script_translations("$languages/en_US-813e104eb47e13dd4cc5af844c618754.json", 'h', 'default');
            } finally {
                remove_filter('load_script_translation_file', $f);
            }
        });
    }
    $r('script:direct', function () use ($languages) {
        return array(load_script_translations("$languages/en_US-813e104eb47e13dd4cc5af844c618754.json", 'h', 'default'),
            load_script_translations("$languages/missing.json", 'h', 'default'), load_script_translations(false, 'h', 'default'),
            load_script_translations('', 'h', 'default'));
    });

    $r('schema:wordpress', function () use ($languages) {
        $f = function () { return 'pl_PL'; };
        add_filter('locale', $f);
        load_textdomain('notice', "$languages/plugins/notice-pl_PL.mo");
        $schema = (object) array(
            'title' => 'block title',
            'keywords' => array('block keyword'),
            'styles' => array((object) array('label' => 'block style label')),
            'context' => (object) array('*' => (object) array('variations' => array((object) array('title' => 'block variation title', 'description' => 'block variation description', 'keywords' => array('block variation keyword'))))),
        );
        $settings = array(
            'title' => 'Notice', 'category' => 'common', 'icon' => 'star', 'description' => 'Shows warning, error or success notices…',
            'keywords' => array('alert', 'message'), 'textdomain' => 'notice',
            'styles' => array(array('name' => 'default', 'label' => 'Default', 'isDefault' => true), array('name' => 'other', 'label' => 'Other')),
            'context' => array('namespace' => array('variations' => array(array('title' => 'Error', 'description' => 'Shows error.', 'keywords' => array('failure'))))),
        );
        $log = array();
        $added = speedyspec_i18n_log(array('gettext_with_context'), $log);
        $out = translate_settings_using_i18n_schema($schema, $settings, 'notice');
        speedyspec_i18n_unlog($added);
        unload_textdomain('notice');
        remove_filter('locale', $f);
        return array($out, $log);
    });
    $r('schema:edges', function () {
        $obj = new stdClass();
        $obj->keep = 'me';
        $out = array(
            translate_settings_using_i18n_schema('0', 'x', 'd'),
            translate_settings_using_i18n_schema('ctx', '', 'd'),
            translate_settings_using_i18n_schema('ctx', 'x', ''),
            translate_settings_using_i18n_schema('ctx', array('a'), 'd'),
            translate_settings_using_i18n_schema(array('ctx'), 'x', 'd'),
            translate_settings_using_i18n_schema(array('ctx'), array('k' => 'a', 5 => 'b'), 'd'),
            translate_settings_using_i18n_schema((object) array('a' => 'ctx', '*' => 'star'), array('a' => 'x', 'b' => 'y', 0 => 'z'), 'd'),
            translate_settings_using_i18n_schema((object) array('a' => array('ctx')), array('a' => array('p', 'q')), 'd'),
            translate_settings_using_i18n_schema((object) array('a' => null), array('a' => 'x'), 'd'),
            translate_settings_using_i18n_schema(5, 'x', 'd'),
        );
        $kept = translate_settings_using_i18n_schema((object) array('a' => 'ctx'), array('a' => 'x', 'o' => $obj), 'd');
        $out[] = $kept['o'] === $obj;
        return $out;
    });

    $r('helpers', function () {
        global $wp_locale;
        $saved = $wp_locale;
        $out = array();
        $wp_locale = null;
        $out[] = array(wp_get_list_item_separator(), wp_get_word_count_type());
        $wp_locale = new WP_Locale();
        $out[] = array(wp_get_list_item_separator(), wp_get_word_count_type());
        $wp_locale = new class extends WP_Locale {
            public function get_list_item_separator() { return ' | '; }
            public function get_word_count_type() { return 'custom'; }
        };
        $out[] = array(wp_get_list_item_separator(), wp_get_word_count_type());
        $wp_locale = new stdClass();
        $out[] = array(wp_get_list_item_separator(), wp_get_word_count_type());
        $wp_locale = $saved;
        return $out;
    });
}
