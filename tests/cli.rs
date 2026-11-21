//! Tests boîte noire.
//! Le binaire est lancé sur de vrais dossiers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CARGO_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by cargo.\n\
# For information about cache directory tags see https://bford.info/cachedir/\n";
const FOREIGN_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by sccache.\n";

/// Dossier de test jetable, supprimé à la fin.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("test_cct_{}_{name}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("création du dossier de test");
        Sandbox(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, content).expect("write");
}

/// Un profil `debug/` réaliste : caches, métadonnées et un exécutable final.
fn fill_profile(root: &Path) {
    write(&root.join("debug/deps/libx-1.rmeta"), "m");
    write(&root.join("debug/.fingerprint/x-1/invoked.timestamp"), "");
    write(&root.join("debug/build/x-1/out"), "o");
    write(&root.join("debug/incremental/x-1/s"), "i");
    write(&root.join("debug/app.d"), "d");
    write(&root.join("debug/app"), "binaire");
    write(&root.join("debug/libx.rlib"), "r");
    write(&root.join("tmp/scratch"), "t");
}

fn run(args: &[&Path], flags: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-clean-targets"))
        .args(flags)
        .args(args)
        .output()
        .expect("lancement du binaire")
}

fn assert_cleaned(root: &Path) {
    assert!(!root.join("debug/deps").exists(), "deps/ doit partir");
    assert!(!root.join("debug/.fingerprint").exists());
    assert!(!root.join("debug/build").exists());
    assert!(!root.join("debug/incremental").exists());
    assert!(!root.join("debug/app.d").exists());
    assert!(!root.join("tmp").exists());
    assert!(root.join("debug/app").exists(), "l'exécutable reste");
    assert!(root.join("debug/libx.rlib").exists(), "la rlib reste");
}

fn assert_untouched(root: &Path) {
    assert!(root.join("debug/deps/libx-1.rmeta").exists());
    assert!(root.join("debug/.fingerprint").exists());
    assert!(root.join("tmp/scratch").exists());
}

#[test]
fn renamed_target_with_cargo_tag_is_cleaned() {
    let sb = Sandbox::new("tag");
    let dir = sb.path("fn-lot-c10a-target");
    fill_profile(&dir);
    write(&dir.join("CACHEDIR.TAG"), CARGO_TAG);
    let out = run(&[&dir], &[]);
    assert!(out.status.success(), "{out:?}");
    assert_cleaned(&dir);
    assert!(dir.join("CACHEDIR.TAG").exists());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("done: 1 target dir(s)"), "{stdout}");
}

#[test]
fn renamed_target_with_rustc_info_is_cleaned() {
    let sb = Sandbox::new("info");
    let dir = sb.path("cache-cargo");
    fill_profile(&dir);
    write(&dir.join(".rustc_info.json"), "{}");
    assert!(run(&[&dir], &[]).status.success());
    assert_cleaned(&dir);
}

#[test]
fn renamed_target_without_tag_is_recognised_by_its_structure() {
    let sb = Sandbox::new("structure");
    let dir = sb.path("fn-lot-c10e-target");
    fill_profile(&dir);
    assert!(run(&[&dir], &[]).status.success());
    assert_cleaned(&dir);
}

#[test]
fn renamed_target_with_triple_wrapper_and_tag_is_cleaned() {
    let sb = Sandbox::new("triple");
    let dir = sb.path("lot-x");
    fill_profile(&dir.join("x86_64-unknown-linux-gnu"));
    write(&dir.join("CACHEDIR.TAG"), CARGO_TAG);
    assert!(run(&[&dir], &[]).status.success());
    let triple = dir.join("x86_64-unknown-linux-gnu");
    assert!(!triple.join("debug/deps").exists());
    assert!(triple.join("debug/app").exists());
}

#[test]
fn parent_of_a_renamed_target_is_not_mistaken_for_one() {
    let sb = Sandbox::new("parent");
    let child = sb.path("projets/x-target");
    fill_profile(&child);
    let out = run(&[&sb.path("projets")], &[]);
    assert!(out.status.success());
    assert_untouched(&child);
}

#[test]
fn dry_run_removes_nothing_on_a_renamed_target() {
    let sb = Sandbox::new("dry");
    let dir = sb.path("fn-lot-c10a-target");
    fill_profile(&dir);
    write(&dir.join("CACHEDIR.TAG"), CARGO_TAG);
    let out = run(&[&dir], &["--dry"]);
    assert!(out.status.success());
    assert_untouched(&dir);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("would rm"), "{stdout}");
    assert!(stdout.contains("done: 1 target dir(s)"), "{stdout}");
}

#[test]
fn unmarked_directory_is_left_alone() {
    let sb = Sandbox::new("plain");
    let dir = sb.path("data");
    write(&dir.join("debug/deps/libx-1.rmeta"), "m");
    write(&dir.join("tmp/scratch"), "t");
    write(&dir.join("debug/.fingerprint/x-1/f"), "");
    // `.fingerprint` sans `deps/` n'est pas la structure d'un profil Cargo.
    fs::remove_dir_all(dir.join("debug/deps")).expect("rm deps");
    write(&dir.join("debug/other/file"), "x");
    let out = run(&[&dir], &[]);
    assert!(out.status.success());
    assert!(dir.join("debug/.fingerprint").exists());
    assert!(dir.join("tmp/scratch").exists());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("done: 0 target dir(s)"), "{stdout}");
}

#[test]
fn foreign_or_unsigned_cachedir_tag_is_not_enough() {
    let sb = Sandbox::new("foreign");
    for (name, tag) in [
        ("sccache-dir", FOREIGN_TAG),
        ("unsigned-dir", "pas une signature\n"),
    ] {
        let dir = sb.path(name);
        write(&dir.join("debug/deps/libx-1.rmeta"), "m");
        write(&dir.join("tmp/scratch"), "t");
        write(&dir.join("CACHEDIR.TAG"), tag);
        assert!(run(&[&dir], &[]).status.success());
        assert!(dir.join("debug/deps/libx-1.rmeta").exists(), "{name}");
        assert!(dir.join("tmp/scratch").exists(), "{name}");
    }
}

#[test]
fn a_renamed_target_below_the_argument_is_not_widened() {
    let sb = Sandbox::new("nested");
    let nested = sb.path("projects/fn-lot-target");
    fill_profile(&nested);
    write(&nested.join("CACHEDIR.TAG"), CARGO_TAG);
    let real = sb.path("projects/app/target");
    fill_profile(&real);
    write(&real.join("CACHEDIR.TAG"), CARGO_TAG);
    let out = run(&[&sb.path("projects")], &[]);
    assert!(out.status.success());
    assert_cleaned(&real);
    assert_untouched(&nested);
}

#[test]
fn directory_named_target_keeps_its_weak_signal() {
    let sb = Sandbox::new("named");
    let dir = sb.path("target");
    fill_profile(&dir);
    write(&dir.join(".rustc_info.json"), "{}");
    assert!(run(&[&dir], &[]).status.success());
    assert_cleaned(&dir);
}

#[test]
fn symlink_argument_is_never_followed() {
    let sb = Sandbox::new("link");
    let real = sb.path("real-target");
    fill_profile(&real);
    write(&real.join("CACHEDIR.TAG"), CARGO_TAG);
    let link = sb.path("lien-target");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    assert!(run(&[&link], &[]).status.success());
    assert_untouched(&real);
}
