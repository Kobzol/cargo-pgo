use crate::utils::{OutputExt, init_cargo_project};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn write_file(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, path.to_string_lossy().as_bytes()).unwrap();
}

fn write_contents(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn assert_contents(path: &Path, contents: &str) {
    assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
}

fn regular_files(directory: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

fn has_profile_suffix(path: &Path, extension: &str) -> bool {
    path.extension() == Some(std::ffi::OsStr::new(extension))
        || path.file_name() == Some(std::ffi::OsStr::new(&format!(".{extension}")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn non_utf8_profile_path(directory: &Path, suffix: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;

    let mut name = b"profile-\xff".to_vec();
    name.extend_from_slice(suffix);
    directory.join(std::ffi::OsString::from_vec(name))
}

fn assert_no_regular_file_contains(directory: &Path, payload: &[u8]) -> anyhow::Result<()> {
    assert!(!payload.is_empty());
    for path in regular_files(directory)? {
        let contents = std::fs::read(&path)?;
        assert!(
            !contents
                .windows(payload.len())
                .any(|window| window == payload),
            "stale profile payload remained in {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn maximum_component_length(directory: &Path) -> anyhow::Result<usize> {
    let probe_directory = directory.join("name-limit-probe");
    std::fs::create_dir_all(&probe_directory)?;

    let can_create = |length: usize| {
        let path = probe_directory.join("x".repeat(length));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                std::fs::remove_file(path).unwrap();
                true
            }
            Err(_) => false,
        }
    };

    let mut lower = 1;
    let mut upper = 64;
    while upper < 65_536 && can_create(upper) {
        lower = upper;
        upper *= 2;
    }
    anyhow::ensure!(upper < 65_536, "could not determine filename limit");
    while lower + 1 < upper {
        let middle = lower + (upper - lower) / 2;
        if can_create(middle) {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    std::fs::remove_dir(probe_directory)?;
    Ok(lower)
}

fn regular_files_with_contents(directory: &Path, payload: &[u8]) -> anyhow::Result<Vec<PathBuf>> {
    regular_files(directory)?
        .into_iter()
        .filter_map(|path| match std::fs::read(&path) {
            Ok(contents) if contents == payload => Some(Ok(path)),
            Ok(_) => None,
            Err(error) => Some(Err(error.into())),
        })
        .collect()
}

fn merged_pgo_profile(directory: &Path) -> PathBuf {
    directory.join(format!("merged-{}.profdata", "a".repeat(64)))
}

fn is_immediate_bolt_merged_profile(path: &Path, directory: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("merged.profdata")
        && path
            .strip_prefix(directory)
            .is_ok_and(|relative| relative.components().count() == 2)
}

fn run_clean(
    project: &crate::utils::CargoProject,
    pgo_directory: Option<&Path>,
    bolt_directory: Option<&Path>,
) -> anyhow::Result<std::process::Output> {
    let mut arguments = vec!["clean"];
    if let Some(directory) = pgo_directory {
        arguments.extend(["--profiles-dir", directory.to_str().unwrap()]);
    }
    if let Some(directory) = bolt_directory {
        arguments.extend(["--bolt-profiles-dir", directory.to_str().unwrap()]);
    }
    project.run(&arguments)
}

#[test]
fn custom_clean_removes_only_pgo_artifacts() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/pgo");
    let raw = profiles.join("nested/session.profraw");
    let dotfile = profiles.join(".profraw");
    let merged = merged_pgo_profile(&profiles);
    let nested_merged = profiles
        .join("user-owned")
        .join(format!("merged-{}.profdata", "b".repeat(64)));
    let ordinary_profdata = profiles.join("report.profdata");
    let near_match = profiles.join("merged-short.profdata");
    let uppercase_hash = profiles
        .join("uppercase")
        .join(format!("merged-{}.profdata", "A".repeat(64)));
    let bolt_profile = profiles.join("nested/session.fdata");
    let unrelated = profiles.join("notes.txt");
    #[cfg(all(unix, not(target_os = "macos")))]
    let non_utf8_raw = non_utf8_profile_path(&profiles.join("non-utf8"), b".profraw");
    for path in [
        &raw,
        &dotfile,
        &merged,
        &nested_merged,
        &ordinary_profdata,
        &near_match,
        &uppercase_hash,
        &bolt_profile,
        &unrelated,
    ] {
        write_file(path);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    write_file(&non_utf8_raw);
    let empty_directory = profiles.join("user-owned/empty");
    std::fs::create_dir_all(&empty_directory)?;

    run_clean(&project, Some(&profiles), None)?.assert_ok();

    assert!(!raw.exists());
    assert!(!dotfile.exists());
    assert!(!merged.exists());
    assert!(nested_merged.exists());
    assert!(ordinary_profdata.exists());
    assert!(near_match.exists());
    assert!(uppercase_hash.exists());
    assert!(bolt_profile.exists());
    assert!(unrelated.exists());
    assert!(empty_directory.is_dir());
    #[cfg(all(unix, not(target_os = "macos")))]
    assert!(!non_utf8_raw.exists());
    Ok(())
}

#[test]
fn custom_clean_pgo_artifact_boundaries() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/pgo-boundaries");
    let deep_raw = profiles.join("one/two/session.profraw");
    let mixed_hash = profiles.join(format!("merged-{}.profdata", "0123456789abcdef".repeat(4)));
    let short_hash = profiles.join(format!("merged-{}.profdata", "a".repeat(63)));
    let long_hash = profiles.join(format!("merged-{}.profdata", "a".repeat(65)));
    let uppercase_hash = profiles.join(format!("merged-{}.profdata", "A".repeat(64)));
    let non_hex_hash = profiles.join(format!("merged-{}.profdata", "g".repeat(64)));
    for path in [
        &deep_raw,
        &mixed_hash,
        &short_hash,
        &long_hash,
        &uppercase_hash,
        &non_hex_hash,
    ] {
        write_file(path);
    }

    run_clean(&project, Some(&profiles), None)?.assert_ok();

    assert!(!deep_raw.exists());
    assert!(!mixed_hash.exists());
    assert!(short_hash.exists());
    assert!(long_hash.exists());
    assert!(uppercase_hash.exists());
    assert!(non_hex_hash.exists());
    Ok(())
}

#[test]
fn custom_clean_removes_only_bolt_artifacts() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let default_profiles = project.path("target/bolt-profiles");
    write_contents(&default_profiles.join("notes.txt"), "default bolt content");

    project.run(&["clean"])?.assert_ok();

    assert!(!default_profiles.exists());

    let profiles = project.path("shared/bolt");
    let raw = profiles.join("foo/session.fdata");
    let merged = profiles.join("foo/merged.profdata");
    let root_merged = profiles.join("merged.profdata");
    let pgo_raw = profiles.join("foo/session.profraw");
    let pgo_merged = merged_pgo_profile(&profiles);
    let unrelated = profiles.join("notes.txt");
    let empty_target = profiles.join("empty-target");
    #[cfg(all(unix, not(target_os = "macos")))]
    let non_utf8_raw = non_utf8_profile_path(&profiles.join("foo"), b".fdata");
    for path in [
        &raw,
        &merged,
        &root_merged,
        &pgo_raw,
        &pgo_merged,
        &unrelated,
    ] {
        write_file(path);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    write_file(&non_utf8_raw);
    std::fs::create_dir_all(&empty_target)?;

    run_clean(&project, None, Some(&profiles))?.assert_ok();

    assert!(!raw.exists());
    assert!(!merged.exists());
    assert!(root_merged.exists());
    assert!(pgo_raw.exists());
    assert!(pgo_merged.exists());
    assert!(unrelated.exists());
    assert!(empty_target.is_dir());
    #[cfg(all(unix, not(target_os = "macos")))]
    assert!(!non_utf8_raw.exists());
    Ok(())
}

#[test]
fn custom_clean_bolt_artifact_depth_boundaries() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let default_pgo = project.path("target/pgo-profiles/notes.txt");
    let default_bolt = project.path("target/bolt-profiles/notes.txt");
    write_contents(&default_pgo, "default PGO content");
    write_contents(&default_bolt, "default BOLT content");

    project.run(&["clean"])?.assert_ok();

    assert!(!default_pgo.exists());
    assert!(!default_bolt.exists());

    let profiles = project.path("shared/bolt-boundaries");
    let root_fdata = profiles.join("root.fdata");
    let dotfile = profiles.join("target/.fdata");
    let deep_fdata = profiles.join("target/deeper/session.fdata");
    let nested_merged = profiles.join("target/deeper/merged.profdata");
    for path in [&root_fdata, &dotfile, &deep_fdata, &nested_merged] {
        write_file(path);
    }

    run_clean(&project, None, Some(&profiles))?.assert_ok();

    assert!(root_fdata.exists());
    assert!(!dotfile.exists());
    assert!(!deep_fdata.exists());
    assert!(nested_merged.exists());
    Ok(())
}

#[test]
fn custom_clean_handles_shared_profile_root() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let pgo_raw = profiles.join("pgo/session.profraw");
    let pgo_merged = merged_pgo_profile(&profiles);
    let bolt_raw = profiles.join("bolt/session.fdata");
    let bolt_merged = profiles.join("bolt/merged.profdata");
    let unrelated = profiles.join("keep/config.toml");
    let suffix = "a".repeat(64);
    let pgo_lookalike = profiles.join(format!(".cargo-pgo-reset-pgo-{suffix}"));
    let pgo_lookalike_artifact = pgo_lookalike.join("session.profraw");
    let pgo_lookalike_note = pgo_lookalike.join("notes.txt");
    let bolt_lookalike = profiles.join(format!(".cargo-pgo-reset-bolt-{suffix}"));
    let bolt_lookalike_artifact = bolt_lookalike.join("session.fdata");
    let bolt_lookalike_note = bolt_lookalike.join("notes.txt");
    for path in [
        &pgo_raw,
        &pgo_merged,
        &bolt_raw,
        &bolt_merged,
        &unrelated,
        &pgo_lookalike_artifact,
        &pgo_lookalike_note,
        &bolt_lookalike_artifact,
        &bolt_lookalike_note,
    ] {
        write_file(path);
    }

    run_clean(&project, Some(&profiles), Some(&profiles))?.assert_ok();

    assert!(!pgo_raw.exists());
    assert!(!pgo_merged.exists());
    assert!(!bolt_raw.exists());
    assert!(!bolt_merged.exists());
    assert!(unrelated.exists());
    assert!(!pgo_lookalike_artifact.exists());
    assert!(pgo_lookalike_note.exists());
    assert!(!bolt_lookalike_artifact.exists());
    assert!(bolt_lookalike_note.exists());

    let transactional_pgo = project.path("shared/transactional-pgo");
    let staged_pgo = transactional_pgo.join("nested/old.profraw");
    let invalid_bolt = project.path("shared/invalid-bolt-root");
    write_contents(&staged_pgo, "old transactional PGO profile");
    write_contents(&invalid_bolt, "not a directory");

    run_clean(&project, Some(&transactional_pgo), Some(&invalid_bolt))?.assert_error();

    assert_contents(&staged_pgo, "old transactional PGO profile");
    assert_contents(&invalid_bolt, "not a directory");
    Ok(())
}

#[test]
fn custom_clean_missing_roots_is_noop() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let pgo_profiles = project.path("missing/pgo");
    let bolt_profiles = project.path("missing/bolt");

    run_clean(&project, Some(&pgo_profiles), Some(&bolt_profiles))?.assert_ok();

    assert!(!pgo_profiles.exists());
    assert!(!bolt_profiles.exists());

    let existing_bolt = project.path("existing/bolt");
    let bolt_artifact = existing_bolt.join("target/session.fdata");
    let bolt_unrelated = existing_bolt.join("notes.txt");
    write_file(&bolt_artifact);
    write_file(&bolt_unrelated);

    run_clean(&project, Some(&pgo_profiles), Some(&existing_bolt))?.assert_ok();

    assert!(!pgo_profiles.exists());
    assert!(!bolt_artifact.exists());
    assert!(bolt_unrelated.exists());

    let existing_pgo = project.path("existing/pgo");
    let pgo_artifact = existing_pgo.join("session.profraw");
    let pgo_unrelated = existing_pgo.join("notes.txt");
    write_file(&pgo_artifact);
    write_file(&pgo_unrelated);

    run_clean(&project, Some(&existing_pgo), Some(&bolt_profiles))?.assert_ok();

    assert!(!pgo_artifact.exists());
    assert!(pgo_unrelated.exists());
    assert!(!bolt_profiles.exists());

    let invalid_pgo = project.path("invalid/pgo-root");
    let invalid_bolt = project.path("invalid/bolt-root");
    write_contents(&invalid_pgo, "not a PGO directory");
    write_contents(&invalid_bolt, "not a BOLT directory");

    run_clean(&project, Some(&invalid_pgo), None)?.assert_error();
    run_clean(&project, None, Some(&invalid_bolt))?.assert_error();

    assert_contents(&invalid_pgo, "not a PGO directory");
    assert_contents(&invalid_bolt, "not a BOLT directory");
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_clean_preserves_nested_symlinks() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let profiles = project.path("shared/pgo");
    let external = project.path("external");
    let external_pgo_profile = external.join("outside.profraw");
    let external_bolt_profile = external.join("outside.fdata");
    let local_pgo_profile = profiles.join("inside.profraw");
    let local_bolt_profile = profiles.join("target/inside.fdata");
    let unrelated = profiles.join("notes.txt");
    let link = profiles.join("linked");
    write_file(&external_pgo_profile);
    write_file(&external_bolt_profile);
    write_file(&local_pgo_profile);
    write_file(&local_bolt_profile);
    write_file(&unrelated);
    symlink(&external, &link)?;

    run_clean(&project, Some(&profiles), None)?.assert_ok();

    assert!(!local_pgo_profile.exists());
    assert!(local_bolt_profile.exists());
    assert!(unrelated.exists());
    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert!(external_pgo_profile.exists());
    assert!(external_bolt_profile.exists());

    run_clean(&project, None, Some(&profiles))?.assert_ok();

    assert!(!local_bolt_profile.exists());
    assert!(unrelated.exists());
    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert!(external_pgo_profile.exists());
    assert!(external_bolt_profile.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_clean_preserves_artifact_named_file_symlinks() -> anyhow::Result<()> {
    use std::os::unix::fs::{FileTypeExt, symlink};
    use std::os::unix::net::UnixListener;

    let project = init_cargo_project()?;
    let profiles = project.path("shared/symlink-files");
    let target = project.path("external/profile-data");
    let pgo_merged_target = project.path("external/pgo-merged-profile");
    let bolt_merged_target = project.path("external/bolt-merged-profile");
    let pgo_link = profiles.join("linked.profraw");
    let bolt_link = profiles.join("target/linked.fdata");
    let pgo_merged_link = merged_pgo_profile(&profiles);
    let bolt_merged_link = profiles.join("target/merged.profdata");
    let pgo_socket = profiles.join("queued.profraw");
    let bolt_socket = profiles.join("target/queued.fdata");
    write_contents(&target, "outside");
    write_contents(&pgo_merged_target, "outside PGO merged profile");
    write_contents(&bolt_merged_target, "outside BOLT merged profile");
    std::fs::create_dir_all(bolt_link.parent().unwrap())?;
    symlink(&target, &pgo_link)?;
    symlink(&target, &bolt_link)?;
    symlink(&pgo_merged_target, &pgo_merged_link)?;
    symlink(&bolt_merged_target, &bolt_merged_link)?;
    let _pgo_listener = UnixListener::bind(&pgo_socket)?;
    let _bolt_listener = UnixListener::bind(&bolt_socket)?;

    run_clean(&project, Some(&profiles), Some(&profiles))?.assert_ok();

    assert!(
        std::fs::symlink_metadata(&pgo_link)?
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&bolt_link)?
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&pgo_merged_link)?
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&bolt_merged_link)?
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&pgo_socket)?
            .file_type()
            .is_socket()
    );
    assert!(
        std::fs::symlink_metadata(&bolt_socket)?
            .file_type()
            .is_socket()
    );
    assert_contents(&target, "outside");
    assert_contents(&pgo_merged_target, "outside PGO merged profile");
    assert_contents(&bolt_merged_target, "outside BOLT merged profile");
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_clean_rejects_symlink_root() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let target = project.path("external");
    let profile = target.join("session.profraw");
    let link = project.path("linked-profiles");
    write_contents(&profile, "pgo target");
    symlink(&target, &link)?;

    run_clean(&project, Some(&link), None)?.assert_error();

    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert_contents(&profile, "pgo target");

    let bolt_profile = target.join("target/session.fdata");
    write_contents(&bolt_profile, "bolt target");

    run_clean(&project, None, Some(&link))?.assert_error();

    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert_contents(&profile, "pgo target");
    assert_contents(&bolt_profile, "bolt target");
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_paths_reject_symlink_ancestors() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let external = project.path("external");
    let selected_target = external.join("selected");
    let pgo_profile = selected_target.join("session.profraw");
    let bolt_profile = selected_target.join("target/session.fdata");
    let link = project.path("profiles-link");
    let selected = link.join("selected");
    write_contents(&pgo_profile, "pgo target");
    write_contents(&bolt_profile, "bolt target");
    symlink(&external, &link)?;

    run_clean(&project, Some(Path::new("profiles-link/selected")), None)?.assert_error();
    assert_contents(&pgo_profile, "pgo target");
    assert_contents(&bolt_profile, "bolt target");

    run_clean(&project, None, Some(&selected))?.assert_error();
    assert_contents(&pgo_profile, "pgo target");
    assert_contents(&bolt_profile, "bolt target");

    project
        .run(&["build", "--profiles-dir", "profiles-link/missing-pgo"])?
        .assert_error();
    assert!(!external.join("missing-pgo").exists());

    let missing_bolt = link.join("missing-bolt");
    project
        .run(&[
            "bolt",
            "build",
            "--profiles-dir",
            missing_bolt.to_str().unwrap(),
        ])?
        .assert_error();
    assert!(!external.join("missing-bolt").exists());

    let guarded = project.path("guard");
    let guarded_external = project.path("guarded-external");
    let guarded_link = guarded.join("link");
    std::fs::create_dir_all(&guarded)?;
    std::fs::create_dir_all(&guarded_external)?;
    symlink(&guarded_external, &guarded_link)?;

    project
        .run(&[
            "build",
            "--keep-profiles",
            "--profiles-dir",
            "guard/missing/../link/profiles",
        ])?
        .assert_error();

    run_clean(
        &project,
        Some(Path::new("guard/missing/../link/profiles")),
        None,
    )?
    .assert_error();
    run_clean(
        &project,
        None,
        Some(Path::new("guard/missing/../link/profiles")),
    )?
    .assert_error();

    assert!(
        std::fs::symlink_metadata(&guarded_link)?
            .file_type()
            .is_symlink()
    );
    assert!(!guarded_external.join("profiles").exists());
    Ok(())
}

#[test]
fn custom_pgo_instrumentation_resets_only_pgo_artifacts() -> anyhow::Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let pgo_raw = profiles.join("pgo/session.profraw");
    let pgo_dotfile = profiles.join(".profraw");
    let pgo_merged = merged_pgo_profile(&profiles);
    let bolt_raw = profiles.join("bolt/session.fdata");
    let unrelated = profiles.join("notes.txt");
    for path in [&pgo_raw, &pgo_dotfile, &pgo_merged, &bolt_raw, &unrelated] {
        write_file(path);
    }
    #[cfg(unix)]
    let (link, linked_profile) = {
        let external = project.path("external-pgo");
        let linked_profile = external.join("linked.profraw");
        let link = profiles.join("linked");
        write_file(&linked_profile);
        symlink(&external, &link)?;
        (link, linked_profile)
    };

    project
        .run(&["build", "--profiles-dir", profiles.to_str().unwrap()])?
        .assert_ok();

    assert!(!pgo_raw.exists());
    assert!(!pgo_dotfile.exists());
    assert!(!pgo_merged.exists());
    assert!(bolt_raw.exists());
    assert!(unrelated.exists());
    #[cfg(unix)]
    {
        assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert!(linked_profile.exists());
    }

    write_contents(&pgo_raw, "kept raw profile");
    write_contents(&pgo_merged, "kept merged profile");
    project
        .run(&[
            "build",
            "--keep-profiles",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])?
        .assert_ok();

    assert_contents(&pgo_raw, "kept raw profile");
    assert_contents(&pgo_merged, "kept merged profile");
    assert!(bolt_raw.exists());
    assert!(unrelated.exists());

    let invalid_profiles = project.path("shared/not-a-pgo-directory");
    write_contents(&invalid_profiles, "not a PGO directory");
    project
        .run(&[
            "build",
            "--profiles-dir",
            invalid_profiles.to_str().unwrap(),
        ])?
        .assert_error();
    project
        .run(&["run", "--profiles-dir", invalid_profiles.to_str().unwrap()])?
        .assert_error();
    assert_contents(&invalid_profiles, "not a PGO directory");

    let default_profiles = project.path("target/pgo-profiles");
    let default_sentinel = default_profiles.join("unrelated.txt");
    write_contents(&default_sentinel, "remove the whole default PGO root");

    project.run(&["build"])?.assert_ok();

    assert!(!default_sentinel.exists());
    assert!(default_profiles.is_dir());
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_instrumentation_rejects_symlink_roots() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let pgo_project = init_cargo_project()?;
    let pgo_target = pgo_project.path("external-pgo");
    let pgo_profile = pgo_target.join("session.profraw");
    let pgo_link = pgo_project.path("linked-pgo");
    write_file(&pgo_profile);
    symlink(&pgo_target, &pgo_link)?;

    pgo_project
        .run(&["build", "--profiles-dir", pgo_link.to_str().unwrap()])?
        .assert_error();

    pgo_project
        .run(&["run", "--profiles-dir", pgo_link.to_str().unwrap()])?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_link)?
            .file_type()
            .is_symlink()
    );
    assert!(pgo_profile.exists());

    let optimize_tools = fake_pgo_optimize_tools(&pgo_project, "symlink-pgo-optimize-tools")?;
    let real_cargo = which::which("cargo")?;
    let real_rustc = which::which("rustc")?;
    let fake_target_libdir = optimize_tools.join("missing-target-libdir");
    let target_files = regular_files(&pgo_target)?;
    let target_profile_contents = std::fs::read(&pgo_profile)?;
    pgo_project
        .cmd(&["optimize", "--profiles-dir", pgo_link.to_str().unwrap()])
        .prepend_path(&optimize_tools)
        .env("REAL_CARGO", real_cargo.to_str().unwrap())
        .env("REAL_RUSTC", real_rustc.to_str().unwrap())
        .env("FAKE_TARGET_LIBDIR", fake_target_libdir.to_str().unwrap())
        .env("EXPECTED_PROFILE_ROOT", pgo_link.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_link)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(regular_files(&pgo_target)?, target_files);
    assert_eq!(std::fs::read(&pgo_profile)?, target_profile_contents);

    let pgo_profiles = pgo_project.path("pgo-output-profiles");
    let pgo_output_target = pgo_project.path("external-pgo-output");
    let pgo_output_sentinel = pgo_output_target.join("sentinel.txt");
    let pgo_output_link = pgo_profiles.join("linked-output");
    let pgo_output = pgo_output_link.join("output.profraw");
    write_contents(&pgo_output_sentinel, "outside");
    std::fs::create_dir_all(&pgo_profiles)?;
    symlink(&pgo_output_target, &pgo_output_link)?;

    pgo_project
        .cmd(&[
            "build",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", pgo_output.to_str().unwrap())
        .run()?
        .assert_ok();

    assert!(
        std::fs::symlink_metadata(&pgo_output_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_output_sentinel, "outside");
    assert!(!pgo_output_target.join("output.profraw").exists());

    let reset_profile = pgo_profiles.join("reset-before-build.profraw");
    write_contents(&reset_profile, "reset before PGO build");
    pgo_project
        .cmd(&["build", "--profiles-dir", pgo_profiles.to_str().unwrap()])
        .env("LLVM_PROFILE_FILE", pgo_output.to_str().unwrap())
        .run()?
        .assert_ok();

    assert!(!reset_profile.exists());
    assert!(
        std::fs::symlink_metadata(&pgo_output_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_output_sentinel, "outside");
    assert!(!pgo_output_target.join("output.profraw").exists());

    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", pgo_output.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_output_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_output_sentinel, "outside");
    assert!(!pgo_output_target.join("output.profraw").exists());

    let pgo_profiles_alias = pgo_project.path("pgo-profiles-alias");
    symlink(&pgo_profiles, &pgo_profiles_alias)?;
    let aliased_pgo_output = pgo_profiles_alias
        .join(pgo_output_link.file_name().unwrap())
        .join("output.profraw");
    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", aliased_pgo_output.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_profiles_alias)?
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&pgo_output_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_output_sentinel, "outside");
    assert!(!pgo_output_target.join("output.profraw").exists());

    let pgo_file_target = pgo_project.path("external-profile-file.profraw");
    let pgo_file_link = pgo_profiles.join("output-file.profraw");
    write_contents(&pgo_file_target, "external profile file");
    symlink(&pgo_file_target, &pgo_file_link)?;

    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", pgo_file_link.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_file_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_file_target, "external profile file");

    let old_profile = pgo_profiles.join("old-before-output-check.profraw");
    write_contents(&old_profile, "old profile");
    pgo_project
        .cmd(&["run", "--profiles-dir", pgo_profiles.to_str().unwrap()])
        .env("LLVM_PROFILE_FILE", pgo_file_link.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&old_profile, "old profile");
    assert!(
        std::fs::symlink_metadata(&pgo_file_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_file_target, "external profile file");

    let default_pgo_profiles = pgo_project.path("target/pgo-profiles");
    let default_pgo_target = pgo_project.path("external-default-profile.profraw");
    let default_pgo_link = default_pgo_profiles.join("output.profraw");
    write_contents(&default_pgo_target, "external default profile");
    std::fs::create_dir_all(&default_pgo_profiles)?;
    symlink(&default_pgo_target, &default_pgo_link)?;

    let default_target_alias = pgo_project.path("default-target-alias");
    symlink(pgo_project.path("target"), &default_target_alias)?;
    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--",
            "--target-dir",
            default_target_alias.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", default_pgo_link.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&default_pgo_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&default_pgo_target, "external default profile");

    let noncanonical_parent = pgo_project.path("noncanonical-parent");
    std::fs::create_dir(&noncanonical_parent)?;
    let noncanonical_pgo_output = noncanonical_parent
        .join("..")
        .join(pgo_profiles.file_name().unwrap())
        .join(pgo_file_link.file_name().unwrap());
    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env(
            "LLVM_PROFILE_FILE",
            noncanonical_pgo_output.to_str().unwrap(),
        )
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_file_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_file_target, "external profile file");

    let escaped_pgo_output = pgo_output_link.join("..").join("escaped.profraw");
    let escaped_pgo_target = pgo_output_target.parent().unwrap().join("escaped.profraw");
    pgo_project
        .cmd(&[
            "run",
            "--keep-profiles",
            "--profiles-dir",
            pgo_profiles.to_str().unwrap(),
        ])
        .env("LLVM_PROFILE_FILE", escaped_pgo_output.to_str().unwrap())
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&pgo_output_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&pgo_output_sentinel, "outside");
    assert!(!escaped_pgo_target.exists());

    let bolt_project = init_cargo_project()?;
    let bolt_target = bolt_project.path("external-bolt");
    let bolt_profile = bolt_target.join("session.fdata");
    let bolt_link = bolt_project.path("linked-bolt");
    write_file(&bolt_profile);
    symlink(&bolt_target, &bolt_link)?;
    let tools = fake_bolt_tools(&bolt_project)?;

    bolt_project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            bolt_link.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&bolt_link)?
            .file_type()
            .is_symlink()
    );
    assert!(bolt_profile.exists());
    Ok(())
}

