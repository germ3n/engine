fn main() {
    let map = map_arg().unwrap_or_else(|| "hall".to_string());
    base::editor(&map);
}

fn map_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    let mut map = None;

    while let Some(arg) = args.next() {
        if arg == "--map" || arg == "-m" {
            map = args.next();

            continue;
        }

        if arg.starts_with('-') {
            continue;
        }

        map = Some(arg);
    }

    map
}
