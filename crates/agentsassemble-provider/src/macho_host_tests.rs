use std::{fs, path::Path, process::Command};

#[test]
fn staged_loader_retains_its_declared_library_after_source_removal() {
    let fixture = tempfile::tempdir().unwrap_or_else(|e| panic!("fixture: {e}"));
    let root = fixture
        .path()
        .canonicalize()
        .unwrap_or_else(|e| panic!("path: {e}"));
    fs::create_dir(root.join("bin")).unwrap_or_else(|e| panic!("bin: {e}"));
    fs::create_dir(root.join("lib")).unwrap_or_else(|e| panic!("lib: {e}"));
    fs::write(root.join("library.c"), "int answer(void) { return 42; }")
        .unwrap_or_else(|e| panic!("source: {e}"));
    fs::write(root.join("main.c"), "#include <stdio.h>\nextern int answer(void); int main(void) { printf(\"%d\\n\", answer()); }").unwrap_or_else(|e| panic!("source: {e}"));
    compile(
        &root,
        &[
            "-dynamiclib",
            "library.c",
            "-Wl,-install_name,@rpath/libanswer.dylib",
            "-o",
            "lib/libanswer.dylib",
        ],
    );
    compile(
        &root,
        &[
            "main.c",
            "-Llib",
            "-lanswer",
            "-Wl,-rpath,@loader_path/../lib",
            "-o",
            "bin/host",
        ],
    );
    let source = root.join("bin/host");
    let identity =
        super::super::executable_identity_sync(&source).unwrap_or_else(|e| panic!("identity: {e}"));
    let bound = super::bind(&source, &identity).unwrap_or_else(|e| panic!("bind: {e}"));
    fs::remove_file(root.join("lib/libanswer.dylib"))
        .unwrap_or_else(|e| panic!("remove source library: {e}"));
    let output = Command::new(bound.launch_path())
        .output()
        .unwrap_or_else(|e| panic!("run: {e}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
    assert!(super::bind(&source, &identity).is_err());
    let private_entry = bound.launch_path().to_owned();
    drop(bound);
    assert!(!Path::new(&private_entry).exists());
}

#[test]
fn loader_destinations_cannot_escape_the_private_root() {
    let root = Path::new("/private/staging");
    assert!(super::confined(root, &root.join("image/bin/../../../outside")).is_err());
    assert!(super::confined(root, Path::new("/other/lib.dylib")).is_err());
}

fn compile(root: &Path, arguments: &[&str]) {
    let output = Command::new("/usr/bin/cc")
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap_or_else(|e| panic!("compile loader fixture: {e}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
