mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use speedyspec_server_domain::AppKind;
use support::{World, id, v};

fn world() -> World {
    let mut w = World::new();
    w.app("blog", AppKind::Php, 3);
    w.publish("blog", "1.0.0", "");
    w
}

#[test]
fn update_installs_and_switches_the_candidate() {
    let w = world();
    let manager = w.manager();
    assert_eq!(manager.update(&id("blog")).unwrap(), Some(v("1.0.0")));
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
    assert_eq!(manager.update(&id("blog")).unwrap(), None, "nothing newer");
}

#[test]
fn pin_during_download_wins_and_nothing_waits_for_the_download() {
    let mut w = world();
    let manager = Arc::new(w.manager());
    manager.update(&id("blog")).unwrap();
    w.publish("blog", "2.0.0", "");
    w.source.set_download_delay(Duration::from_millis(600));
    let background = {
        let manager = manager.clone();
        std::thread::spawn(move || manager.update(&id("blog")))
    };
    std::thread::sleep(Duration::from_millis(150));

    let started = Instant::now();
    let status = manager.status();
    manager.pin(&id("blog"), v("1.0.0")).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "status and pin waited {:?}",
        started.elapsed()
    );
    assert_eq!(status.apps[&id("blog")].active, Some(v("1.0.0")));

    assert_eq!(
        background.join().unwrap().unwrap(),
        None,
        "the pin set during the download stops the switch"
    );
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
    assert_eq!(w.saved().apps[&id("blog")].active, Some(v("1.0.0")));
}

#[test]
fn rollback_during_download_wins() {
    let mut w = world();
    let manager = Arc::new(w.manager());
    manager.update(&id("blog")).unwrap();
    w.publish("blog", "1.1.0", "");
    manager.update(&id("blog")).unwrap();
    w.publish("blog", "2.0.0", "");
    w.source.set_download_delay(Duration::from_millis(400));
    let background = {
        let manager = manager.clone();
        std::thread::spawn(move || manager.update(&id("blog")))
    };
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(manager.rollback(&id("blog"), None).unwrap(), v("1.0.0"));
    background.join().unwrap().unwrap();
    assert_eq!(w.runtime.active("blog"), Some(v("1.0.0")));
}
