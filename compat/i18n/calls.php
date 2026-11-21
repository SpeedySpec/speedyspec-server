<?php
/**
 * The calls record.php and the port's test both make, in order. Each result is serialize()d, so types count too;
 * a thrown error is recorded as its class and message.
 */

function speedyspec_i18n_try(callable $f) {
    try {
        return base64_encode(serialize($f()));
    } catch (Throwable $e) {
        return 'threw ' . get_class($e) . ': ' . $e->getMessage();
    }
}

function speedyspec_i18n_calls(string $data): array {
    $out = array();
    $r = function ($name, callable $f) use (&$out) {
        $out[$name] = speedyspec_i18n_try($f);
    };
    $locale = 'de_DE';
    $fixtures = array('simple.mo', 'plural.mo', 'pomo-context.mo', 'fa_IR.mo', 'pomo-de_DE-2.8.mo', 'pomo-overload.mo',
        'example-simple.mo', 'plural-complex.mo', 'duplicates.mo', 'escapes.l10n.php', 'broken-magic.mo', 'big-endian.mo');
    foreach ($fixtures as $i => $f) {
        $domain = "d$i";
        $r("load:$f", function () use ($data, $f, $domain, $locale) { return load_textdomain($domain, "$data/$f", $locale); });
        $r("loaded:$f", function () use ($domain) { return is_textdomain_loaded($domain); });
        $file = WP_Translation_File::create("$data/$f");
        $entries = $file ? $file->entries() : array();
        $r("create:$f", function () use ($file) { return $file ? array($file->headers(), $file->error(), get_class($file)) : false; });
        $n = 0;
        foreach ($entries as $key => $value) {
            if (++$n > 40) {
                break;
            }
            $key = (string) $key;
            $parts = explode("\4", $key);
            if (isset($parts[1])) {
                $r("_x:$f:$key", function () use ($parts, $domain) { return _x($parts[1], $parts[0], $domain); });
                foreach (array(0, 1, 2, 5, 11, 21, 101) as $c) {
                    $r("_nx:$f:$key:$c", function () use ($parts, $c, $domain) { return _nx($parts[1], $parts[1] . 's', $c, $parts[0], $domain); });
                }
            } else {
                $r("__:$f:$key", function () use ($key, $domain) { return __($key, $domain); });
                $r("translate:$f:$key", function () use ($key, $domain) { return translate($key, $domain); });
                foreach (array(0, 1, 2, 3, 5, 11, 21, 22, 101, -1) as $c) {
                    $r("_n:$f:$key:$c", function () use ($key, $c, $domain) { return _n($key, 'many ' . $key, $c, $domain); });
                }
            }
        }
        $r("miss:$f", function () use ($domain) { return array(__('not there', $domain), _x('not there', 'ctx', $domain), _n('cat', 'cats', 1, $domain), _n('cat', 'cats', 2, $domain), _nx('cat', 'cats', 3, 'ctx', $domain)); });
        $r("entries:$f", function () use ($domain) { $t = get_translations_for_domain($domain); return array(get_class($t), count((array) $t->entries), $t->headers); });
        $r("loaded-after:$f", function () use ($domain) { return is_textdomain_loaded($domain); });
    }
    $r('controller', function () use ($data, $locale) {
        $c = WP_Translation_Controller::get_instance();
        return array($c->get_locale(), $c->is_textdomain_loaded('d0', $locale), $c->has_translation('baba', 'd0', $locale), $c->has_translation('baba', 'd0'), $c->translate('baba', '', 'd0', $locale), $c->translate_plural(array('one dragon', '%d dragons'), 5, '', 'd1', $locale), array_keys($c->get_headers('d0')));
    });
    $r('new-controller', function () use ($data) {
        $c = new WP_Translation_Controller();
        return array($c->load_file("$data/simple.mo", 'x'), $c->translate('baba', '', 'x'), $c->unload_file("$data/simple.mo", 'x'), $c->translate('baba', '', 'x'), $c->unload_textdomain('foo'), WP_Translation_Controller::get_instance()->translate('baba', '', 'x'));
    });
    $r('noop', function () {
        $a = __('anything', 'never-loaded');
        $b = __('else', 'also-never');
        return array($a, $b, get_class($GLOBALS['l10n']['never-loaded']), $GLOBALS['l10n']['never-loaded'] === $GLOBALS['l10n']['also-never'], is_textdomain_loaded('never-loaded'));
    });
    $r('plugin-object', function () {
        $GLOBALS['l10n']['plug'] = new class extends Translations {
            public function translate($singular, $context = null) { return "X$singular" . ($context ? "|$context" : ''); }
            public function translate_plural($singular, $plural, $count, $context = null) { return "P$count"; }
        };
        return array(__('a', 'plug'), _x('a', 'c', 'plug'), _n('a', 'b', 3, 'plug'), _nx('a', 'b', 4, 'c', 'plug'));
    });
    $r('coercion', function () {
        return array(__(42, 'd0'), __(null, 'd0'), __('baba', 'd0'), __('baba'), _n('one dragon', '%d dragons', '3', 'd1'), _n('one dragon', '%d dragons', 3.7, 'd1'));
    });
    $r('filters', function () {
        $log = array();
        $g = function ($t, $text, $domain) use (&$log) { $log[] = array('gettext', $t, $text, $domain); return strtoupper($t); };
        $gd = function ($t, $text, $domain) use (&$log) { $log[] = array('gettext_d0', $t, $text, $domain); return "[$t]"; };
        $n = function ($t, $s, $p, $c, $domain) use (&$log) { $log[] = array('ngettext', $t, $s, $p, $c, $domain); return $t; };
        add_filter('gettext', $g, 10, 3);
        add_filter('gettext_d0', $gd, 10, 3);
        add_filter('ngettext', $n, 10, 5);
        $res = array(__('baba', 'd0'), __('nope', 'd0'), translate('baba', 'd1'), _n('one dragon', '%d dragons', 2, 'd1'), _x('baba', 'ctx', 'd0'));
        remove_filter('gettext', $g, 10);
        remove_filter('gettext_d0', $gd, 10);
        remove_filter('ngettext', $n, 10);
        return array($res, $log, __('baba', 'd0'), did_filter('gettext') > 0, did_filter('gettext_d0') > 0);
    });
    $r('division-by-zero', function () use ($data) {
        load_textdomain('dz', "$data/divzero.mo", 'de_DE');
        return array(__('cat', 'dz'), _n('cat', 'cats', 1, 'dz'));
    });
    $r('unload', function () {
        return array(unload_textdomain('d0'), is_textdomain_loaded('d0'), __('baba', 'd0'), unload_textdomain('never-loaded'), unload_textdomain('d1', true), is_textdomain_loaded('d1'), isset($GLOBALS['l10n_unloaded']['d0']), isset($GLOBALS['l10n_unloaded']['d1']));
    });
    $r('transform', function () use ($data) {
        return array(md5((string) WP_Translation_File::transform("$data/simple.mo", 'php')), md5((string) WP_Translation_File::transform("$data/escapes.l10n.php", 'mo')), WP_Translation_File::transform("$data/simple.mo", 'po'), WP_Translation_File::transform("$data/nope.mo", 'php'));
    });
    $r('unsupported-php', function () use ($data) {
        return array(load_textdomain('up', "$data/unsupported-call.l10n.php", 'de_DE'), __('a', 'up'));
    });
    $r('reflection', function () {
        $out = array();
        foreach (array('__', '_x', '_n', '_nx', 'translate', 'translate_with_gettext_context') as $fn) {
            $p = array();
            foreach ((new ReflectionFunction($fn))->getParameters() as $param) {
                $p[] = array($param->getName(), $param->isOptional(), $param->isOptional() ? $param->getDefaultValue() : null);
            }
            $out[$fn] = $p;
        }
        foreach (array('WP_Translation_Controller', 'WP_Translation_File', 'WP_Translation_File_MO', 'WP_Translation_File_PHP', 'WP_Translations') as $class) {
            $methods = array();
            foreach ((new ReflectionClass($class))->getMethods() as $m) {
                if ($m->isPrivate()) {
                    continue;
                }
                $params = array();
                foreach ($m->getParameters() as $param) {
                    $params[] = $param->getName() . ($param->isOptional() ? '?' : '') . ':' . $param->getType();
                }
                $methods[$m->getName()] = array($m->isStatic(), $m->isProtected(), implode(',', $params), (string) $m->getReturnType());
            }
            ksort($methods);
            $out[$class] = $methods;
        }
        return $out;
    });
    require_once __DIR__ . '/calls-locale.php';
    speedyspec_i18n_locale_calls(WP_LANG_DIR, $data, $r);
    require_once __DIR__ . '/calls-pomo.php';
    speedyspec_pomo_calls(dirname(__DIR__, 2), $data, $r);
    return $out;
}
