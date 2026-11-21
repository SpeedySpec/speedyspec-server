<?php
/**
 * Builds the formatting area's scenarios from WordPress's own PHPUnit data providers.
 *
 * Each entry of providers.json names a test file, its class, a data provider and the function under test; each row
 * the provider returns becomes a one-step scenario calling that function with the row's argument columns. WordPress's
 * expected values are ignored: the recording against stock WordPress is the expectation. An entry with
 * `whole_posts` makes one scenario per post in data/formatting/whole-posts.php.
 *
 * Expects SPEEDYSPEC_FORMATTING_PROVIDERS, SPEEDYSPEC_FORMATTING_TESTS and DIR_TESTDATA to be defined. Returns the
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

function speedyspec_formatting_scenarios(): string
{
    $entries = json_decode((string) file_get_contents(SPEEDYSPEC_FORMATTING_PROVIDERS), true, 512, JSON_THROW_ON_ERROR);
    $scenarios = [];
    foreach ($entries as $entry) {
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
            $scenarios[] = [
                'name' => "$function/{$entry['provider']}/$key",
                'steps' => [['call' => $function, 'args' => $args]],
            ];
        }
    }
    return json_encode(
        ['area' => 'formatting', 'scenarios' => $scenarios],
        JSON_THROW_ON_ERROR | JSON_PRESERVE_ZERO_FRACTION | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE
    );
}
