use std::io::{Read, Write};
use std::path::PathBuf;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn stop(code: i32) -> ! {
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" { fn _exit(code: i32) -> !; }
        _exit(code)
    }
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" { fn ExitProcess(code: u32) -> !; }
        ExitProcess(code as u32)
    }
}

#[inline(never)]
fn exercise(value: usize) -> usize {
    if value > 3 { value * 7 } else { value + 12 }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("ok");
    let mut input = Vec::new();
    if mode == "hold-stdin" {
        use std::process::{Command, Stdio};
        let mut helper = Command::new(std::env::var_os("HOLDER_PYTHON").unwrap())
            .arg(std::env::var_os("HOLDER_SCRIPT").unwrap())
            .arg(std::env::var_os("HOLDER_READY").unwrap())
            .arg(std::env::var_os("HOLDER_RELEASE").unwrap())
            .arg(std::env::var_os("HOLDER_DONE").unwrap())
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn().unwrap();
        // The descendant has inherited stdin before this workload can finish.
        let mut ready = [0];
        helper.stdout.take().unwrap().read_exact(&mut ready).unwrap();
        assert_eq!(ready, [b'R']);
    } else if mode != "ignore-stdin" {
        std::io::stdin().read_to_end(&mut input).unwrap();
    }
    let mut record = String::new();
    for arg in &args { record.push_str(&format!("arg={}\n", hex(arg.as_bytes()))); }
    record.push_str(&format!("stdin={}\n", hex(&input)));
    record.push_str(&format!("cwd={}\n", std::env::current_dir().unwrap().display()));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        record.push_str(&format!("cwd_bytes={}\n", hex(std::env::current_dir().unwrap().as_os_str().as_bytes())));
    }
    record.push_str(&format!("crate={}\n", env!("CARGO_CRATE_NAME")));
    record.push_str(&format!("executable={}\n", std::env::current_exe().unwrap().display()));
    for name in ["TRAIN_VALUE", "TRAIN_REMOVE", "TRAIN_PARENT", "llvm_profile_file"] {
        let value = std::env::var(name).map(|s| hex(s.as_bytes())).unwrap_or("NONE".into());
        record.push_str(&format!("{name}={value}\n"));
    }
    let records = PathBuf::from(std::env::var_os("RECORD_DIR").unwrap());
    std::fs::create_dir_all(&records).unwrap();
    let mut observation = None;
    for n in 0.. {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(records.join(format!("{n:06}"))) {
            Ok(mut file) => {
                file.write_all(record.as_bytes()).unwrap();
                observation = Some(records.join(format!("{n:06}")));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => panic!("{e}"),
        }
    }
    println!("probe stdout: {}", exercise(args.len()));
    eprintln!("probe stderr");
    std::io::stdout().flush().unwrap();
    if mode == "real" { return; }
    if mode == "realfail" { std::process::exit(9); }
    if mode == "remove-cwd" {
        std::fs::remove_dir(std::env::var_os("REMOVE_DIR").unwrap()).unwrap();
    }
    unsafe extern "C" {
        fn __llvm_profile_get_filename() -> *const std::ffi::c_char;
        fn __llvm_profile_get_size_for_buffer() -> u64;
        fn __llvm_profile_write_buffer(buffer: *mut std::ffi::c_char) -> i32;
    }
    // Ask LLVM to expand its public output pattern instead of assuming a filename layout.
    let pattern = PathBuf::from(unsafe {
        std::ffi::CStr::from_ptr(__llvm_profile_get_filename()).to_string_lossy().into_owned()
    });
    let root = pattern.parent().unwrap();
    std::fs::create_dir_all(root).unwrap();
    let payload = std::env::var("PAYLOAD").unwrap_or_else(|_| "accepted".into());
    for byte in payload.bytes() {
        for _ in 0..byte { std::hint::black_box(exercise(byte as usize)); }
    }
    if mode == "none" { stop(0); }
    if mode == "empty" { std::fs::write(root.join("same.profraw"), b"").unwrap(); stop(0); }
    let mut raw = vec![0u8; unsafe { __llvm_profile_get_size_for_buffer() } as usize];
    assert_eq!(unsafe { __llvm_profile_write_buffer(raw.as_mut_ptr().cast()) }, 0);
    let mut observation = std::fs::OpenOptions::new().append(true).open(observation.unwrap()).unwrap();
    writeln!(observation, "profile={}", hex(&raw)).unwrap();
    #[cfg(unix)]
    if mode == "copy-error" || mode == "copy-retry" {
        use std::os::unix::fs::PermissionsExt;
        // Keep directories searchable: enumeration succeeds, but reading one source does not.
        for name in ["a.profraw", "b.profraw", "c.profraw"] {
            std::fs::write(root.join(name), &raw).unwrap();
        }
        if mode == "copy-error" {
            let unreadable = root.join(args.get(1).unwrap());
            std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert!(std::fs::read(&unreadable).is_err(), "fixture must enforce ordinary file permissions");
        }
        stop(0);
    }
    #[cfg(unix)]
    if mode == "sealed" {
        use std::os::unix::fs::PermissionsExt;
        let sealed = root.join("sealed");
        std::fs::create_dir(&sealed).unwrap();
        std::fs::write(sealed.join("sample.profraw"), &raw).unwrap();
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(std::fs::write(sealed.join("permission-probe"), b"denied").is_err(),
            "fixture must enforce ordinary directory permissions");
        stop(0);
    }
    std::fs::write(root.join("same.profraw"), &raw).unwrap();
    if mode == "multi" || mode == "links" {
        std::fs::create_dir_all(root.join("nested/deeper")).unwrap();
        std::fs::create_dir_all(root.join("folder.profraw")).unwrap();
        std::fs::write(root.join("folder.profraw/notes.txt"), b"directory, not a profile").unwrap();
        std::fs::write(root.join("nested/.profraw"), &raw).unwrap();
        std::fs::write(root.join("nested/deeper/same.profraw"), &raw).unwrap();
        std::fs::write(root.join("extra.profraw"), &raw).unwrap();
        std::fs::write(root.join("empty.profraw"), b"").unwrap();
        std::fs::write(root.join("other.txt"), b"not a profile").unwrap();
        std::fs::write(root.join("other.PROFRAW"), &raw).unwrap();
    }
    #[cfg(unix)]
    if mode == "links" {
        use std::os::unix::ffi::OsStringExt;
        let byte_name = if cfg!(target_os = "macos") {
            std::ffi::OsString::from("raw-macos.profraw")
        } else {
            std::ffi::OsString::from_vec(b"raw-\xff.profraw".to_vec())
        };
        std::fs::write(root.join(byte_name), &raw).unwrap();
        let outside = PathBuf::from(std::env::var_os("OUTSIDE").unwrap());
        std::fs::write(outside.join("outside.profraw"), &raw).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("outside.profraw"), root.join("link.profraw")).unwrap();
    }
    stop(if mode == "fail" { 7 } else { 0 });
}
