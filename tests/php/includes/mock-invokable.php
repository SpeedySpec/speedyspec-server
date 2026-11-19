<?php
/*
 * Copied unchanged from WordPress's own tests (wordpress-develop, tests/phpunit/includes/mock-invokable.php).
 *
 * WordPress is free software, released under the GNU General Public License, version 2 or later.
 * Copyright 2011-2026 by the WordPress contributors. See https://wordpress.org/about/license/.
 */
/**
 * File for Mock_Invokable class.
 *
 * @package WordPress
 * @subpackage UnitTests
 */

/**
 * Class Mock_Invokable.
 *
 * This class is used to mock a class that has an `__invoke` method.
 */
class Mock_Invokable {

	public function __invoke() {}
}
