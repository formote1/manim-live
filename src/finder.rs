use anyhow::Result;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// Discovers animation .py files from:
/// 1. Recent files in ~/.config/manim-live/settings.json
/// 2. Current working directory & subdirectories (depth 4)
/// 3. Common user directories (~/Animaitons, ~/Animations, ~/Projects, ~/Documents)
pub fn discover_animation_files() -> Vec<PathBuf> {
    let mut discovered: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // 1. Files from settings.json
    let store = crate::config::SettingsStore::load();
    for file_str in store.files.keys() {
        let p = PathBuf::from(file_str);
        if p.exists() && p.is_file() {
            if let Ok(canon) = fs::canonicalize(&p) {
                if seen.insert(canon.clone()) {
                    discovered.push(canon);
                }
            } else if seen.insert(p.clone()) {
                discovered.push(p);
            }
        }
    }

    // 2. Current working directory (depth 4)
    if let Ok(cwd) = std::env::current_dir() {
        scan_dir_for_py(&cwd, 0, 4, &mut discovered, &mut seen);
    }

    // 3. Common user directories
    if let Some(home) = dirs_home() {
        let candidate_roots = [
            home.join("Animaitons"),
            home.join("Animations"),
            home.join("Projects"),
            home.join("Documents"),
            home.join("manim"),
        ];

        for root in candidate_roots {
            if root.exists() && root.is_dir() {
                scan_dir_for_py(&root, 0, 4, &mut discovered, &mut seen);
            }
        }
    }

    discovered
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn scan_dir_for_py(
    dir: &Path,
    current_depth: usize,
    max_depth: usize,
    results: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    if current_depth > max_depth {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden directories, target, node_modules, build, venvs
        if name_str.starts_with('.')
            || name_str == "target"
            || name_str == "node_modules"
            || name_str == "__pycache__"
            || name_str == "venv"
            || name_str == ".venv"
            || name_str == "build"
            || name_str == "dist"
        {
            continue;
        }

        if path.is_file() {
            if path.extension().and_then(|ext| ext.to_str()) == Some("py") {
                if let Ok(canon) = fs::canonicalize(&path) {
                    if seen.insert(canon.clone()) {
                        results.push(canon);
                    }
                } else if seen.insert(path.clone()) {
                    results.push(path);
                }
            }
        } else if path.is_dir() {
            scan_dir_for_py(&path, current_depth + 1, max_depth, results, seen);
        }
    }
}

/// Dynamically finds an existing animation file matching a query string
/// (which can be a bare filename like "scene_1.py", a stem like "scene_1", or a relative/absolute path).
pub fn find_matching_file(query: &str) -> Option<PathBuf> {
    let raw_path = Path::new(query);

    // 1. Direct path check
    if raw_path.exists() && raw_path.is_file() {
        return fs::canonicalize(raw_path).ok().or(Some(raw_path.to_path_buf()));
    }

    // 2. Direct path with .py appended
    if raw_path.extension().is_none() {
        let with_py = raw_path.with_extension("py");
        if with_py.exists() && with_py.is_file() {
            return fs::canonicalize(&with_py).ok().or(Some(with_py));
        }
    }

    // 3. Search across all discovered animation files
    let all_files = discover_animation_files();

    let query_name = raw_path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    let query_stem = raw_path.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    let query_with_py = if query_name.ends_with(".py") {
        query_name.clone()
    } else {
        format!("{}.py", query_name)
    };

    // Pass 1: Exact filename match (case-sensitive)
    let raw_name = raw_path.file_name().unwrap_or_default().to_string_lossy();
    let raw_name_with_py = if raw_name.ends_with(".py") {
        raw_name.to_string()
    } else {
        format!("{}.py", raw_name)
    };
    for file in &all_files {
        if let Some(fname) = file.file_name() {
            if fname == raw_name_with_py.as_str() {
                return Some(file.clone());
            }
        }
    }

    // Pass 2: Case-insensitive filename match
    for file in &all_files {
        if let Some(fname) = file.file_name() {
            let fname_lower = fname.to_string_lossy().to_lowercase();
            if fname_lower == query_with_py || fname_lower == query_name {
                return Some(file.clone());
            }
        }
    }

    // Pass 3: Stem match (e.g. "scene_1" matching "scene_1.py")
    for file in &all_files {
        if let Some(fstem) = file.file_stem() {
            let fstem_lower = fstem.to_string_lossy().to_lowercase();
            if fstem_lower == query_stem {
                return Some(file.clone());
            }
        }
    }

    // Pass 4: Substring/Fuzzy match on filename
    for file in &all_files {
        if let Some(fname) = file.file_name() {
            let fname_lower = fname.to_string_lossy().to_lowercase();
            if fname_lower.contains(&query_stem) {
                return Some(file.clone());
            }
        }
    }

    None
}

/// Resolves the target file dynamically from an optional CLI path argument.
/// If omitted, defaults to the latest active file or first discovered animation file.
pub fn resolve_target_file(requested: Option<&Path>) -> Result<PathBuf> {
    if let Some(req) = requested {
        let req_str = req.to_string_lossy();
        if let Some(matched) = find_matching_file(&req_str) {
            return Ok(matched);
        }

        // If not found anywhere, check if extension is missing
        let target = if req.extension().is_none() {
            req.with_extension("py")
        } else {
            req.to_path_buf()
        };

        return Ok(target);
    }

    // If no argument was provided: check discovered animation files
    let discovered = discover_animation_files();
    if let Some(first) = discovered.first() {
        return Ok(first.clone());
    }

    // Default to scene.py in current directory
    Ok(PathBuf::from("scene.py"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_matching_file_exact_and_stem() {
        let dir = tempdir().unwrap();
        let scene_path = dir.path().join("my_animation.py");
        fs::write(&scene_path, "# animation").unwrap();

        let canon_scene = fs::canonicalize(&scene_path).unwrap();

        // 1. Match by absolute path
        assert_eq!(
            find_matching_file(&canon_scene.to_string_lossy()),
            Some(canon_scene.clone())
        );

        // 2. Match by exact file path without extension
        let without_ext = canon_scene.with_extension("");
        assert_eq!(
            find_matching_file(&without_ext.to_string_lossy()),
            Some(canon_scene.clone())
        );
    }

    #[test]
    fn test_resolve_target_file_fallback() {
        let resolved = resolve_target_file(Some(Path::new("brand_new_scene")));
        assert_eq!(resolved.unwrap(), PathBuf::from("brand_new_scene.py"));
    }

    #[test]
    fn test_resolve_bare_filename_and_stem() {
        // If scene_1.py exists on this machine, test that bare name resolves to it
        let real_scene = PathBuf::from("/home/chuck/Animaitons/manimaitons/scene_1.py");
        if real_scene.exists() {
            let canon = fs::canonicalize(&real_scene).unwrap();
            assert_eq!(find_matching_file("scene_1.py"), Some(canon.clone()));
            assert_eq!(find_matching_file("scene_1"), Some(canon));
        }
    }
}