fn fake_bolt_tools_named(
    project: &crate::utils::CargoProject,
    name: &str,
    bolt_succeeds: bool,
) -> anyhow::Result<PathBuf> {
    let tools = project.path(name);
    let bolt_source = tools.join("fake-bolt.rs");
    let merge_source = tools.join("fake-merge-fdata.rs");
    let bolt = tools.join(format!("llvm-bolt{}", std::env::consts::EXE_SUFFIX));
    let merge = tools.join(format!("merge-fdata{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(&tools)?;
    let bolt_program = r#"use std::ffi::OsString;
use std::process::exit;

const SUCCEEDS: bool = __SUCCEEDS__;

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut profile = None;
    for pair in arguments.windows(2) {
        if pair[0] == "-instrument" {
            input = Some(pair[1].clone());
        }
        if pair[0] == "-o" {
            output = Some(pair[1].clone());
        }
        if pair[0] == "--instrumentation-file" {
            profile = Some(pair[1].clone());
        }
    }
    if input.is_some() {
        let profile = match std::env::var_os("FAKE_BOLT_PROFILE_PATH") {
            Some(profile) => std::path::PathBuf::from(profile),
            None => {
                let mut profile = profile.unwrap();
                profile.push(".new.fdata");
                std::path::PathBuf::from(profile)
            }
        };
        std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
        std::fs::write(profile, b"new bolt profile").unwrap();
    }
    if !SUCCEEDS {
        exit(42);
    }
    let input = input.unwrap_or_else(|| arguments.first().unwrap().clone());
    std::fs::copy(input, output.unwrap()).unwrap();
}
"#
    .replace("__SUCCEEDS__", if bolt_succeeds { "true" } else { "false" });
    std::fs::write(&bolt_source, bolt_program)?;
    std::fs::write(&merge_source, "fn main() {}\n")?;
    for (source, output) in [(&bolt_source, &bolt), (&merge_source, &merge)] {
        let status = Command::new("rustc")
            .args([source.as_os_str(), std::ffi::OsStr::new("-o")])
            .arg(output)
            .status()?;
        if !status.success() {
            anyhow::bail!("could not compile fake BOLT tool");
        }
    }
    Ok(tools)
}

#[cfg(unix)]
fn fake_bolt_tools(project: &crate::utils::CargoProject) -> anyhow::Result<PathBuf> {
    fake_bolt_tools_named(project, "fake-bolt-tools", true)
}

#[cfg(unix)]
fn fake_pgo_optimize_tools(
    project: &crate::utils::CargoProject,
    name: &str,
) -> anyhow::Result<PathBuf> {
    let tools = project.path(name);
    let profdata_source = tools.join("fake-llvm-profdata.rs");
    let cargo_source = tools.join("fake-cargo.rs");
    let rustc_source = tools.join("fake-rustc.rs");
    let profdata = tools.join(format!("llvm-profdata{}", std::env::consts::EXE_SUFFIX));
    let cargo = tools.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let rustc = tools.join(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(&tools)?;

    std::fs::write(
        &profdata_source,
        r#"use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;

fn main() {
    let expected = PathBuf::from(std::env::var_os("EXPECTED_PROFILE_ROOT").unwrap());
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let mut output = None;
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "-o" {
            index += 1;
            output = arguments.get(index).cloned();
        } else if arguments[index] != "merge" {
            inputs.push(arguments[index].clone());
        }
        index += 1;
    }
    if inputs.is_empty()
        || inputs
            .iter()
            .any(|input| !Path::new(input).starts_with(&expected))
    {
        eprintln!("unexpected profile inputs {inputs:?}; expected root {expected:?}");
        exit(41);
    }
    if let Some(expected_inputs) = std::env::var_os("EXPECTED_PROFILE_INPUTS") {
        let mut expected_inputs = std::env::split_paths(&expected_inputs).collect::<Vec<_>>();
        let mut actual_inputs = inputs.iter().map(PathBuf::from).collect::<Vec<_>>();
        if std::env::var("EXPECT_NON_UTF8_PROFILE").as_deref() == Ok("1") {
            let expected_name = b"profile-\xff.profraw";
            let matches = actual_inputs
                .iter()
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| name.as_bytes() == expected_name)
                })
                .count();
            if matches != 1 {
                eprintln!("expected one non-UTF-8 profile input, found {matches}");
                exit(43);
            }
            actual_inputs.retain(|path| {
                !path
                    .file_name()
                    .is_some_and(|name| name.as_bytes() == expected_name)
            });
        }
        expected_inputs.sort();
        actual_inputs.sort();
        if actual_inputs != expected_inputs {
            eprintln!("unexpected profile inputs {actual_inputs:?}; expected {expected_inputs:?}");
            exit(42);
        }
    }
    std::fs::write(output.unwrap(), b"fake merged profile").unwrap();
}
"#,
    )?;
    std::fs::write(
        &cargo_source,
        r#"use std::ffi::OsString;
use std::process::{Command, exit};

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if matches!(
        arguments.first().and_then(|value| value.to_str()),
        Some("pgo") | Some("metadata")
    ) {
        let status = Command::new(std::env::var_os("REAL_CARGO").unwrap())
            .args(&arguments)
            .status()
            .unwrap();
        exit(status.code().unwrap_or(1));
    }

}
"#,
    )?;
    std::fs::write(
        &rustc_source,
        r#"use std::ffi::OsString;
use std::process::{Command, exit};

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments.len() == 2
        && arguments[0] == "--print"
        && arguments[1] == "target-libdir"
    {
        println!("{}", std::env::var("FAKE_TARGET_LIBDIR").unwrap());
        return;
    }
    let status = Command::new(std::env::var_os("REAL_RUSTC").unwrap())
        .args(&arguments)
        .status()
        .unwrap();
    exit(status.code().unwrap_or(1));
}
"#,
    )?;

    for (source, output) in [
        (&profdata_source, &profdata),
        (&cargo_source, &cargo),
        (&rustc_source, &rustc),
    ] {
        let status = Command::new("rustc")
            .args([source.as_os_str(), std::ffi::OsStr::new("-o")])
            .arg(output)
            .status()?;
        if !status.success() {
            anyhow::bail!("could not compile fake PGO optimize tool");
        }
    }
    Ok(tools)
}

