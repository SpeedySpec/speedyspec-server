# Changelog

## [1.4.1](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.4.0...speedyspec-php-runtime-v1.4.1) (2026-11-24)

## [1.4.0](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.3.0...speedyspec-php-runtime-v1.4.0) (2026-11-23)


### Features

* **wp-media:** WordPress's MIME list widened to every format the reader reads, and content checks through it (plan 5 of 5) ([#108](https://github.com/SpeedySpec/speedyspec-server/issues/108)) ([c3dc4ae](https://github.com/SpeedySpec/speedyspec-server/commit/c3dc4ae41fa256b14bfa3ba9c2ae3dc0924d1f4e))

## [1.3.0](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.2.0...speedyspec-php-runtime-v1.3.0) (2026-11-22)


### Features

* wp_text_diff() as a container port PHP can rebind ([#85](https://github.com/SpeedySpec/speedyspec-server/issues/85)) ([a71720d](https://github.com/SpeedySpec/speedyspec-server/commit/a71720d0b1061e490e399602724867c70496f002))
* **wp-cron:** WordPress's cron scheduling API in Rust, every write a compare-and-swap ([#86](https://github.com/SpeedySpec/speedyspec-server/issues/86)) ([97fdc98](https://github.com/SpeedySpec/speedyspec-server/commit/97fdc98fd55444b5036e23f2b4301231ac4f260c))
* **wp-media:** WordPress's audio and video metadata functions on FFmpeg (plan 3 of 5) ([#100](https://github.com/SpeedySpec/speedyspec-server/issues/100)) ([d310c7a](https://github.com/SpeedySpec/speedyspec-server/commit/d310c7a67914e110f38c9f5e38869a0c73b94574))
* **wp-media:** WordPress's image functions, with PHP's getimagesize() as the fast path (plan 4 of 5) ([#101](https://github.com/SpeedySpec/speedyspec-server/issues/101)) ([5e3871a](https://github.com/SpeedySpec/speedyspec-server/commit/5e3871a92151d1eccc68efad0b3813ff37bb4c78))


### Build System

* a pinned, LGPL-only static FFmpeg for media metadata (plan 1 of 5) ([#92](https://github.com/SpeedySpec/speedyspec-server/issues/92)) ([52a7b7a](https://github.com/SpeedySpec/speedyspec-server/commit/52a7b7a56270ece5a13e53dff08dce324f00883b))

## [1.2.0](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.1.1...speedyspec-php-runtime-v1.2.0) (2026-11-22)


### Features

* **php-runtime:** options in the worker ([#78](https://github.com/SpeedySpec/speedyspec-server/issues/78)) ([5576e38](https://github.com/SpeedySpec/speedyspec-server/commit/5576e3834c208665e4b03279ef189028c6459a0b))
* WordPress's password hashing in Rust, argon2id by default, pluggables as container ports ([#77](https://github.com/SpeedySpec/speedyspec-server/issues/77)) ([19e900e](https://github.com/SpeedySpec/speedyspec-server/commit/19e900efab08ceb1d210ebb2d2d5a917d97fc3e4))


### Bug Fixes

* **php-runtime:** refuse a release's db.php drop-in when the app has no site directory ([#80](https://github.com/SpeedySpec/speedyspec-server/issues/80)) ([d5dd22d](https://github.com/SpeedySpec/speedyspec-server/commit/d5dd22d5ad107487e1908e5228adee16a839ca33))
* the sodium port's review minors ([#74](https://github.com/SpeedySpec/speedyspec-server/issues/74)) ([51cf912](https://github.com/SpeedySpec/speedyspec-server/commit/51cf912812eaabcbd95638f5e6332e6c11920d18))
* **wp-password:** the password port's deferred review minors, and per-site cost ceilings ([#83](https://github.com/SpeedySpec/speedyspec-server/issues/83)) ([93e64f5](https://github.com/SpeedySpec/speedyspec-server/commit/93e64f5fdeca54f953aeb04b39cf13cc63feb3ab))

## [1.1.1](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.1.0...speedyspec-php-runtime-v1.1.1) (2026-11-22)

## [1.1.0](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.0.4...speedyspec-php-runtime-v1.1.0) (2026-11-22)


### Features

* guarantee ext-sodium, and WordPress's sodium functions in Rust ([#61](https://github.com/SpeedySpec/speedyspec-server/issues/61)) ([5caf81f](https://github.com/SpeedySpec/speedyspec-server/commit/5caf81f97db45d4e3c5ce6180d727174f80a3563))


### Build System

* publish the PHP runtime crates to crates.io ([#64](https://github.com/SpeedySpec/speedyspec-server/issues/64)) ([e56a53c](https://github.com/SpeedySpec/speedyspec-server/commit/e56a53c857e36558b1392f13fde899bccef33d59))

## [1.0.4](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.0.3...speedyspec-php-runtime-v1.0.4) (2026-11-22)

## [1.0.3](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.0.2...speedyspec-php-runtime-v1.0.3) (2026-11-21)

## [1.0.2](https://github.com/SpeedySpec/speedyspec-server/compare/speedyspec-php-runtime-v1.0.1...speedyspec-php-runtime-v1.0.2) (2026-11-21)

## 1.0.1 (2026-11-21)
