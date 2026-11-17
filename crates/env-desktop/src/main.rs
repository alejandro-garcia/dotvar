fn main() {
    let mut args = std::env::args_os().skip(1);
    let initial = args.next().map(std::path::PathBuf::from);
    if args.next().is_some() {
        eprintln!("{}", env_desktop::usage());
        std::process::exit(2);
    }
    env_desktop::run(initial);
}
