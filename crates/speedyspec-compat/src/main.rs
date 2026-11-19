//! `speedyspec-compat record <area> --wordpress <version>`: re-records an area's fixture.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: speedyspec-compat record <area> --wordpress <version>";
    let (area, version) = match args.as_slice() {
        [cmd, area, flag, version] if cmd == "record" && flag == "--wordpress" => (area, version),
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    match speedyspec_compat::record(area, version) {
        Ok(path) => println!("wrote {}", path.display()),
        Err(why) => {
            eprintln!("{why}");
            std::process::exit(1);
        }
    }
}
