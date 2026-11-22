<?php
/**
 * Builds the formatting area's scenarios from WordPress's own PHPUnit data providers.
 *
 * Each entry of providers.json names a test file, its class, a data provider and the function under test; each row
 * the provider returns becomes a one-step scenario calling that function with the row's argument columns. WordPress's
 * expected values are ignored: the recording against stock WordPress is the expectation. An entry with
 * `whole_posts` makes one scenario per post in data/formatting/whole-posts.php.
 *
 * An entry with `capture` runs every test method of that test file (each data provider row in turn) with its calls
 * to `functions` rewritten to a recorder, and makes one scenario per distinct call, as
 * `<function>/capture/<method>/<n>`. That covers WordPress's literal calls, including ones a test builds in a loop,
 * without transcribing them. `skip` lists methods to leave out, and `as` maps a method's calls onto a wrapper
 * function from setup.php: `{"function": W, "args": [columns], "extra": [values]}`; a mapped call whose first
 * argument is `'reset'` (wptexturize's reset after a translation test) is dropped.
 *
 * literals.json adds hand-written cases: WordPress's literal test inputs and edge cases, already in the corpus
 * encoding, as `<function>/literal/<name>`.
 *
 * Expects SPEEDYSPEC_FORMATTING_PROVIDERS, SPEEDYSPEC_FORMATTING_LITERALS, SPEEDYSPEC_FORMATTING_TESTS and
 * DIR_TESTDATA to be defined. Returns the
 * catalogue as JSON.
 */

declare(strict_types=1);

if (!class_exists('WP_UnitTestCase')) {
    abstract class WP_UnitTestCase
    {
    }
}

function speedyspec_formatting_encode(mixed $value): mixed
{
    if (is_string($value)) {
        return mb_check_encoding($value, 'UTF-8') ? $value : ['bytes' => base64_encode($value)];
    }
    if (is_float($value)) {
        return ['float' => $value];
    }
    if (is_array($value)) {
        $pairs = [];
        foreach ($value as $key => $item) {
            $pairs[] = [speedyspec_formatting_encode($key), speedyspec_formatting_encode($item)];
        }
        return ['array' => $pairs];
    }
    if ($value === null || is_bool($value) || is_int($value)) {
        return $value;
    }
    throw new RuntimeException('a provider argument cannot be recorded: ' . get_debug_type($value));
}

/** The test case base class the capture mode gives the rewritten tests: every assertion and expectation is a no-op. */
abstract class SpeedySpecCaptureCase
{
    public function set_up()
    {
    }

    public function tear_down()
    {
    }

    public function __call(string $name, array $arguments): mixed
    {
        return null;
    }
}

/** The calls the capture mode has recorded, in order, and the method that is running. */
final class SpeedySpecCapture
{
    /** @var list<array{string, string, list<mixed>}> */
    public static array $calls = [];
    public static string $method = '';
}

/**
 * Records one call from a rewritten test.
 *
 * @param string $function
 *     The function the test called.
 * @param mixed ...$args
 *     Its arguments.
 * @return mixed
 *     The first argument: the generator runs without WordPress, so the test sees its input back.
 */
function speedyspec_capture(string $function, mixed ...$args): mixed
{
    SpeedySpecCapture::$calls[] = [SpeedySpecCapture::$method, $function, $args];
    return $args[0] ?? null;
}

foreach (['add_filter', 'remove_filter', 'switch_to_locale', 'restore_previous_locale', 'benchmark_pcre_backtracking',
    '_get_wptexturize_split_regex', '_get_wptexturize_shortcode_regex', 'get_shortcode_regex', 'smilies_init',
    'update_option'] as $stub) {
    if (!function_exists($stub)) {
        eval("function $stub(...\$args) { return null; }");
    }
}

// What data providers call to build their rows: the home URL a WordPress test site has.
if (!function_exists('home_url')) {
    function home_url($path = '')
    {
        return 'http://example.org' . ($path ? '/' . ltrim($path, '/') : '');
    }
}
if (!class_exists('WP_Query')) {
    // sanitizeTextField.php's provider passes one as an argument; the encoder drops its row.
    class WP_Query
    {
    }
}
if (!function_exists('includes_url')) {
    function includes_url($path = '')
    {
        return 'https://example.org/wp-includes/' . $path;
    }
}
if (!function_exists('set_url_scheme')) {
    function set_url_scheme($url, $scheme = null)
    {
        return preg_replace('#^\w+://#', $scheme . '://', $url);
    }
}

/**
 * Runs one test file's methods with their calls rewritten, and returns the scenarios for the distinct calls.
 *
 * @param array<string, mixed> $entry
 *     The providers.json entry: `capture` (the test file), `class`, `functions`, optional `skip` and `as`.
 * @return list<array<string, mixed>>
 *     One scenario per distinct call.
 */
