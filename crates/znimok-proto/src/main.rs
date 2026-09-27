//! Phase 1 prototypes. `znimok-proto p1` opens the canvas prototype (ZK-14); `--headless`
//! renders the reference scene to a PNG without a window.

mod p1;

fn usage() -> ! {
    eprintln!(
        "usage: znimok-proto p1 [--headless] [--size WxH] [--image file.png] [--export out.png] [--scale N]\n\
         \n  --headless   render the reference scene 1:1 to --export (default target/p1.png) and exit\
         \n  --size WxH   synthetic screenshot size (default 1600x1000; use 3840x2160 for the 4K test)\
         \n  --image      use a real PNG as the screenshot instead of the synthetic one\
         \n  --scale N    initial zoom (default: fit)"
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };
    match cmd.as_str() {
        "p1" => {
            let mut opts = p1::Options::default();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--headless" => opts.headless = true,
                    "--size" => {
                        i += 1;
                        let (w, h) = args
                            .get(i)
                            .and_then(|s| s.split_once('x'))
                            .unwrap_or_else(|| usage());
                        opts.size = (
                            w.parse().unwrap_or_else(|_| usage()),
                            h.parse().unwrap_or_else(|_| usage()),
                        );
                    }
                    "--image" => {
                        i += 1;
                        opts.image = Some(args.get(i).cloned().unwrap_or_else(|| usage()));
                    }
                    "--export" => {
                        i += 1;
                        opts.export = Some(args.get(i).cloned().unwrap_or_else(|| usage()));
                    }
                    "--scale" => {
                        i += 1;
                        opts.scale = args.get(i).and_then(|s| s.parse().ok());
                    }
                    _ => usage(),
                }
                i += 1;
            }
            if let Err(e) = p1::run(opts) {
                eprintln!("p1: {e}");
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
}