fn add_blocking_cargo(tools: &Path) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let marker = tools.join("cargo-started");
    let stop = tools.join("cargo-stop");
    let done = tools.join("cargo-stopped");
    let source = tools.join("blocking-cargo.rs");
    let cargo = tools.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(
        &source,
        r#"use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, exit};
use std::time::Duration;

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments.first().and_then(|value| value.to_str()) == Some("metadata") {
        let status = Command::new(std::env::var_os("REAL_CARGO").unwrap())
            .args(&arguments)
            .status()
            .unwrap();
        exit(status.code().unwrap_or(1));
    }

    std::fs::write(std::env::var_os("BLOCK_MARKER").unwrap(), b"started").unwrap();
    let stop = std::env::var_os("BLOCK_STOP").unwrap();
    while !Path::new(&stop).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::write(std::env::var_os("BLOCK_DONE").unwrap(), b"stopped").unwrap();
    exit(1);
}
"#,
    )?;
    let status = Command::new("rustc")
        .args([source.as_os_str(), std::ffi::OsStr::new("-o")])
        .arg(&cargo)
        .status()?;
    if !status.success() {
        anyhow::bail!("could not compile blocking cargo helper");
    }
    Ok((marker, stop, done))
}

fn add_blocking_bolt(tools: &Path) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let marker = tools.join("bolt-started");
    let stop = tools.join("bolt-stop");
    let done = tools.join("bolt-stopped");
    let source = tools.join("blocking-bolt.rs");
    let bolt = tools.join(format!("llvm-bolt{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(
        &source,
        r#"use std::path::Path;
use std::process::exit;
use std::time::Duration;

fn main() {
    std::fs::write(std::env::var_os("BLOCK_MARKER").unwrap(), b"started").unwrap();
    let stop = std::env::var_os("BLOCK_STOP").unwrap();
    while !Path::new(&stop).exists() {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::write(std::env::var_os("BLOCK_DONE").unwrap(), b"stopped").unwrap();
    exit(1);
}
"#,
    )?;
    let status = Command::new("rustc")
        .args([source.as_os_str(), std::ffi::OsStr::new("-o")])
        .arg(&bolt)
        .status()?;
    if !status.success() {
        anyhow::bail!("could not compile blocking BOLT helper");
    }
    Ok((marker, stop, done))
}

fn interrupt_after_profile_reset(
    project: &crate::utils::CargoProject,
    arguments: &[&str],
    tools: &Path,
    marker: &Path,
    stop: &Path,
    done: &Path,
) -> anyhow::Result<()> {
    let real_cargo = which::which("cargo")?;
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(tools.to_path_buf()).chain(std::env::split_paths(&current_path)),
    )?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-pgo"));
    command
        .arg("pgo")
        .args(arguments)
        .current_dir(&project.dir)
        .env("PATH", path)
        .env("REAL_CARGO", real_cargo)
        .env("BLOCK_MARKER", marker)
        .env("BLOCK_STOP", stop)
        .env("BLOCK_DONE", done)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() {
        if let Some(status) = child.try_wait()? {
            let _ = std::fs::write(stop, b"stop");
            anyhow::bail!("cargo-pgo exited before the reset marker: {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::write(stop, b"stop");
            anyhow::bail!("timed out waiting for cargo-pgo to reset profiles");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    child.kill()?;
    child.wait()?;
    std::fs::write(stop, b"stop")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done.exists() {
        if Instant::now() >= deadline {
            anyhow::bail!("timed out stopping blocked cargo helper");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(unix)]
#[cfg(target_os = "linux")]
#[test]
fn custom_bolt_instrumentation_resets_only_bolt_artifacts() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let bolt_raw = profiles.join("bolt/session.fdata");
    let bolt_dotfile = profiles.join("bolt/.fdata");
    let bolt_merged = profiles.join("bolt/merged.profdata");
    let pgo_raw = profiles.join("pgo/session.profraw");
    let unrelated = profiles.join("notes.txt");
    for path in [&bolt_raw, &bolt_dotfile, &bolt_merged, &pgo_raw, &unrelated] {
        write_file(path);
    }
    let external = project.path("external-bolt");
    let linked_profile = external.join("linked.fdata");
    let link = profiles.join("linked");
    write_file(&linked_profile);
    symlink(&external, &link)?;
    let tools = fake_bolt_tools(&project)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_ok();

    assert!(!bolt_raw.exists());
    assert!(!bolt_dotfile.exists());
    assert!(!bolt_merged.exists());
    assert!(pgo_raw.exists());
    assert!(unrelated.exists());
    assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert!(linked_profile.exists());

    write_contents(&bolt_raw, "kept raw profile");
    write_contents(&bolt_merged, "kept merged profile");
    project
        .cmd(&[
            "bolt",
            "build",
            "--keep-profiles",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_ok();

    assert_contents(&bolt_raw, "kept raw profile");
    assert_contents(&bolt_merged, "kept merged profile");
    assert!(pgo_raw.exists());
    assert!(unrelated.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn custom_bolt_instrumentation_rejects_symlinked_outputs() -> anyhow::Result<()> {
    use std::os::unix::fs::symlink;

    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let external = project.path("external-bolt-target");
    let sentinel = external.join("sentinel.txt");
    write_contents(&sentinel, "outside");
    std::fs::create_dir_all(&profiles)?;
    let target_link = profiles.join("foo");
    symlink(&external, &target_link)?;
    let tools = fake_bolt_tools(&project)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&target_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&sentinel, "outside");

    let default_profiles = project.path("target/bolt-profiles");
    let default_external = project.path("external-default-bolt-target");
    let default_sentinel = default_external.join("sentinel.txt");
    let default_target_link = default_profiles.join("foo");
    write_contents(&default_sentinel, "default outside");
    std::fs::create_dir_all(&default_profiles)?;
    symlink(&default_external, &default_target_link)?;

    project
        .cmd(&["bolt", "build", "--keep-profiles"])
        .prepend_path(&tools)
        .run()?
        .assert_error();

    assert!(
        std::fs::symlink_metadata(&default_target_link)?
            .file_type()
            .is_symlink()
    );
    assert_contents(&default_sentinel, "default outside");

    Ok(())
}

#[test]
fn successful_custom_pgo_run_retains_new_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/successful-run");
    let old = profiles.join("old.profraw");
    let bolt = profiles.join("bolt/session.fdata");
    let default_profiles = project.path("target/pgo-profiles");
    write_contents(&old, "old profile");
    write_contents(&bolt, "BOLT profile");

    project
        .run(&["run", "--profiles-dir", profiles.to_str().unwrap()])?
        .assert_ok();

    assert!(!old.exists());
    let new_profiles = regular_files(&profiles)?
        .into_iter()
        .filter(|path| has_profile_suffix(path, "profraw"))
        .collect::<Vec<_>>();
    assert!(!new_profiles.is_empty());
    assert!(
        regular_files(&default_profiles)?
            .into_iter()
            .all(|path| !has_profile_suffix(&path, "profraw"))
    );
    assert_contents(&bolt, "BOLT profile");
    Ok(())
}

#[test]
fn successful_custom_pgo_build_removes_transaction_backups() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/successful-build");
    let bolt = profiles.join("bolt/session.fdata");
    let old_payload = "stale PGO profile payload 9c2f6a";
    write_contents(&profiles.join("old/session.profraw"), old_payload);
    write_contents(&bolt, "BOLT profile");

    project
        .run(&["build", "--profiles-dir", profiles.to_str().unwrap()])?
        .assert_ok();

    let remaining_profiles: Vec<_> = regular_files(&profiles)?
        .into_iter()
        .filter(|path| has_profile_suffix(path, "profraw"))
        .collect();
    assert!(remaining_profiles.is_empty());
    assert_no_regular_file_contains(&profiles, old_payload.as_bytes())?;
    assert_contents(&bolt, "BOLT profile");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn successful_custom_bolt_build_commits_reset_and_retains_new_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let tools = fake_bolt_tools_named(&project, "successful-bolt-tools", true)?;
    let invalid_profiles = project.path("shared/not-a-bolt-directory");
    write_contents(&invalid_profiles, "not a BOLT directory");
    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            invalid_profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_error();
    assert_contents(&invalid_profiles, "not a BOLT directory");

    let kept_profiles = project.path("shared/kept-bolt");
    assert!(!kept_profiles.exists());
    project
        .cmd(&[
            "bolt",
            "build",
            "--keep-profiles",
            "--profiles-dir",
            kept_profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_ok();
    assert_eq!(
        regular_files_with_contents(&kept_profiles.join("foo"), b"new bolt profile")?.len(),
        1
    );

    let profiles = project.path("shared/successful-bolt");
    let target_profiles = profiles.join("foo");
    let old = profiles.join("old/session.fdata");
    let pgo = profiles.join("pgo/session.profraw");
    let old_payload = "stale BOLT profile payload 9c2f6a";
    write_contents(&old, old_payload);
    write_contents(&pgo, "PGO profile");
    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_ok();

    assert!(!old.exists());
    let new_profiles = regular_files_with_contents(&target_profiles, b"new bolt profile")?;
    assert_eq!(new_profiles.len(), 1);
    let new = &new_profiles[0];
    assert_eq!(new.extension(), Some(std::ffi::OsStr::new("fdata")));
    assert_contents(&pgo, "PGO profile");
    assert_no_regular_file_contains(&profiles, old_payload.as_bytes())?;
    let remaining_profiles: Vec<_> = regular_files(&profiles)?
        .into_iter()
        .filter(|path| has_profile_suffix(path, "fdata"))
        .collect();
    assert_eq!(remaining_profiles, vec![new.clone()]);

    let default_profiles = project.path("target/bolt-profiles");
    let default_sentinel = default_profiles.join("unrelated.txt");
    write_contents(&default_sentinel, "remove the whole default BOLT root");

    project
        .cmd(&["bolt", "build"])
        .prepend_path(&tools)
        .run()?
        .assert_ok();

    assert!(!default_sentinel.exists());
    assert!(default_profiles.is_dir());
    Ok(())
}

#[test]
fn failed_custom_pgo_build_restores_previous_profiles() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let raw = profiles.join("old/session.profraw");
    let merged = merged_pgo_profile(&profiles);
    let bolt = profiles.join("foo/session.fdata");
    let unrelated = profiles.join("notes.txt");
    write_contents(&raw, "old raw profile");
    write_contents(&merged, "old merged profile");
    write_contents(&bolt, "bolt profile");
    write_contents(&unrelated, "user data");
    project.file(
        "src/main.rs",
        "fn main() { let _: u32 = \"not a number\"; }",
    );

    project
        .run(&["build", "--profiles-dir", profiles.to_str().unwrap()])?
        .assert_error();

    assert_contents(&raw, "old raw profile");
    assert_contents(&merged, "old merged profile");
    assert_contents(&bolt, "bolt profile");
    assert_contents(&unrelated, "user data");

    Ok(())
}

#[test]
fn failed_custom_pgo_run_keeps_old_and_new_profiles() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let old = profiles.join(".profraw");
    let merged = merged_pgo_profile(&profiles);
    write_contents(&old, "old raw profile");
    write_contents(&merged, "old merged profile");
    project.file(
        "src/main.rs",
        "fn main() { panic!(\"expected runtime failure\"); }",
    );

    project
        .cmd(&["run", "--profiles-dir", profiles.to_str().unwrap()])
        .env("LLVM_PROFILE_FILE", old.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&old, "old raw profile");
    assert_contents(&merged, "old merged profile");
    let new_profiles: Vec<_> = regular_files(&profiles)?
        .into_iter()
        .filter(|path| path != &old && has_profile_suffix(path, "profraw"))
        .collect();
    assert!(!new_profiles.is_empty());
    assert!(
        new_profiles
            .iter()
            .any(|path| std::fs::read(path).unwrap() != b"old raw profile")
    );

    let profiles_before_merged_collision = regular_files(&profiles)?;

    project
        .cmd(&["run", "--profiles-dir", profiles.to_str().unwrap()])
        .env("LLVM_PROFILE_FILE", merged.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&old, "old raw profile");
    assert_contents(&merged, "old merged profile");
    let retained_collision_profiles: Vec<_> = regular_files(&profiles)?
        .into_iter()
        .filter(|path| {
            !profiles_before_merged_collision.contains(path) && has_profile_suffix(path, "profraw")
        })
        .collect();
    assert_eq!(retained_collision_profiles.len(), 1);
    assert_ne!(
        std::fs::read(&retained_collision_profiles[0])?,
        b"old merged profile"
    );

    #[cfg(unix)]
    {
        let expected_profile_inputs = regular_files(&profiles)?
            .into_iter()
            .filter(|path| has_profile_suffix(path, "profraw"))
            .collect::<Vec<_>>();
        assert!(expected_profile_inputs.contains(&retained_collision_profiles[0]));
        let expected_profile_inputs = std::env::join_paths(expected_profile_inputs)?;
        let optimize_tools = fake_pgo_optimize_tools(&project, "collision-optimize-tools")?;
        let real_cargo = which::which("cargo")?;
        let real_rustc = which::which("rustc")?;
        let fake_target_libdir = optimize_tools.join("missing-target-libdir");
        project
            .cmd(&["optimize", "--profiles-dir", profiles.to_str().unwrap()])
            .prepend_path(&optimize_tools)
            .env("REAL_CARGO", real_cargo.to_str().unwrap())
            .env("REAL_RUSTC", real_rustc.to_str().unwrap())
            .env("FAKE_TARGET_LIBDIR", fake_target_libdir.to_str().unwrap())
            .env("EXPECTED_PROFILE_ROOT", profiles.to_str().unwrap())
            .env(
                "EXPECTED_PROFILE_INPUTS",
                expected_profile_inputs.to_str().unwrap(),
            )
            .run()?
            .assert_ok();
    }

    #[cfg(unix)]
    let retained_maximum_name_profiles = {
        let component_limit = maximum_component_length(&profiles)?;
        let maximum_name_profile = profiles.join(format!(
            "{}.profraw",
            "a".repeat(component_limit - ".profraw".len())
        ));
        write_contents(&maximum_name_profile, "old maximum-name profile");
        let profiles_before_maximum_name_collision = regular_files(&profiles)?;

        project.file(
            "src/main.rs",
            r#"fn main() {
    let output = std::env::var_os("LLVM_PROFILE_FILE").unwrap();
    std::fs::write(output, "new maximum-name profile").unwrap();
    std::process::abort();
}
"#,
        );

        project
            .cmd(&["run", "--profiles-dir", profiles.to_str().unwrap()])
            .env("LLVM_PROFILE_FILE", maximum_name_profile.to_str().unwrap())
            .run()?
            .assert_error();

        assert_contents(&maximum_name_profile, "old maximum-name profile");
        let retained: Vec<_> = regular_files(&profiles)?
            .into_iter()
            .filter(|path| {
                !profiles_before_maximum_name_collision.contains(path)
                    && has_profile_suffix(path, "profraw")
            })
            .collect();
        assert_eq!(retained.len(), 1);
        assert_ne!(std::fs::read(&retained[0])?, b"old maximum-name profile");
        (maximum_name_profile, retained)
    };

    run_clean(&project, Some(&profiles), None)?.assert_ok();

    assert!(!merged.exists());
    assert!(
        retained_collision_profiles
            .iter()
            .all(|path| !path.exists())
    );
    #[cfg(unix)]
    {
        assert!(!retained_maximum_name_profiles.0.exists());
        assert!(!retained_maximum_name_profiles.1[0].exists());
    }
    Ok(())
}

#[test]
fn failed_custom_bolt_cargo_build_restores_previous_profiles() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let raw = profiles.join("foo/session.fdata");
    let merged = profiles.join("foo/merged.profdata");
    let pgo = profiles.join("pgo/session.profraw");
    write_contents(&raw, "old raw profile");
    write_contents(&merged, "old merged profile");
    write_contents(&pgo, "pgo profile");
    project.file(
        "build.rs",
        r#"fn main() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let profile = root.join("shared/profiles/foo/session.fdata");
    std::fs::write(profile, "changed raw profile").unwrap();
    panic!("fail after changing the existing BOLT profile");
}
"#,
    );
    project.file(
        "src/main.rs",
        "fn main() { let _: u32 = \"not a number\"; }",
    );
    let tools = fake_bolt_tools_named(&project, "cargo-failure-bolt-tools", true)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_error();

    assert_contents(&raw, "old raw profile");
    assert_contents(&merged, "old merged profile");
    assert_contents(&pgo, "pgo profile");
    let retained_compile_profiles = regular_files_with_contents(&profiles, b"changed raw profile")?;
    assert_eq!(retained_compile_profiles.len(), 1);
    assert_ne!(retained_compile_profiles[0], raw);
    assert!(has_profile_suffix(&retained_compile_profiles[0], "fdata"));

    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn failed_custom_bolt_instrumentation_restores_previous_profiles() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let target_profiles = profiles.join("foo");
    let raw = profiles.join("foo/session.fdata");
    let dotfile = profiles.join("foo/.fdata");
    let merged = profiles.join("foo/merged.profdata");
    let pgo = profiles.join("pgo/session.profraw");
    write_contents(&raw, "old raw profile");
    write_contents(&dotfile, "old hidden raw profile");
    write_contents(&merged, "old merged profile");
    write_contents(&pgo, "pgo profile");
    let tools = fake_bolt_tools_named(&project, "instrument-failure-bolt-tools", false)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .run()?
        .assert_error();

    assert_contents(&raw, "old raw profile");
    assert_contents(&dotfile, "old hidden raw profile");
    assert_contents(&merged, "old merged profile");
    assert_contents(&pgo, "pgo profile");
    let new_profiles = regular_files_with_contents(&target_profiles, b"new bolt profile")?;
    assert_eq!(new_profiles.len(), 1);
    assert_eq!(
        new_profiles[0].extension(),
        Some(std::ffi::OsStr::new("fdata"))
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn failed_custom_bolt_instrumentation_preserves_colliding_new_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let target_profiles = profiles.join("foo");
    let collision = target_profiles.join(".fdata");
    let merged = profiles.join("foo/merged.profdata");
    let pgo = profiles.join("pgo/session.profraw");
    write_contents(&collision, "old colliding bolt profile");
    write_contents(&merged, "old merged profile");
    write_contents(&pgo, "pgo profile");
    let tools = fake_bolt_tools_named(&project, "collision-bolt-tools", false)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .env("FAKE_BOLT_PROFILE_PATH", collision.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&merged, "old merged profile");
    assert_contents(&pgo, "pgo profile");
    assert_contents(&collision, "old colliding bolt profile");
    let retained_new_profiles = regular_files_with_contents(&target_profiles, b"new bolt profile")?;
    assert_eq!(retained_new_profiles.len(), 1);
    assert_ne!(retained_new_profiles[0], collision);
    assert!(has_profile_suffix(&retained_new_profiles[0], "fdata"));

    #[cfg(unix)]
    let blocked_recovery_slot = {
        use std::os::unix::fs::symlink;

        let external = project.path("unrelated-recovery-target");
        let sentinel = external.join("notes.txt");
        let link = profiles.join("foo.recovered-1");
        write_contents(&sentinel, "unrelated recovery content");
        symlink(&external, &link)?;
        Some((link, sentinel))
    };

    let profiles_before_merged_collision = regular_files(&profiles)?;

    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&tools)
        .env("FAKE_BOLT_PROFILE_PATH", merged.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&merged, "old merged profile");
    assert_contents(&pgo, "pgo profile");
    let retained_collision_profiles = regular_files_with_contents(&profiles, b"new bolt profile")?
        .into_iter()
        .filter(|path| {
            !profiles_before_merged_collision.contains(path)
                && ((path
                    .strip_prefix(&profiles)
                    .is_ok_and(|relative| relative.components().count() >= 2)
                    && has_profile_suffix(path, "fdata"))
                    || is_immediate_bolt_merged_profile(path, &profiles))
        })
        .collect::<Vec<_>>();
    assert!(!retained_collision_profiles.is_empty());

    #[cfg(unix)]
    let retained_maximum_name_profiles = {
        let component_limit = maximum_component_length(&target_profiles)?;
        let maximum_name_profile = target_profiles.join(format!(
            "{}.fdata",
            "a".repeat(component_limit - ".fdata".len())
        ));
        write_contents(&maximum_name_profile, "old maximum-name BOLT profile");
        let profiles_before_maximum_name_collision = regular_files(&profiles)?;

        project
            .cmd(&[
                "bolt",
                "build",
                "--profiles-dir",
                profiles.to_str().unwrap(),
            ])
            .prepend_path(&tools)
            .env(
                "FAKE_BOLT_PROFILE_PATH",
                maximum_name_profile.to_str().unwrap(),
            )
            .run()?
            .assert_error();

        assert_contents(&maximum_name_profile, "old maximum-name BOLT profile");
        let retained = regular_files_with_contents(&profiles, b"new bolt profile")?
            .into_iter()
            .filter(|path| {
                !profiles_before_maximum_name_collision.contains(path)
                    && has_profile_suffix(path, "fdata")
            })
            .collect::<Vec<_>>();
        assert_eq!(retained.len(), 1);
        (maximum_name_profile, retained)
    };

    #[cfg(unix)]
    if let Some((link, sentinel)) = &blocked_recovery_slot {
        assert!(std::fs::symlink_metadata(link)?.file_type().is_symlink());
        assert_contents(sentinel, "unrelated recovery content");
    }

    run_clean(&project, None, Some(&profiles))?.assert_ok();

    assert!(!collision.exists());
    assert!(!retained_new_profiles[0].exists());
    assert!(!merged.exists());
    assert!(
        retained_collision_profiles
            .iter()
            .all(|path| !path.exists())
    );
    #[cfg(unix)]
    {
        assert!(!retained_maximum_name_profiles.0.exists());
        assert!(!retained_maximum_name_profiles.1[0].exists());
    }
    assert_contents(&pgo, "pgo profile");
    #[cfg(unix)]
    if let Some((link, sentinel)) = &blocked_recovery_slot {
        assert!(std::fs::symlink_metadata(link)?.file_type().is_symlink());
        assert_contents(sentinel, "unrelated recovery content");
    }
    Ok(())
}

#[test]
fn interrupted_custom_pgo_reset_is_recovered() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let raw = profiles.join("old/session.profraw");
    let merged = merged_pgo_profile(&profiles);
    let bolt = profiles.join("foo/session.fdata");
    write_contents(&raw, "old raw profile");
    write_contents(&merged, "old merged profile");
    write_contents(&bolt, "bolt profile");
    let tools = project.path("blocking-pgo-tools");
    std::fs::create_dir_all(&tools)?;
    let (marker, stop, done) = add_blocking_cargo(&tools)?;

    interrupt_after_profile_reset(
        &project,
        &["run", "--profiles-dir", profiles.to_str().unwrap()],
        &tools,
        &marker,
        &stop,
        &done,
    )?;
    assert_contents(&bolt, "bolt profile");

    let recovery_profile = profiles.join("new-after-interrupted-recovery.profraw");
    write_contents(
        &project.path("src/main.rs"),
        "fn main() { panic!(\"fail after interrupted recovery\"); }\n",
    );
    project
        .cmd(&["run", "--profiles-dir", profiles.to_str().unwrap()])
        .env("LLVM_PROFILE_FILE", recovery_profile.to_str().unwrap())
        .run()?
        .assert_error();

    assert_contents(&raw, "old raw profile");
    assert_contents(&merged, "old merged profile");
    assert_contents(&bolt, "bolt profile");
    assert!(recovery_profile.is_file());
    write_contents(&project.path("src/main.rs"), "fn main() {}\n");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let dot_profile = profiles.join(".profraw");
        let nested_profile = profiles.join("nested/session.profraw");
        #[cfg(not(target_os = "macos"))]
        let non_utf8_profile = non_utf8_profile_path(&profiles.join("non-utf8"), b".profraw");
        let linked_profile_target = project.path("external-optimize-profile.profraw");
        let linked_profile = profiles.join("linked.profraw");
        write_contents(&dot_profile, "synthetic custom-root profile");
        write_contents(&nested_profile, "nested custom-root profile");
        #[cfg(not(target_os = "macos"))]
        write_contents(&non_utf8_profile, "non-UTF-8 custom-root profile");
        write_contents(&linked_profile_target, "external linked profile");
        symlink(&linked_profile_target, &linked_profile)?;
        write_contents(&profiles.join("nested/session.profraw.tmp"), "near match");
        write_contents(
            &profiles.join("nested/session.PROFRAW"),
            "uppercase near match",
        );

        let optimize_tools = fake_pgo_optimize_tools(&project, "pgo-optimize-tools")?;
        let real_cargo = which::which("cargo")?;
        let real_rustc = which::which("rustc")?;
        let fake_target_libdir = optimize_tools.join("missing-target-libdir");
        let expected_profile_inputs =
            std::env::join_paths([&raw, &recovery_profile, &dot_profile, &nested_profile])?;
        project
            .cmd(&["optimize", "--profiles-dir", profiles.to_str().unwrap()])
            .prepend_path(&optimize_tools)
            .env("REAL_CARGO", real_cargo.to_str().unwrap())
            .env("REAL_RUSTC", real_rustc.to_str().unwrap())
            .env("FAKE_TARGET_LIBDIR", fake_target_libdir.to_str().unwrap())
            .env("EXPECTED_PROFILE_ROOT", profiles.to_str().unwrap())
            .env(
                "EXPECTED_PROFILE_INPUTS",
                expected_profile_inputs.to_str().unwrap(),
            )
            .env(
                "EXPECT_NON_UTF8_PROFILE",
                if cfg!(target_os = "macos") { "0" } else { "1" },
            )
            .run()?
            .assert_ok();

        assert!(
            std::fs::symlink_metadata(&linked_profile)?
                .file_type()
                .is_symlink()
        );
        assert_contents(&linked_profile_target, "external linked profile");

        for path in [&marker, &stop, &done] {
            std::fs::remove_file(path)?;
        }
        let optimize_recovery_profiles = project.path("shared/optimize-recovery-profiles");
        let optimize_recovery_raw = optimize_recovery_profiles.join("old/session.profraw");
        write_contents(&optimize_recovery_raw, "old optimize-recovery profile");
        interrupt_after_profile_reset(
            &project,
            &[
                "run",
                "--profiles-dir",
                optimize_recovery_profiles.to_str().unwrap(),
            ],
            &tools,
            &marker,
            &stop,
            &done,
        )?;
        let expected_profile_inputs = std::env::join_paths([&optimize_recovery_raw])?;
        project
            .cmd(&[
                "optimize",
                "--profiles-dir",
                optimize_recovery_profiles.to_str().unwrap(),
            ])
            .prepend_path(&optimize_tools)
            .env("REAL_CARGO", real_cargo.to_str().unwrap())
            .env("REAL_RUSTC", real_rustc.to_str().unwrap())
            .env("FAKE_TARGET_LIBDIR", fake_target_libdir.to_str().unwrap())
            .env(
                "EXPECTED_PROFILE_ROOT",
                optimize_recovery_profiles.to_str().unwrap(),
            )
            .env(
                "EXPECTED_PROFILE_INPUTS",
                expected_profile_inputs.to_str().unwrap(),
            )
            .run()?
            .assert_ok();
        assert_contents(&optimize_recovery_raw, "old optimize-recovery profile");
    }

    let default_profiles = project.path("target/pgo-profiles");
    let default_raw = default_profiles.join("old/session.profraw");
    let default_merged = merged_pgo_profile(&default_profiles);
    write_contents(&default_raw, "old default raw profile");
    write_contents(&default_merged, "old default merged profile");
    let default_tools = project.path("blocking-default-pgo-tools");
    std::fs::create_dir_all(&default_tools)?;
    let (default_marker, default_stop, default_done) = add_blocking_cargo(&default_tools)?;
    interrupt_after_profile_reset(
        &project,
        &[
            "build",
            "--profiles-dir",
            default_profiles.to_str().unwrap(),
        ],
        &default_tools,
        &default_marker,
        &default_stop,
        &default_done,
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let target_alias = project.path("default-pgo-target-alias");
        symlink(project.path("target"), &target_alias)?;
        project
            .run(&[
                "run",
                "--keep-profiles",
                "--",
                "--target-dir",
                target_alias.to_str().unwrap(),
            ])?
            .assert_ok();
    }
    #[cfg(not(unix))]
    project.run(&["run", "--keep-profiles"])?.assert_ok();

    assert_contents(&default_raw, "old default raw profile");
    assert_contents(&default_merged, "old default merged profile");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn interrupted_custom_bolt_reset_is_recovered() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let profiles = project.path("shared/profiles");
    let raw = profiles.join("foo/session.fdata");
    let merged = profiles.join("foo/merged.profdata");
    let pgo = profiles.join("pgo/session.profraw");
    write_contents(&raw, "old raw profile");
    write_contents(&merged, "old merged profile");
    write_contents(&pgo, "pgo profile");
    let blocking_tools = fake_bolt_tools_named(&project, "blocking-bolt-tools", true)?;
    let (marker, stop, done) = add_blocking_bolt(&blocking_tools)?;

    interrupt_after_profile_reset(
        &project,
        &[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ],
        &blocking_tools,
        &marker,
        &stop,
        &done,
    )?;
    assert_contents(&pgo, "pgo profile");

    let recovery_tools = fake_bolt_tools_named(&project, "recovery-bolt-tools", false)?;
    project
        .cmd(&[
            "bolt",
            "build",
            "--profiles-dir",
            profiles.to_str().unwrap(),
        ])
        .prepend_path(&recovery_tools)
        .run()?
        .assert_error();

    assert_contents(&raw, "old raw profile");
    assert_contents(&merged, "old merged profile");
    assert_contents(&pgo, "pgo profile");
    assert_eq!(
        regular_files_with_contents(&profiles.join("foo"), b"new bolt profile")?.len(),
        1
    );

    for path in [&marker, &stop, &done] {
        std::fs::remove_file(path)?;
    }
    let optimize_recovery_profiles = project.path("shared/bolt-optimize-recovery-profiles");
    let optimize_recovery_raw = optimize_recovery_profiles.join("foo/old.fdata");
    write_contents(&optimize_recovery_raw, "old BOLT optimize-recovery profile");
    interrupt_after_profile_reset(
        &project,
        &[
            "bolt",
            "build",
            "--profiles-dir",
            optimize_recovery_profiles.to_str().unwrap(),
        ],
        &blocking_tools,
        &marker,
        &stop,
        &done,
    )?;
    let optimize_recovery_tools =
        fake_bolt_tools_named(&project, "optimize-recovery-bolt-tools", true)?;
    project
        .cmd(&[
            "bolt",
            "optimize",
            "--profiles-dir",
            optimize_recovery_profiles.to_str().unwrap(),
        ])
        .prepend_path(&optimize_recovery_tools)
        .run()?
        .assert_ok();
    assert_contents(&optimize_recovery_raw, "old BOLT optimize-recovery profile");

    let default_profiles = project.path("target/bolt-profiles");
    let default_raw = default_profiles.join("foo/session.fdata");
    let default_merged = default_profiles.join("foo/merged.profdata");
    write_contents(&default_raw, "old default raw profile");
    write_contents(&default_merged, "old default merged profile");
    let default_blocking_tools =
        fake_bolt_tools_named(&project, "blocking-default-bolt-tools", true)?;
    let (default_marker, default_stop, default_done) = add_blocking_bolt(&default_blocking_tools)?;
    interrupt_after_profile_reset(
        &project,
        &[
            "bolt",
            "build",
            "--profiles-dir",
            default_profiles.to_str().unwrap(),
        ],
        &default_blocking_tools,
        &default_marker,
        &default_stop,
        &default_done,
    )?;

    let default_recovery_tools =
        fake_bolt_tools_named(&project, "recovery-default-bolt-tools", true)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let target_alias = project.path("default-bolt-target-alias");
        symlink(project.path("target"), &target_alias)?;
        project
            .cmd(&[
                "bolt",
                "build",
                "--keep-profiles",
                "--",
                "--target-dir",
                target_alias.to_str().unwrap(),
            ])
            .prepend_path(&default_recovery_tools)
            .run()?
            .assert_ok();
    }
    #[cfg(not(unix))]
    project
        .cmd(&["bolt", "build", "--keep-profiles"])
        .prepend_path(&default_recovery_tools)
        .run()?
        .assert_ok();

    assert_contents(&default_raw, "old default raw profile");
    assert_contents(&default_merged, "old default merged profile");
    Ok(())
}