function speedyspec_formatting_capture(array $entry): array
{
    $source = (string) file_get_contents(SPEEDYSPEC_FORMATTING_TESTS . '/' . $entry['capture']);
    $source = preg_replace('/^<\?php/', '', $source);
    $class = $entry['class'] . '_SpeedySpecCapture';
    $source = preg_replace('/\bclass\s+' . preg_quote($entry['class'], '/') . '\s+extends\s+WP_UnitTestCase/',
        "class $class extends SpeedySpecCaptureCase", $source, 1, $renamed);
    if ($renamed !== 1) {
        throw new RuntimeException("{$entry['capture']}: no class {$entry['class']} extending WP_UnitTestCase");
    }
    foreach ($entry['functions'] as $function) {
        $source = preg_replace('/(?<![\w>$:\'"])' . preg_quote($function, '/') . '\s*\(/',
            "speedyspec_capture('$function', ", $source);
    }
    eval($source);
    SpeedySpecCapture::$calls = [];
    $reflection = new ReflectionClass($class);
    foreach ($reflection->getMethods(ReflectionMethod::IS_PUBLIC) as $method) {
        $name = $method->getName();
        if (!str_starts_with($name, 'test') || in_array($name, $entry['skip'] ?? [], true)) {
            continue;
        }
        SpeedySpecCapture::$method = $name;
        $rows = [[]];
        if (preg_match('/@dataProvider\s+(\w+)/', (string) $method->getDocComment(), $provider)) {
            $rows = $reflection->newInstance()->{$provider[1]}();
        }
        foreach ($rows as $row) {
            $test = $reflection->newInstance();
            try {
                $test->set_up();
                $method->invokeArgs($test, array_values($row));
            } catch (Throwable) {
                // A test that needs more of WordPress stops here; the calls it made before are kept.
            }
        }
    }
    $scenarios = [];
    $seen = [];
    $counts = [];
    foreach (SpeedySpecCapture::$calls as [$method, $function, $args]) {
        $as = $entry['as'][$method] ?? null;
        if ($as !== null) {
            if (($args[0] ?? null) === 'reset') {
                continue;
            }
            $mapped = [];
            foreach ($as['args'] as $column) {
                $mapped[] = $args[$column] ?? null;
            }
            $function = $as['function'];
            $args = $mapped;
        }
        try {
            $encoded = array_map('speedyspec_formatting_encode', $args);
            foreach ($as['extra'] ?? [] as $extra) {
                $encoded[] = $extra;
            }
        } catch (RuntimeException) {
            continue; // An object or a closure: the corpus cannot hold it.
        }
        $key = $function . json_encode($encoded, JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION);
        if (isset($seen[$key])) {
            continue;
        }
        $seen[$key] = true;
        $n = $counts[$function][$method] = ($counts[$function][$method] ?? -1) + 1;
        $scenarios[] = [
            'name' => "$function/capture/$method/$n",
            'steps' => [['call' => $function, 'args' => $encoded]],
        ];
    }
    return $scenarios;
}

function speedyspec_formatting_scenarios(): string
{
    $entries = json_decode((string) file_get_contents(SPEEDYSPEC_FORMATTING_PROVIDERS), true, 512, JSON_THROW_ON_ERROR);
    $scenarios = [];
    foreach ($entries as $entry) {
        if (isset($entry['capture'])) {
            array_push($scenarios, ...speedyspec_formatting_capture($entry));
            continue;
        }
        $function = $entry['function'];
        if (!empty($entry['whole_posts'])) {
            require_once DIR_TESTDATA . '/formatting/whole-posts.php';
            foreach (data_whole_posts() as $index => $row) {
                $scenarios[] = [
                    'name' => "$function/whole_posts/$index",
                    'steps' => [['call' => $function, 'args' => [speedyspec_formatting_encode($row[0])]]],
                ];
            }
            continue;
        }
        require_once SPEEDYSPEC_FORMATTING_TESTS . '/' . $entry['test'];
        $test = new $entry['class']();
        foreach ($test->{$entry['provider']}() as $key => $row) {
            $row = array_values($row);
            $args = [];
            foreach ($entry['args'] as $column) {
                $args[] = speedyspec_formatting_encode($row[$column]);
            }
            foreach ($entry['extra'] ?? [] as $extra) {
                $args[] = $extra;
            }
            $scenarios[] = [
                'name' => "$function/{$entry['provider']}/$key",
                'steps' => [['call' => $function, 'args' => $args]],
            ];
        }
    }
    $literals = json_decode((string) file_get_contents(SPEEDYSPEC_FORMATTING_LITERALS), true, 512, JSON_THROW_ON_ERROR);
    foreach ($literals as $literal) {
        $scenarios[] = [
            'name' => "{$literal['function']}/literal/{$literal['name']}",
            'steps' => [['call' => $literal['function'], 'args' => $literal['args']]],
        ];
    }
    return json_encode(
        ['area' => 'formatting', 'scenarios' => $scenarios],
        JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
    );
}
