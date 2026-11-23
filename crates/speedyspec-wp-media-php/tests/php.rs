//! WordPress's media functions in embedded PHP: the facade over the natives, with WordPress 7.1.5's own functions
//! (kses among them) loaded around it, reading in-process through the sniffing reader.
//!
//! PHP boots once per process, so this test has its own `main()` (`harness = false`) and runs each case in order.

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php, PhpError};
use speedyspec_wp_hooks::HookValue;
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_media::SniffingReader;
use speedyspec_wp_media_contracts::ProbeLimits;
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_midi::MidiReader;
use speedyspec_wp_media_php::MediaArea;
use speedyspec_wp_media_testkit as kit;

fn check(name: &str, ok: bool, detail: impl std::fmt::Debug) {
    if ok {
        println!("ok: {name}");
    } else {
        eprintln!("FAIL: {name}: {detail:?}");
        std::process::exit(1);
    }
}

fn text(result: &Result<HookValue, PhpError>) -> String {
    match result {
        Ok(HookValue::Str(s)) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn json(result: &Result<HookValue, PhpError>) -> serde_json::Value {
    serde_json::from_str(&text(result)).unwrap_or(serde_json::Value::Null)
}

/// WordPress 7.1.5 without the functions the media area declares natively.
fn speedyspec_media_release() -> std::path::PathBuf {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
    speedyspec_wp_media_php::prepare_release(
        &release,
        &Path::new(env!("CARGO_TARGET_TMPDIR")).join("wordpress-media-php"),
    )
    .expect("the release is prepared")
}

fn main() {
    let release = speedyspec_media_release();
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media/corpus");
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let limits = ProbeLimits::default();
    let media = MediaArea::new(hooks.api(), container.clone(), limits.clone());
    media.bind_defaults(Rc::new(SniffingReader::new(
        Box::new(MidiReader),
        Box::new(FfmpegReader::new(&limits)),
    )));
    let container_area = media.ports(ContainerArea::new("test", container.clone()));
    let areas: Vec<Rc<dyn NativeArea>> = vec![
        speedyspec_php_ext::twins_area(),
        Rc::new(container_area),
        Rc::new(hooks),
        Rc::new(media),
    ];
    let php = Php::boot(BootOptions {
        hooks: Rc::new(MemoryHooks::new()),
        preload: &[],
        areas,
    })
    .expect("PHP boots");

    let load = format!(
        "define('ABSPATH', {:?}); define('WPINC', 'wp-includes'); error_reporting(E_ALL & ~E_DEPRECATED);
         add_filter('pre_option', function ($pre, $option, $default) {{ return ['blog_charset' => 'UTF-8'][$option] ?? ($default === false ? '' : $default); }}, 10, 3);
         spl_autoload_register(function ($c) {{ $f = ABSPATH . WPINC . '/html-api/class-' . strtolower(str_replace('_', '-', $c)) . '.php'; if (is_file($f)) {{ require $f; }} }});
         foreach (array('load.php', 'formatting.php', 'kses.php', 'functions.php', 'class-wp-token-map.php') as $f) {{ require ABSPATH . WPINC . '/' . $f; }}",
        format!("{}/", release.display())
    );
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");

    let wav = kit::write_temp(&kit::wav_pcm16(1.0, 8000, 1), "wav");
    let wav_path = wav.path().display().to_string();
    let mp4 = corpus.join("bars.mp4").display().to_string();
    let mkv = corpus.join("bars.mkv").display().to_string();
    let latin1 = corpus.join("latin1-v1.mp3").display().to_string();

    let r = php.eval(
        "speedyspec://test/native",
        "return (new ReflectionFunction('wp_read_audio_metadata'))->getFileName();",
    );
    check(
        "functions_are_the_facade",
        text(&r) == "speedyspec://wp-media",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/audio",
        &format!("return json_encode(wp_read_audio_metadata({wav_path:?}));"),
    );
    let v = json(&r);
    check(
        "audio_metadata_from_php",
        v["length"] == 1 && v["fileformat"] == "wav" && v["sample_rate"] == 8000,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/video",
        &format!("return json_encode(wp_read_video_metadata({mp4:?}));"),
    );
    let v = json(&r);
    check(
        "video_metadata_from_php",
        v["width"] == 160
            && v["height"] == 120
            && v["fileformat"] == "mp4"
            && v["created_timestamp"] == 1_714_979_289,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/missing",
        "return var_export(wp_read_audio_metadata('/nonexistent/x.mp3'), true);",
    );
    check("false_for_missing_file", text(&r) == "false", &r);

    let r = php.eval(
        "speedyspec://test/filter",
        &format!(
            "add_filter('wp_read_audio_metadata', function ($m, $f, $ff, $d) {{ $GLOBALS['seen'] = [func_num_args(), $f, $ff, $d['speedyspec']['schema'] ?? null, $d['fileformat'] ?? null]; return 'x'; }}, 10, 4);
             $r = wp_read_audio_metadata({wav_path:?});
             remove_all_filters('wp_read_audio_metadata');
             return json_encode([$r, $GLOBALS['seen']]);"
        ),
    );
    let v = json(&r);
    check(
        "filter_sees_four_arguments_and_wins",
        v[0] == "x"
            && v[1][0] == 4
            && v[1][1] == wav_path.as_str()
            && v[1][2] == "wav"
            && v[1][4] == "wav",
        &r,
    );
    check("speedyspec_key_has_schema_1", v[1][3] == 1, &r);

    let r = php.eval(
        "speedyspec://test/id3",
        "$m = array('keep' => 1); wp_add_id3_tag_data($m, array('id3v2' => array('comments' => array('title' => array('<b>T</b><script>x</script>'), 'length' => array('9'))), 'id3v2x' => 1));
         return json_encode($m);",
    );
    let v = json(&r);
    check(
        "add_id3_tag_data_by_reference",
        v["keep"] == 1 && v["title"] == "<b>T</b>x" && v.get("length").is_none(),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/ts",
        "return var_export(wp_get_media_creation_timestamp(array('fileformat' => 'matroska', 'matroska' => array('comments' => array('creation_time' => array('2024-05-06T07:08:09Z'))))), true);",
    );
    check(
        "creation_timestamp_matroska_string",
        text(&r) == "1714979289",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/mkv",
        &format!("return json_encode(wp_read_video_metadata({mkv:?}));"),
    );
    check(
        "matroska_creation_time_from_the_file",
        json(&r)["created_timestamp"] == 1_714_979_289,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/latin1",
        &format!("$m = wp_read_audio_metadata({latin1:?}); return bin2hex($m['title']);"),
    );
    check("non_utf8_title_survives", text(&r) == "436166c3a9", &r);

    let r = php.eval(
        "speedyspec://test/file-url",
        &format!("return json_encode(wp_read_audio_metadata('file://' . {wav_path:?}));"),
    );
    check(
        "file_url_paths_are_read",
        json(&r)["fileformat"] == "wav",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/relative",
        &format!("$cwd = getcwd(); chdir(dirname({wav_path:?})); $m = wp_read_audio_metadata(basename({wav_path:?})); chdir($cwd); return json_encode($m);"),
    );
    check(
        "relative_paths_are_read",
        json(&r)["fileformat"] == "wav",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/id3-untouched",
        "$o = new ArrayObject(); $m = array('x' => $o); wp_add_id3_tag_data($m, array('id3v1' => array('comments' => array('title' => array('T')))));
         $n = null; wp_add_id3_tag_data($n, array());
         return json_encode([$m['x'] === $o, $m['title'], $n === null]);",
    );
    check(
        "add_id3_tag_data_leaves_other_values_alone",
        text(&r) == r#"[true,"T",true]"#,
        &r,
    );

    let images = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media/images");
    let img = |name: &str| images.join(name).display().to_string();

    let r = php.eval(
        "speedyspec://test/fast-path",
        &format!("return json_encode(wp_getimagesize({0:?})) === json_encode(getimagesize({0:?})) ? 'same' : json_encode([wp_getimagesize({0:?}), getimagesize({0:?})]);", img("rgb.png")),
    );
    check("getimagesize_fast_path_is_phps", text(&r) == "same", &r);

    let r = php.eval(
        "speedyspec://test/image-info",
        &format!("$i = 'untouched'; wp_getimagesize({0:?}); $a = $i; $j = null; wp_getimagesize({0:?}, $j); return json_encode([$a, is_array($j)]);", img("baseline.jpg")),
    );
    check(
        "image_info_only_when_passed",
        text(&r) == r#"["untouched",true]"#,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/app13",
        &format!("$i = null; wp_getimagesize({:?}, $i); return isset($i['APP13']) ? 'yes' : json_encode(array_keys((array) $i));", img("iptc.jpg")),
    );
    check("jpeg_app13_reaches_image_info", text(&r) == "yes", &r);

    let r = php.eval(
        "speedyspec://test/stale-info",
        &format!(
            "$i = array('APP13' => 'old'); wp_getimagesize({:?}, $i); return json_encode($i);",
            img("photo.qoi")
        ),
    );
    check(
        "image_info_is_reset_on_the_reader_path",
        text(&r) == "[]",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/jxl",
        &format!(
            "return json_encode(wp_getimagesize({:?}));",
            img("photo.jxl")
        ),
    );
    let v = json(&r);
    check(
        "reader_path_jxl",
        v["0"] == 64 && v["1"] == 48 && v["2"] == 0 && v["mime"] == "image/jxl",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/mislabelled",
        &format!(
            "return wp_get_image_mime({:?}) . ' ' . wp_getimagesize({:?})['mime'];",
            img("mislabelled.jpg"),
            img("mislabelled.jpg")
        ),
    );
    check(
        "mislabelled_png_is_png",
        text(&r) == "image/png image/png",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/broken-images",
        &format!("return json_encode([wp_getimagesize({0:?}), wp_getimagesize({1:?}), wp_get_image_mime({1:?})]);", img("truncated.jpg"), img("empty.png")),
    );
    check(
        "truncated_and_empty_are_false",
        text(&r) == "[false,false,false]",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/webp-info",
        &format!("return json_encode([wp_get_webp_info({:?}), wp_get_webp_info({:?}), wp_get_webp_info({:?})]);", img("lossy.webp"), img("lossless.webp"), img("rgb.png")),
    );
    let v = json(&r);
    check(
        "webp_info_matches_wordpress",
        v[0]["width"] == 64
            && v[0]["type"] == "lossy"
            && v[1]["type"] == "lossless"
            && v[2]["width"] == false,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/avif-info",
        &format!(
            "return json_encode(wp_get_avif_info({:?}));",
            img("lossy.avif")
        ),
    );
    let v = json(&r);
    check(
        "avif_info_matches_wordpress",
        v["width"] == 64 && v["height"] == 48 && v["bit_depth"] == 8 && v["num_channels"] == 3,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/qoi-mime",
        &format!("return wp_get_image_mime({:?});", img("photo.qoi")),
    );
    check("image_mime_for_reader_types", text(&r) == "image/qoi", &r);

    let r = php.eval(
        "speedyspec://test/php-reader",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, fn () => new class implements \\SpeedySpec\\WpMedia\\MediaReader {{
                 public function read($path) {{ return ['schema' => 1, 'container' => 'mp3', 'duration' => 61.4, 'file_size' => 7, 'streams' => [['type' => 'audio', 'codec' => 'mp3', 'sample_rate' => 22050, 'channels' => 1]], 'tags' => [['key' => 'title', 'system' => 'id3v2', 'values' => ['From PHP']]], 'pictures' => []]; }}
             }});
             return json_encode(wp_read_audio_metadata({wav_path:?}));"
        ),
    );
    let v = json(&r);
    check(
        "php_reader_replaces_the_default",
        v["length_formatted"] == "1:01" && v["sample_rate"] == 22050 && v["title"] == "From PHP",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/delegate",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, fn () => new class implements \\SpeedySpec\\WpMedia\\MediaReader {{
                 public function read($path) {{ $n = \\SpeedySpec\\WpMedia\\Native\\read_media($path); $n['tags'][] = ['key' => 'title', 'system' => 'id3v2', 'values' => ['Wrapped']]; return $n; }}
             }});
             return json_encode(wp_read_audio_metadata({wav_path:?}));"
        ),
    );
    let v = json(&r);
    check(
        "php_reader_can_delegate_to_read_media",
        v["title"] == "Wrapped" && v["sample_rate"] == 8000,
        &r,
    );

    let r = php.eval(
        "speedyspec://test/malformed",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, fn () => new class implements \\SpeedySpec\\WpMedia\\MediaReader {{
                 public function read($path) {{ return ['schema' => 7]; }}
             }});
             return json_encode(wp_read_audio_metadata({wav_path:?}));"
        ),
    );
    let v = json(&r);
    check(
        "malformed_php_reader_array_is_unreadable",
        v.as_object()
            .is_some_and(|m| m.len() == 1 && m.contains_key("filesize")),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/throws",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, fn () => new class implements \\SpeedySpec\\WpMedia\\MediaReader {{
                 public function read($path) {{ throw new RuntimeException('reader failed'); }}
             }});
             try {{ wp_read_audio_metadata({wav_path:?}); return 'no exception'; }} catch (RuntimeException $e) {{ return $e->getMessage(); }}"
        ),
    );
    check(
        "php_reader_exception_propagates",
        text(&r) == "reader failed",
        &r,
    );
    php.restart_request().expect("a fresh request");
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");

    let r = php.eval(
        "speedyspec://test/wrapper",
        &format!(
            "class MemWrap {{
                 public $context; private $pos = 0;
                 public function stream_open($p, $m, $o, &$op) {{ return true; }}
                 public function stream_read($n) {{ $d = substr($GLOBALS['wav'], $this->pos, $n); $this->pos += strlen($d); return $d; }}
                 public function stream_eof() {{ return $this->pos >= strlen($GLOBALS['wav']); }}
                 public function stream_stat() {{ return ['size' => strlen($GLOBALS['wav'])]; }}
                 public function url_stat($p, $f) {{ return ['size' => strlen($GLOBALS['wav']), 'mode' => 0100644]; }}
             }}
             stream_wrapper_register('memwrap', 'MemWrap');
             $GLOBALS['wav'] = file_get_contents({wav_path:?});
             return json_encode(wp_read_audio_metadata('memwrap://a.wav'));"
        ),
    );
    let v = json(&r);
    check(
        "stream_wrapper_path_is_copied",
        v["fileformat"] == "wav" && v["length"] == 1,
        &r,
    );

    // The MIME list and filetype checks (plan 5).
    let media_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media");
    let at = |rel: &str| media_dir.join(rel).display().to_string();

    let r = php.eval(
        "speedyspec://test/mime-types",
        "$t = wp_get_mime_types(); $k = array_keys($t);
         return json_encode([$t['caf'] ?? null, $t['jpg|jpeg|jpe'], $k[array_search('mka', $k) + 1], $k[array_search('heifs', $k) + 1]]);",
    );
    let v = json(&r);
    check(
        "mime_types_keep_wordpress_and_add_rows",
        v[0] == "audio/x-caf" && v[1] == "image/jpeg" && v[2] == "aif|aiff|aifc" && v[3] == "jp2",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/mime-filters",
        "add_filter('mime_types', function () { return array('x' => 'y'); });
         add_filter('ext2type', function ($t) { $t['made'] = array('foo'); return $t; });
         $r = [wp_get_mime_types(), wp_ext2type('FOO'), wp_ext2type('caf')];
         remove_all_filters('mime_types'); remove_all_filters('ext2type');
         return json_encode($r);",
    );
    let v = json(&r);
    check(
        "mime_types_and_ext2type_filters_run",
        v[0] == serde_json::json!({"x": "y"}) && v[1] == "made" && v[2] == "audio",
        &r,
    );

    let r = php.eval(
        "speedyspec://test/allowed",
        // A function declared in the same script exists before it runs, so the capability is declared afterwards.
        "$GLOBALS['without'] = get_allowed_mime_types();
         eval('function current_user_can($cap) { return $cap === \"unfiltered_html\"; }');
         $without = $GLOBALS['without'];
         $with = get_allowed_mime_types();
         return json_encode([isset($without['htm|html']), isset($without['js']), isset($without['swf']), isset($without['caf']), isset($with['htm|html']), isset($with['js']), isset($with['exe'])]);",
    );
    check(
        "allowed_mime_types_drop_html_and_js_without_capability",
        json(&r) == serde_json::json!([false, false, false, true, true, true, false]),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/regex-key",
        "add_filter('upload_mimes', function ($m) { return $m + array('svg|svgz' => 'image/svg+xml', 'jpe?g2' => 'image/x-test'); });
         $r = [wp_check_filetype('a.SVGZ'), wp_check_filetype('b.jpg2'), wp_check_filetype('c.jpeg2'), wp_check_filetype('d.jpeeg2')];
         remove_all_filters('upload_mimes');
         return json_encode($r);",
    );
    check(
        "check_filetype_reads_a_plugins_regex_key",
        json(&r)
            == serde_json::json!([
                {"ext": "SVGZ", "type": "image/svg+xml"},
                {"ext": "jpg2", "type": "image/x-test"},
                {"ext": "jpeg2", "type": "image/x-test"},
                {"ext": false, "type": false}
            ]),
        &r,
    );

    let filetype = |file: &str, name: &str| {
        php.eval(
            "speedyspec://test/filetype",
            &format!("return json_encode(wp_check_filetype_and_ext({file:?}, {name:?}));"),
        )
    };

    let r = filetype(&at("uploads/tone.caf"), "tone.caf");
    check(
        "filetype_and_ext_accepts_added_formats",
        json(&r)
            == serde_json::json!({"ext": "caf", "type": "audio/x-caf", "proper_filename": false}),
        &r,
    );

    let mka = filetype(&at("corpus/tone.mka"), "tone.mka");
    let wma = filetype(&at("corpus/tone.wma"), "tone.wma");
    check(
        "audio_only_matroska_is_accepted",
        json(&mka)
            == serde_json::json!({"ext": "mka", "type": "audio/x-matroska", "proper_filename": false})
            && json(&wma)
                == serde_json::json!({"ext": "wma", "type": "audio/x-ms-wma", "proper_filename": false}),
        (&mka, &wma),
    );

    let html = kit::write_temp(
        b"<!DOCTYPE html><html><body><script>alert(1)</script></body></html>\n",
        "mp3",
    );
    let r = filetype(&html.path().display().to_string(), "song.mp3");
    check(
        "html_named_mp3_is_rejected",
        json(&r) == serde_json::json!({"ext": false, "type": false, "proper_filename": false}),
        &r,
    );

    let r = filetype(&at("images/mislabelled.jpg"), "mislabelled.jpg");
    check(
        "png_named_jpg_gets_proper_filename",
        json(&r)
            == serde_json::json!({"ext": "png", "type": "image/png", "proper_filename": "mislabelled.png"}),
        &r,
    );

    let r = filetype(&at("images/photo.jxl"), "photo.png");
    check(
        "jxl_named_png_becomes_jxl",
        json(&r)
            == serde_json::json!({"ext": "jxl", "type": "image/jxl", "proper_filename": "photo.jxl"}),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/filetype-wrapper",
        &format!(
            "class MkaWrap {{
                 public $context; private $pos = 0;
                 public function stream_open($p, $m, $o, &$op) {{ return true; }}
                 public function stream_read($n) {{ $d = substr($GLOBALS['mka'], $this->pos, $n); $this->pos += strlen($d); return $d; }}
                 public function stream_eof() {{ return $this->pos >= strlen($GLOBALS['mka']); }}
                 public function stream_seek($o, $w) {{ $this->pos = $w === SEEK_SET ? $o : ($w === SEEK_CUR ? $this->pos + $o : strlen($GLOBALS['mka']) + $o); return true; }}
                 public function stream_tell() {{ return $this->pos; }}
                 public function stream_stat() {{ return ['size' => strlen($GLOBALS['mka']), 'mode' => 0100644]; }}
                 public function url_stat($p, $f) {{ return ['size' => strlen($GLOBALS['mka']), 'mode' => 0100644]; }}
             }}
             stream_wrapper_register('mkawrap', 'MkaWrap');
             $GLOBALS['mka'] = file_get_contents({:?});
             return json_encode(wp_check_filetype_and_ext('mkawrap://tone.mka', 'tone.mka'));",
            at("corpus/tone.mka")
        ),
    );
    check(
        "stream_wrapper_path_uses_fileinfo",
        json(&r) == serde_json::json!({"ext": false, "type": false, "proper_filename": false}),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/default-ext",
        "return json_encode([wp_get_default_extension_for_mime_type('audio/mpeg'), wp_get_default_extension_for_mime_type('video/mpeg'), wp_get_default_extension_for_mime_type('audio/x-caf'), wp_get_default_extension_for_mime_type('wibble/wobble')]);",
    );
    check(
        "default_extension_unchanged_for_shared_mimes",
        json(&r) == serde_json::json!(["mp3", "mpeg", "caf", false]),
        &r,
    );

    let wp_data =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/php/wordpress/media/data/uploads");
    let phone = wp_data
        .join("audio-in-video-container.m4a")
        .display()
        .to_string();
    let csv = wp_data.join("test.csv").display().to_string();
    let r = php.eval(
        "speedyspec://test/real-mime",
        &format!(
            "add_filter('wp_check_filetype_and_ext', function ($r, $f, $n, $m, $real) {{ $GLOBALS['real'][] = $real; return $r; }}, 10, 5);
             $GLOBALS['real'] = array();
             $phone = wp_check_filetype_and_ext({phone:?}, 'phone.m4a');
             $mka = wp_check_filetype_and_ext({:?}, 'tone.mka');
             remove_all_filters('wp_check_filetype_and_ext');
             return json_encode([$phone['type'], $GLOBALS['real'][0] === finfo_file(finfo_open(FILEINFO_MIME_TYPE), {phone:?}), $mka['type'], $GLOBALS['real'][1]]);",
            at("corpus/tone.mka")
        ),
    );
    check(
        "real_mime_is_fileinfos_where_wordpress_accepts",
        json(&r) == serde_json::json!(["audio/mpeg", true, "audio/x-matroska", "audio/x-matroska"]),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/throwing-reader",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, fn () => new class implements \\SpeedySpec\\WpMedia\\MediaReader {{
                 public function read($path) {{ throw new RuntimeException('unknown format'); }}
             }});
             try {{
                 return json_encode([wp_check_filetype_and_ext({:?}, 'tone.mka'), wp_check_filetype_and_ext({csv:?}, 'test.csv')['type']]);
             }} catch (Throwable $e) {{ return 'threw: ' . $e->getMessage(); }}",
            at("corpus/tone.mka")
        ),
    );
    check(
        "a_throwing_reader_leaves_fileinfos_answer",
        json(&r)
            == serde_json::json!([{"ext": false, "type": false, "proper_filename": false}, "text/csv"]),
        &r,
    );

    let r = php.eval(
        "speedyspec://test/unresolvable-reader",
        &format!(
            "\\SpeedySpec\\Container\\Container::getInstance()->bind(\\SpeedySpec\\WpMedia\\MediaReader::class, function () {{ throw new RuntimeException('no reader'); }});
             try {{
                 return json_encode([wp_check_filetype_and_ext({csv:?}, 'test.csv')['type'], wp_check_filetype_and_ext({:?}, 'tone.mka')['type']]);
             }} catch (Throwable $e) {{ return 'threw: ' . $e->getMessage(); }}",
            at("corpus/tone.mka")
        ),
    );
    check(
        "an_unresolvable_reader_leaves_fileinfos_answer",
        json(&r) == serde_json::json!(["text/csv", false]),
        &r,
    );

    // One fileinfo handle per request: it answers every check, and a new request gets a new one.
    let twice = format!(
        "return json_encode([wp_check_filetype_and_ext({csv:?}, 'a.csv')['type'], wp_check_filetype_and_ext({csv:?}, 'b.csv')['type']]);"
    );
    let first = php.eval("speedyspec://test/finfo-twice", &twice);
    php.restart_request().expect("a fresh request");
    php.eval("speedyspec://test/load", &load)
        .expect("WordPress's functions load");
    let again = php.eval("speedyspec://test/finfo-again", &twice);
    check(
        "fileinfo_handle_is_kept_per_request",
        json(&first) == serde_json::json!(["text/csv", "text/csv"]) && json(&again) == json(&first),
        (&first, &again),
    );

    // A stream wrapper fileinfo can read (it hands over a real stream): fileinfo's answer decides, as in WordPress,
    // and the reader isn't consulted for a path it can't open.
    let r = php.eval(
        "speedyspec://test/readable-wrapper",
        &format!(
            "class FileWrap {{
                 public $context; private $h;
                 private static function real($p) {{ return $GLOBALS['wrapped'][substr($p, strlen('filewrap://'))]; }}
                 public function stream_open($p, $m, $o, &$op) {{ $this->h = fopen(self::real($p), 'rb'); return (bool) $this->h; }}
                 public function stream_read($n) {{ return fread($this->h, $n); }}
                 public function stream_eof() {{ return feof($this->h); }}
                 public function stream_seek($o, $w) {{ return fseek($this->h, $o, $w) === 0; }}
                 public function stream_tell() {{ return ftell($this->h); }}
                 public function stream_stat() {{ return fstat($this->h); }}
                 public function stream_cast($as) {{ return $this->h; }}
                 public function url_stat($p, $f) {{ return @stat(self::real($p)); }}
             }}
             stream_wrapper_register('filewrap', 'FileWrap');
             $GLOBALS['wrapped'] = array('test.csv' => {csv:?}, 'tone.mka' => {:?});
             return json_encode([wp_check_filetype_and_ext('filewrap://test.csv', 'test.csv'), wp_check_filetype_and_ext('filewrap://tone.mka', 'tone.mka')]);",
            at("corpus/tone.mka")
        ),
    );
    check(
        "readable_stream_wrapper_uses_fileinfos_answer",
        json(&r)
            == serde_json::json!([
                {"ext": "csv", "type": "text/csv", "proper_filename": false},
                {"ext": false, "type": false, "proper_filename": false}
            ]),
        &r,
    );
}
