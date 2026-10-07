//! `modify_file` 写盘时对文件元数据（权限位、xattr、硬链接 inode）保留的回归测试。

use super::tests::make_test_dir;
use super::*;

#[cfg(unix)]
#[test]
fn test_modify_file_replace_lines_preserves_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = make_test_dir();
    let file = dir.join("run.sh");
    std::fs::write(&file, "#!/bin/sh\necho one\necho two\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();

    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"run.sh","mode":"replace_lines","start_line":2,"end_line":2,"content":"echo changed"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "replace_lines 应保留原文件可执行位");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_modify_file_insert_after_line_preserves_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = make_test_dir();
    let file = dir.join("run2.sh");
    std::fs::write(&file, "#!/bin/sh\necho one\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();

    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"run2.sh","mode":"insert_after_line","after_line":1,"content":"echo two"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已插入内容"), "{}", out);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "insert_after_line 应保留原文件可执行位");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_modify_file_replace_lines_preserves_readonly_mode_and_cleans_tmp() {
    use std::os::unix::fs::PermissionsExt;

    let dir = make_test_dir();
    let file = dir.join("ro.txt");
    std::fs::write(&file, "a\nb\nc\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();

    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"ro.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"B"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nB\nc\n");
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o444, "应保留原只读权限位");

    let leftover: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("crabmate_edit_tmp"))
        .collect();
    assert!(leftover.is_empty(), "成功提交后不应残留临时文件: {leftover:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
#[test]
fn test_modify_file_replace_lines_preserves_xattr() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let dir = make_test_dir();
    let file = dir.join("x.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();

    let path_c = CString::new(file.as_os_str().as_bytes()).unwrap();
    let name = CString::new("user.crabmate_test").unwrap();
    let value = b"v1";
    let rc = unsafe {
        libc::setxattr(
            path_c.as_ptr(),
            name.as_ptr(),
            value.as_ptr() as *const libc::c_void,
            value.len(),
            0,
        )
    };
    if rc != 0 {
        // 文件系统不支持 user xattr（如部分 tmpfs）时跳过。
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }

    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"x.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);

    let mut buf = [0u8; 16];
    let n = unsafe {
        libc::getxattr(
            path_c.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
        )
    };
    assert!(n > 0, "扩展属性应在编辑后保留");
    assert_eq!(&buf[..n as usize], value);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn test_modify_file_replace_lines_preserves_hard_link_inode() {
    use std::os::unix::fs::MetadataExt;

    let dir = make_test_dir();
    let file = dir.join("orig.txt");
    let link = dir.join("alias.txt");
    std::fs::write(&file, "L1\nL2\n").unwrap();
    std::fs::hard_link(&file, &link).unwrap();

    let inode_before = std::fs::metadata(&file).unwrap().ino();
    assert_eq!(
        std::fs::metadata(&link).unwrap().ino(),
        inode_before,
        "前置条件：硬链接应指向同一 inode"
    );

    let cfg = crate::cm_config::load_config(None).expect("embedded default config");
    let ctx =
        crate::cm_tools::tools::tool_context_for(&cfg, cfg.command_exec.allowed_commands.as_ref(), &dir);
    let out = modify_file(
        r#"{"path":"orig.txt","mode":"replace_lines","start_line":2,"end_line":2,"content":"X"}"#,
        &dir,
        &ctx,
    );
    assert!(out.contains("已按行替换"), "{}", out);

    let inode_after = std::fs::metadata(&file).unwrap().ino();
    assert_eq!(inode_after, inode_before, "编辑多硬链接文件应保留原 inode");
    assert_eq!(
        std::fs::metadata(&link).unwrap().ino(),
        inode_after,
        "硬链接应仍指向原 inode"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "L1\nX\n");
    assert_eq!(
        std::fs::read_to_string(&link).unwrap(),
        "L1\nX\n",
        "通过硬链接应能看到更新后的内容"
    );

    let leftover: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("crabmate_edit_tmp"))
        .collect();
    assert!(leftover.is_empty(), "成功提交后不应残留临时文件: {leftover:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
