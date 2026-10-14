use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn files(directory: &Path, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read frontend build directory") {
        let path = entry.expect("read frontend asset").path();
        if path.is_dir() {
            files(&path, output);
        } else {
            output.push(path);
        }
    }
}

fn main() {
    let frontend = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend");
    let directory = frontend.join("dist");
    println!("cargo:rerun-if-changed={}", directory.display());
    assert!(directory.join("index.html").is_file(), "dashboard assets are missing; run `cd frontend && vp install --frozen-lockfile && vp run build` before Cargo");
    // Catch a forgotten frontend rebuild during local Cargo development.
    let built_at = fs::metadata(directory.join("index.html"))
        .expect("frontend index")
        .modified()
        .expect("frontend build time");
    let mut inputs = Vec::new();
    files(&frontend.join("src"), &mut inputs);
    for name in [
        "package.json",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        "vite.config.ts",
        "svelte.config.js",
        "index.html",
        "tsconfig.json",
        "tsconfig.node.json",
    ] {
        inputs.push(frontend.join(name));
    }
    inputs.push(frontend.join("../crates/keryx-render/assets/keryx-logo.svg"));
    println!("cargo:rerun-if-changed={}", frontend.join("src").display());
    for path in inputs {
        println!("cargo:rerun-if-changed={}", path.display());
        let changed_at = fs::metadata(&path)
            .expect("frontend source file")
            .modified()
            .expect("frontend source time");
        assert!(
            changed_at <= built_at,
            "frontend source {} changed; run `cd frontend && vp run build` before Cargo",
            path.display()
        );
    }
    let mut assets = Vec::new();
    files(&directory, &mut assets);
    assets.sort();
    let mut source = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for path in assets {
        let relative = path
            .strip_prefix(&directory)
            .expect("asset within frontend build")
            .to_str()
            .expect("UTF-8 asset path")
            .replace('\\', "/");
        let absolute = path.canonicalize().expect("resolve frontend asset");
        source.push_str(&format!(
            "({relative:?}, include_bytes!({:?})),\n",
            absolute.to_str().expect("UTF-8 frontend build path")
        ));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").expect("Cargo OUT_DIR")).join("frontend.rs"),
        source,
    )
    .expect("write embedded frontend asset map");
}
