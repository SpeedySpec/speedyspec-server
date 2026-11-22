//! The static PHP has every extension WordPress uses: Site Health's list, plus what WordPress otherwise relies on.

use std::rc::Rc;

use speedyspec_hooks::Value;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, Php};

const EXTENSIONS: &[&str] = &[
    "bcmath",
    "ctype",
    "curl",
    "dom",
    "exif",
    "fileinfo",
    "filter",
    "gd",
    "hash",
    "iconv",
    "imagick",
    "intl",
    "json",
    "libxml",
    "mbstring",
    "mysqli",
    "mysqlnd",
    "Zend OPcache",
    "openssl",
    "pcre",
    "pdo",
    "pdo_mysql",
    "pdo_sqlite",
    "Phar",
    "session",
    "SimpleXML",
    "sodium",
    "sqlite3",
    "tokenizer",
    "xml",
    "xmlreader",
    "xmlwriter",
    "zip",
    "zlib",
];

/// The libsodium version `scripts/build-php.sh` pins. WordPress needs ext-sodium, and the runtime guarantees it, so
/// the build must link exactly this one.
const SODIUM: &str = "1.0.22";

fn main() {
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas: Vec::new(),
    })
    .expect("PHP boots");
    let list = EXTENSIONS
        .iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(",");
    let probe = format!(
        r#"
        $missing = [];
        foreach ([{list}] as $e) {{ if (!extension_loaded($e)) {{ $missing[] = $e; }} }}
        foreach (['curl_version','exif_read_data','finfo_file','hash','json_last_error','mb_check_encoding',
                  'mysqli_connect','openssl_encrypt','preg_match','filter_list','iconv'] as $f) {{
            if (!function_exists($f)) {{ $missing[] = "function $f"; }}
        }}
        foreach (['DOMNode','ZipArchive','Imagick','IntlChar','PDO','SQLite3'] as $c) {{
            if (!class_exists($c)) {{ $missing[] = "class $c"; }}
        }}
        if (!defined('SODIUM_LIBRARY_VERSION')) {{ $missing[] = 'constant SODIUM_LIBRARY_VERSION'; }}
        elseif (SODIUM_LIBRARY_VERSION !== '{SODIUM}') {{
            $missing[] = 'sodium: expected libsodium {SODIUM}, got ' . SODIUM_LIBRARY_VERSION;
        }}
        if (!function_exists('sodium_crypto_sign_verify_detached')) {{ $missing[] = 'function sodium_crypto_sign_verify_detached'; }}
        if (function_exists('gd_info')) {{
            $gd = gd_info();
            foreach (['JPEG Support','PNG Support','WebP Support','AVIF Support','FreeType Support'] as $k) {{
                if (empty($gd[$k])) {{ $missing[] = "gd $k"; }}
            }}
        }}
        return implode(', ', $missing);
        "#
    );
    let missing = php.eval("extensions", &probe).expect("the probe runs");
    if missing != Value::Str(String::new()) {
        eprintln!("missing: {missing:?}");
        std::process::exit(1);
    }
    println!("every WordPress extension loads");
}
