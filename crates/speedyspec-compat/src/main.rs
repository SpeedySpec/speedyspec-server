//! `speedyspec-compat record <area> --wordpress <version> [--multisite]`: re-records an area's fixture (with
//! `--multisite`, its scenarios marked `multisite`, on a multisite install).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: speedyspec-compat record <area> --wordpress <version> [--multisite]";
    let (area, version, multisite) = match args.as_slice() {
        [cmd, area, flag, version] if cmd == "record" && flag == "--wordpress" => {
            (area, version, false)
        }
        [cmd, area, flag, version, ms]
            if cmd == "record" && flag == "--wordpress" && ms == "--multisite" =>
        {
            (area, version, true)
        }
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    match speedyspec_compat::record_for(area, version, multisite) {
        Ok(path) => println!("wrote {}", path.display()),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
