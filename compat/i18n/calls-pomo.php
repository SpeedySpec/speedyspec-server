<?php
/**
 * Phase 4's recorded calls: the pomo classes (MO, PO, Plural_Forms, the PO helpers), MO and PO export, and
 * WP_Translation_File::transform(). Objects are recorded as their properties, recursively, so entries and headers
 * compare exactly; paths under the checkout are made relative.
 *
 * `$root` is the checkout; `$data` holds the corpus's own fixtures.
 */

/**
 * A value with objects turned into `[class, properties]`, recursively.
 *
 * @param mixed $value
 *     Any value.
 * @return mixed
 *     The value, objects expanded.
 */
function speedyspec_pomo_shape($value) {
    if (is_object($value)) {
        $props = array();
        foreach ((array) $value as $k => $v) {
            // Protected and private properties as PHP's array cast names them, without the NUL markers.
            $props[str_replace("\0", '|', $k)] = speedyspec_pomo_shape($v);
        }
        return array(get_class($value), $props);
    }
    if (is_array($value)) {
        return array_map('speedyspec_pomo_shape', $value);
    }
    return $value;
}

/**
 * A translations object's state after an import, with the import's result.
 *
 * @param mixed $result
 *     What import_from_file() returned.
 * @param Gettext_Translations $t
 *     The object.
 * @return array
 *     The state.
 */
function speedyspec_pomo_state($result, $t): array {
    $plurals = array();
    foreach (array(0, 1, 2, 3, 11, 21) as $n) {
        $plurals[$n] = $t->select_plural_form($n);
    }
    return speedyspec_i18n_portable(array($result, speedyspec_pomo_shape($t), $t->get_plural_forms_count(), $plurals));
}

