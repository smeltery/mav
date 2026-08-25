#[test]
fn test_join_write_paths_resolves_relative_and_absolute() {
    let base = PathBuf::from(if cfg!(windows) {
        "C:\\project"
    } else {
        "/project"
    });
    let abs = if cfg!(windows) {
        "C:\\abs\\path"
    } else {
        "/abs/path"
    };
    let joined = join_write_paths(
        &[
            abs.to_string(),
            "relative/dir".to_string(),
            "file.txt".to_string(),
        ],
        Some(base.as_path()),
        cfg!(windows),
    );
    assert_eq!(
        joined,
        vec![
            PathBuf::from(abs),
            base.join("relative/dir"),
            base.join("file.txt"),
        ]
    );
}

#[test]
fn test_join_write_paths_drops_relative_without_base() {
    // Absolute paths still pass through; relative ones are dropped when
    // there's no base to resolve them against.
    let abs = if cfg!(windows) {
        "C:\\abs\\keep"
    } else {
        "/abs/keep"
    };
    let joined = join_write_paths(
        &[abs.to_string(), "relative/drop".to_string()],
        None,
        cfg!(windows),
    );
    assert_eq!(joined, vec![PathBuf::from(abs)]);
}

#[test]
fn test_join_write_paths_converts_wsl_drive_mounts_on_windows() {
    let joined = join_write_paths(
        &["/mnt/c/example/write-root".to_string()],
        Some(Path::new("C:\\project")),
        true,
    );
    assert_eq!(joined, vec![PathBuf::from("C:\\example\\write-root")]);
}

#[test]
fn test_join_write_paths_only_converts_wsl_drive_mounts_for_windows_paths() {
    let joined = join_write_paths(
        &["/mnt/c/example/write-root".to_string()],
        Some(Path::new("/project")),
        false,
    );
    assert_eq!(joined, vec![PathBuf::from("/mnt/c/example/write-root")]);
}

#[test]
fn test_join_write_paths_preserves_wsl_absolute_paths_on_windows() {
    let joined = join_write_paths(
        &["/home/example".to_string()],
        Some(Path::new("C:\\project")),
        true,
    );
    assert_eq!(joined, vec![PathBuf::from("/home/example")]);
}

#[test]
fn test_join_write_paths_normalizes_parent_traversal() {
    let base = PathBuf::from(if cfg!(windows) {
        "C:\\project"
    } else {
        "/project"
    });
    // `..` is resolved lexically so containment checks and the approval
    // prompt see the real target rather than a traversal that the sandbox
    // would canonicalize differently.
    let joined = join_write_paths(
        &[
            "build/../../escape".to_string(),
            if cfg!(windows) {
                "C:\\abs\\a\\..\\b".to_string()
            } else {
                "/abs/a/../b".to_string()
            },
        ],
        Some(base.as_path()),
        cfg!(windows),
    );
    let expected_escape = if cfg!(windows) {
        PathBuf::from("C:\\escape")
    } else {
        PathBuf::from("/escape")
    };
    let expected_abs = if cfg!(windows) {
        PathBuf::from("C:\\abs\\b")
    } else {
        PathBuf::from("/abs/b")
    };
    assert_eq!(joined, vec![expected_escape, expected_abs]);
}
