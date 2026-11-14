use std::{error::Error, process::Command};

/// The commit of this build: the window shows its start, and links to the code as it stood then.
fn commit() -> String {
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output();
    output.ok().and_then(|output| String::from_utf8(output.stdout).ok()).unwrap_or_default().trim().to_string()
}

/// The icon of the program file itself, which only Windows reads from there. A build script runs
/// on the machine that builds, so `cfg(windows)` would ask about that machine: the system the
/// program is for is in `CARGO_CFG_TARGET_OS`.
fn windows_icon() -> Result<(), Box<dyn Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|system| system == "windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("icons/icon.ico");
        resource.compile()?;
    }
    Ok(())
}

/// What the program for this system is built from, taken from `components/` before the build: the list, and the
/// license texts of those components, compressed. Both are written by `scripts/components.py`.
fn components() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=components/list.tsv");
    println!("cargo:rerun-if-changed=components/notices.txt");
    let system = match std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default().as_str() {
        "macos" => 'm',
        "windows" => 'w',
        _ => 'l',
    };
    // A checkout with CRLF must read the same: a component's name ending in `\r` would find no license text.
    let list = std::fs::read_to_string("components/list.tsv")?.replace("\r\n", "\n");
    let mut here = std::collections::BTreeSet::new();
    let mut kept = String::new();
    for line in list.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if let [name, version, _, _, systems] = fields.as_slice()
            && systems.contains(system)
        {
            here.insert(format!("{name} {version}"));
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let notices = std::fs::read_to_string("components/notices.txt")?.replace("\r\n", "\n");
    let mut texts = String::new();
    for block in notices.split("\n== ").map(|block| block.trim_start_matches("== ")) {
        let (carriers, _) = block.split_once('\n').unwrap_or((block, ""));
        if carriers.split(", ").any(|carrier| here.contains(carrier)) {
            texts.push_str("== ");
            texts.push_str(block.trim_end());
            texts.push_str("\n\n");
        }
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::write(out.join("components.tsv"), kept)?;
    std::fs::write(out.join("notices.xz"), liblzma::encode_all(texts.as_bytes(), 9)?)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    windows_icon()?;
    components()?;
    println!("cargo:rustc-env=BUILD_COMMIT={}", commit());
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
    compile_window()
}

/// Slint's compiler walks the window's tree deeply enough to overflow a main thread of 1 MB, which is what Windows
/// gives one, and a Windows build failed so. It runs on a thread of its own, with room to spare.
const COMPILER_STACK: usize = 64 * 1024 * 1024;

fn compile_window() -> Result<(), Box<dyn Error>> {
    let compiler = std::thread::Builder::new().name("slint".to_string()).stack_size(COMPILER_STACK).spawn(|| {
        let config = slint_build::CompilerConfiguration::new()
            .with_style("fluent".to_string())
            .with_bundled_translations("lang");
        slint_build::compile_with_config("ui/app.slint", config).map_err(|error| error.to_string())
    })?;
    match compiler.join() {
        Ok(result) => Ok(result?),
        Err(_) => Err("the window's compiler stopped".into()),
    }
}
