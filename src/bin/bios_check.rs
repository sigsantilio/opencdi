// Checks CD-i system ROM files or zips and prints what OpenCDI makes of each: bios_check <file>...
use opencdi::machine::Machine;

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: bios_check <file>...");
        std::process::exit(2);
    }
    let mut status = 0;
    for f in &files {
        match Machine::read_bios(f) {
            Ok((entry, img)) => {
                let r = &img.report;
                let name = entry.map_or_else(|| f.clone(), |e| format!("{}:{}", f, e));
                let sha = if r.status == opencdi::bios::Status::Error { "SHA-1" } else { "SHA-1 in CPU order" };
                println!("{} [{}] {} ({} {})", name, r.status, r.message, sha, r.sha1);
                if !r.usable() {
                    status = 1;
                }
            }
            Err(e) => {
                println!("{} [error] {}", f, e);
                status = 1;
            }
        }
    }
    std::process::exit(status);
}