function speedyspec_pomo_calls(string $root, string $data, callable $r): void {
    // Canonical paths, so case names and recorded paths are the same whoever passes `..` segments.
    $root = realpath($root);
    $data = realpath($data);
    $mo_files = array_merge(
        glob("$root/tests/php/data/pomo/*.mo"),
        glob("$root/tests/php/data/l10n/*.mo"),
        glob("$data/*.mo"),
        glob("$data/mo/*.mo")
    );
    sort($mo_files);
    foreach ($mo_files as $file) {
        $name = speedyspec_i18n_portable($file);
        $r("mo-import:$name", function () use ($file) {
            $mo = new MO();
            return speedyspec_pomo_state($mo->import_from_file($file), $mo);
        });
        $r("mo-export:$name", function () use ($file) {
            $mo = new MO();
            $mo->import_from_file($file);
            $first = md5($mo->export());
            $mo->add_entry(new Translation_Entry(array('singular' => 'zz-untranslated')));
            $mo->add_entry(new Translation_Entry(array('singular' => 'zz-plural', 'plural' => 'zz-plurals', 'translations' => array('eins', 'zwei'))));
            return array($first, md5($mo->export()));
        });
    }
    $r('mo-import:missing', function () {
        $mo = new MO();
        return speedyspec_pomo_state($mo->import_from_file('/nonexistent/file.mo'), $mo);
    });

    $po_files = array_merge(glob("$root/tests/php/data/pomo/*.po*"), glob("$data/po/*.po"));
    sort($po_files);
    foreach ($po_files as $file) {
        $name = speedyspec_i18n_portable($file);
        $r("po-import:$name", function () use ($file) {
            $po = new PO();
            return speedyspec_pomo_state($po->import_from_file($file), $po);
        });
        $r("po-export:$name", function () use ($file) {
            $po = new PO();
            $po->import_from_file($file);
            $po->set_comment_before_headers("Before\nheaders");
            return array($po->export(), $po->export(false), $po->export_headers(), $po->export_entries());
        });
    }

    $strings = array('', 'plain', "a\nb", "a\nb\n", "\n", "\n\n", "tab\there", "back\\slash", 'quote"d', "cr\rhere",
        "trailing space \n", "\xff\xfe bytes", "multi\nline\nwith \"quotes\" and \\", "ends\\");
    $r('po-static', function () use ($strings) {
        $out = array();
        foreach ($strings as $s) {
            $out[] = array(
                PO::poify($s), PO::unpoify('"' . $s . '"'), PO::unpoify(PO::poify($s)), PO::prepend_each_line($s, '#. '),
                PO::comment_block($s), PO::comment_block($s, ':'), PO::trim_quotes($s), PO::trim_quotes('"' . $s . '"'),
                PO::match_begin_and_end_newlines($s, "\n" . $s), PO::match_begin_and_end_newlines("\n$s\n", 'x'),
            );
        }
        $entries = array(
            new Translation_Entry(array('singular' => 'a', 'translations' => array('b'))),
            new Translation_Entry(array('singular' => "multi\nline", 'translations' => array("viele\nZeilen"), 'context' => 'ctx')),
            new Translation_Entry(array('singular' => 'cat', 'plural' => 'cats', 'translations' => array('Katze', 'Katzen'))),
            new Translation_Entry(array('singular' => 'cat', 'plural' => 'cats')),
            new Translation_Entry(array('singular' => 'r', 'references' => array('a.php:1', 'b.php:2'), 'flags' => array('fuzzy', 'php-format'),
                'translator_comments' => "one\ntwo", 'extracted_comments' => 'extracted')),
            new Translation_Entry(array('singular' => null)),
            new Translation_Entry(array('singular' => '', 'translations' => array("H: v\n"))),
        );
        foreach ($entries as $e) {
            $out[] = PO::export_entry($e);
        }
        return $out;
    });

    $expressions = array('n != 1', 'n > 1', 'n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2',
        '(n == 1) ? 0 : ((n >= 2 && n <= 4) ? 1 : 2)', 'n ? 1 : 2', '0', 'n % 0', 'n # 2', '((n)', '(n))', 'n : 2', 'n ? 1', '', ' ', 'n n',
        '!n', 'n == 1 || n == 2', '99999999999999999999 % 7', 'n =! 1');
    $ns = array_merge(range(-3, 30), range(100, 110), array(PHP_INT_MAX));
    foreach ($expressions as $i => $e) {
        $r("plural-forms:$i", function () use ($e, $ns) {
            $p = new Plural_Forms($e);
            $out = array();
            foreach ($ns as $n) {
                try {
                    $out[$n] = $p->get($n);
                } catch (Throwable $t) {
                    $out[$n] = 'threw ' . get_class($t) . ': ' . $t->getMessage();
                }
            }
            return $out;
        });
    }

    $transform = array_merge(glob("$root/tests/php/data/l10n/*.mo"), glob("$root/tests/php/data/l10n/*.php"),
        glob("$data/*.mo"), glob("$data/*.l10n.php"), glob("$data/mo/*.mo"));
    sort($transform);
    foreach ($transform as $file) {
        $name = speedyspec_i18n_portable($file);
        foreach (array('mo', 'php') as $type) {
            $r("transform:$name:$type", function () use ($file, $type) {
                $out = WP_Translation_File::transform($file, $type);
                return is_string($out) && strlen($out) < 600 ? $out : (is_string($out) ? md5($out) : $out);
            });
        }
    }

    $r('subclass', function () use ($root) {
        $seen = array();
        $mo = new class extends MO {
            public $seen = array();
            public function &make_entry($original, $translation) {
                $this->seen[] = $original;
                $entry = parent::make_entry($original, $translation);
                return $entry;
            }
            public function export_original($entry) {
                return 'X' . parent::export_original($entry);
            }
        };
        $mo->import_from_file("$root/tests/php/data/pomo/simple.mo");
        $po = new class extends PO {
            public $reads = 0;
            public function read_entry($f, $lineno = 0) {
                ++$this->reads;
                return parent::read_entry($f, $lineno);
            }
        };
        $po->import_from_file("$root/tests/php/data/pomo/simple.po");
        return array($mo->seen, md5($mo->export()), $po->reads, count($po->entries));
    });
}
