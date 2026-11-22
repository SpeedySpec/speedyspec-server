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

fn main() {
    let release = speedyspec_compat::wordpress_release("7.1.5").expect("WordPress 7.1.5");
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
}
