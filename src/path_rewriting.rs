use globset::{Glob, GlobSetBuilder};
use serde_json::Value;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use defs::*;
use filter::*;

fn to_lowercase_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().unwrap().to_lowercase().collect::<String>() + c.as_str()
}

fn to_uppercase_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().unwrap().to_uppercase().collect::<String>() + c.as_str()
}

fn find_in(mapping: &Option<Value>, path: &PathBuf) -> Option<PathBuf> {
    match mapping {
        Some(mapping) => {
            let path = path.to_str().unwrap();
            match mapping.get(to_lowercase_first(path)) {
                Some(p) => Some(PathBuf::from(p.as_str().unwrap())),
                None => match mapping.get(to_uppercase_first(path)) {
                    Some(p) => Some(PathBuf::from(p.as_str().unwrap())),
                    None => None,
                },
            }
        }
        None => None,
    }
}

fn remove_common(prefix_dir: &PathBuf, path: &PathBuf) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if prefix_dir.ends_with(ancestor) {
            return Some(path.strip_prefix(ancestor).unwrap().to_path_buf());
        }
    }
    None
}

fn remove_prefix(prefix_dir: &Option<PathBuf>, path: &PathBuf) -> Option<PathBuf> {
    match prefix_dir {
        Some(prefix_dir) => {
            if prefix_dir.is_absolute() {
                remove_common(&prefix_dir, path)
            } else if path.starts_with(&prefix_dir) {
                Some(path.strip_prefix(&prefix_dir).unwrap().to_path_buf())
            } else {
                None
            }
        }
        None => None,
    }
}

pub fn canonicalize_path<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)?;

    #[cfg(windows)]
    let path = match {
        let spath = path.to_str().unwrap();
        if spath.starts_with(r"\\?\") {
            Some(PathBuf::from(spath[r"\\?\".len()..].to_string()))
        } else {
            None
        }
    } {
        Some(p) => p,
        None => path,
    };

    Ok(path)
}

pub fn rewrite_paths(
    result_map: CovResultMap,
    path_mapping: Option<Value>,
    source_dir: Option<PathBuf>,
    prefix_dir: Option<PathBuf>,
    prepend_prefix_dir: Option<PathBuf>,
    ignore_global: bool,
    ignore_not_existing: bool,
    to_ignore_dirs: Vec<String>,
    filter_option: Option<bool>,
) -> CovResultIter {
    let mut glob_builder = GlobSetBuilder::new();
    for to_ignore_dir in to_ignore_dirs {
        glob_builder.add(Glob::new(&to_ignore_dir).unwrap());
    }
    let to_ignore_globset = glob_builder.build().unwrap();

    Box::new(result_map.into_iter().filter_map(move |(path, result)| {
        /* The goal is to get an absolute and canonical path to the file
           and a path relative to source directory.
           In real life, paths in gcno can be aboslute (canonical or not)
           or relative (relative to source dir, relative to cwd, relative to ???).
         */

        let path = match &prepend_prefix_dir {
            Some(prepend_path) => prepend_path.join(PathBuf::from(path)),
            None => PathBuf::from(path),
        };

        let path = PathBuf::from(path.to_str().unwrap().replace("\\", "/"));

        let (rel_path, found_in_mapping) = match find_in(&path_mapping, &path) {
            Some(p) => (p, true),
            None => (
                match remove_prefix(&prefix_dir, &path) {
                    Some(p) => p,
                    None => match remove_prefix(&source_dir, &path) {
                        Some(p) => p,
                        None => path,
                    },
                },
                false,
            ),
        };

        if ignore_global && !rel_path.is_relative() {
            return None;
        }

        // Get absolute path to source file.
        let abs_path = if rel_path.is_relative() {
            let rel_path = if !cfg!(windows) {
                rel_path.clone()
            } else {
                PathBuf::from(rel_path.to_str().unwrap().replace("/", "\\"))
            };
            match &source_dir {
                Some(source_dir) => source_dir.join(rel_path),
                None => rel_path,
            }
        } else {
            rel_path.clone()
        };

        // Canonicalize, if possible.
        let abs_path = match canonicalize_path(&abs_path) {
            Ok(p) => p,
            Err(_) => abs_path,
        };

        if ignore_not_existing && !abs_path.exists() {
            return None;
        }

        let rel_path = if found_in_mapping {
            rel_path
        } else {
            match &source_dir {
                Some(source_dir) => if abs_path.starts_with(&source_dir) {
                    abs_path.strip_prefix(&source_dir).unwrap().to_path_buf()
                } else {
                    abs_path.clone()
                },
                None => abs_path.clone(),
            }
        };

        if to_ignore_globset.is_match(&rel_path) {
            return None;
        }

        let rel_path = PathBuf::from(rel_path.to_str().unwrap().replace("\\", "/"));

        match filter_option {
            Some(true) => if !is_covered(&result) {
                return None;
            },
            Some(false) => if is_covered(&result) {
                return None;
            },
            None => (),
        };

        Some((abs_path, rel_path, result))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, HashMap};

    #[test]
    fn test_to_lowercase_first() {
        assert_eq!(to_lowercase_first("marco"), "marco");
        assert_eq!(to_lowercase_first("Marco"), "marco");
    }

    #[test]
    #[should_panic]
    fn test_to_lowercase_first_empty() {
        to_lowercase_first("");
    }

    #[test]
    fn test_to_uppercase_first() {
        assert_eq!(to_uppercase_first("marco"), "Marco");
        assert_eq!(to_uppercase_first("Marco"), "Marco");
    }

    #[test]
    #[should_panic]
    fn test_to_uppercase_first_empty() {
        to_uppercase_first("");
    }

    macro_rules! empty_result {
        () => {{
            CovResult {
                lines: BTreeMap::new(),
                branches: BTreeMap::new(),
                functions: HashMap::new(),
            }
        }};
    }

    macro_rules! covered_result {
        () => {{
            CovResult {
                lines: [(42, 1)].iter().cloned().collect(),
                branches: BTreeMap::new(),
                functions: HashMap::new(),
            }
        }};
    }

    macro_rules! uncovered_result {
        () => {{
            CovResult {
                lines: [(42, 0)].iter().cloned().collect(),
                branches: BTreeMap::new(),
                functions: HashMap::new(),
            }
        }};
    }

    #[test]
    fn test_rewrite_paths_basic() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_ignore_global_files() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("/usr/include/prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            true,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_ignore_global_files() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("C:\\usr\\include\\prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            true,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "/home/worker/src/workspace/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            None,
            Some(PathBuf::from("/home/worker/src/workspace/")),
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            None,
            Some(PathBuf::from("C:\\Users\\worker\\src\\workspace\\")),
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_remove_prefix_with_slash() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:/Users/worker/src/workspace/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            None,
            Some(PathBuf::from("C:/Users/worker/src/workspace/")),
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_remove_prefix_with_slash_longer_path() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:/Users/worker/src/workspace/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            None,
            Some(PathBuf::from("C:/Users/worker/src/")),
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("workspace/main.cpp"));
            assert_eq!(rel_path.to_str().unwrap(), "workspace/main.cpp");
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_add_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("org/example/Hello.java".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            Some(PathBuf::from("mobile/android/app/src/java")),
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(
                abs_path,
                PathBuf::from("mobile/android/app/src/java/org/example/Hello.java")
            );
            assert_eq!(
                rel_path,
                PathBuf::from("mobile/android/app/src/java/org/example/Hello.java")
            );
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_add_prefix_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("org/example/Hello.java".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            Some(PathBuf::from("mobile/android")),
            Some(PathBuf::from("mobile/android/app/src/java")),
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(
                abs_path,
                PathBuf::from("app/src/java/org/example/Hello.java")
            );
            assert_eq!(
                rel_path,
                PathBuf::from("app/src/java/org/example/Hello.java")
            );
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_ignore_non_existing_files() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("tests/class/main.cpp".to_string(), empty_result!());
        result_map.insert("tests/class/doesntexist.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            assert!(rel_path.ends_with("tests/class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_ignore_non_existing_files() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("tests\\class\\main.cpp".to_string(), empty_result!());
        result_map.insert("tests\\class\\doesntexist.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert!(rel_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_ignore_a_directory() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("mydir/prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            vec!["mydir/*".to_string()],
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_ignore_a_directory() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("mydir\\prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            vec!["mydir/*".to_string()],
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_ignore_multiple_directories() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("mydir/prova.h".to_string(), empty_result!());
        result_map.insert("mydir2/prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            vec!["mydir/*".to_string(), "mydir2/*".to_string()],
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_ignore_multiple_directories() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("main.cpp".to_string(), empty_result!());
        result_map.insert("mydir\\prova.h".to_string(), empty_result!());
        result_map.insert("mydir2\\prova.h".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            vec!["mydir/*".to_string(), "mydir2/*".to_string()],
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("main.cpp"));
            assert_eq!(rel_path, PathBuf::from("main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_source_directory() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("class/main.cpp".to_string(), empty_result!());
        result_map.insert("tests/class/main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            Some(canonicalize_path("tests").unwrap()),
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            assert_eq!(rel_path, PathBuf::from("class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 2);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_source_directory() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("class\\main.cpp".to_string(), empty_result!());
        result_map.insert("tests\\class\\main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            None,
            Some(canonicalize_path("tests").unwrap()),
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 2);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_and_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "/home/worker/src/workspace/class/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            Some(canonicalize_path("tests").unwrap()),
            Some(PathBuf::from("/home/worker/src/workspace")),
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            eprintln!("{:?}", rel_path);
            assert_eq!(rel_path, PathBuf::from("class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_and_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\class\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            None,
            Some(canonicalize_path("tests").unwrap()),
            Some(PathBuf::from("C:\\Users\\worker\\src\\workspace")),
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("class/main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            Some(json!({"class/main.cpp": "rewritten/main.cpp"})),
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("rewritten/main.cpp"));
            assert_eq!(rel_path, PathBuf::from("rewritten/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("class\\main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            Some(json!({"class/main.cpp": "rewritten/main.cpp"})),
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("rewritten\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("rewritten\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_ignore_non_existing() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("rewritten/main.cpp".to_string(), empty_result!());
        result_map.insert("tests/class/main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"rewritten/main.cpp": "tests/class/main.cpp", "tests/class/main.cpp": "rewritten/main.cpp"}),
            ),
            None,
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests/class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_ignore_non_existing() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("rewritten\\main.cpp".to_string(), empty_result!());
        result_map.insert("tests\\class\\main.cpp".to_string(), empty_result!());
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"rewritten/main.cpp": "tests/class/main.cpp", "tests/class/main.cpp": "rewritten/main.cpp"}),
            ),
            None,
            None,
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "/home/worker/src/workspace/rewritten/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(json!({"/home/worker/src/workspace/rewritten/main.cpp": "tests/class/main.cpp"})),
            None,
            Some(PathBuf::from("/home/worker/src/workspace")),
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests/class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_remove_prefix() {
        // Mapping with uppercase disk and prefix with uppercase disk.
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\rewritten\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"C:/Users/worker/src/workspace/rewritten/main.cpp": "tests/class/main.cpp"}),
            ),
            None,
            None,
            Some(PathBuf::from("C:\\Users\\worker\\src\\workspace")),
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);

        // Mapping with lowercase disk and prefix with uppercase disk.
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\rewritten\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"c:/Users/worker/src/workspace/rewritten/main.cpp": "tests/class/main.cpp"}),
            ),
            None,
            None,
            Some(PathBuf::from("C:\\Users\\worker\\src\\workspace")),
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);

        // Mapping with uppercase disk and prefix with lowercase disk.
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\rewritten\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"C:/Users/worker/src/workspace/rewritten/main.cpp": "tests/class/main.cpp"}),
            ),
            None,
            None,
            Some(PathBuf::from("c:\\Users\\worker\\src\\workspace")),
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);

        // Mapping with lowercase disk and prefix with lowercase disk.
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\rewritten\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(
                json!({"c:/Users/worker/src/workspace/rewritten/main.cpp": "tests/class/main.cpp"}),
            ),
            None,
            None,
            Some(PathBuf::from("c:\\Users\\worker\\src\\workspace")),
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("tests\\class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_source_directory_and_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "/home/worker/src/workspace/rewritten/main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(json!({"/home/worker/src/workspace/rewritten/main.cpp": "class/main.cpp"})),
            Some(canonicalize_path("tests").unwrap()),
            Some(PathBuf::from("/home/worker/src/workspace")),
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests/class/main.cpp"));
            assert_eq!(rel_path, PathBuf::from("class/main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    #[test]
    fn test_rewrite_paths_rewrite_path_using_mapping_and_source_directory_and_remove_prefix() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert(
            "C:\\Users\\worker\\src\\workspace\\rewritten\\main.cpp".to_string(),
            empty_result!(),
        );
        let results = rewrite_paths(
            result_map,
            Some(json!({"C:/Users/worker/src/workspace/rewritten/main.cpp": "class/main.cpp"})),
            Some(canonicalize_path("tests").unwrap()),
            Some(PathBuf::from("C:\\Users\\worker\\src\\workspace")),
            None,
            false,
            true,
            Vec::new(),
            None,
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert!(abs_path.is_absolute());
            assert!(abs_path.ends_with("tests\\class\\main.cpp"));
            assert_eq!(rel_path, PathBuf::from("class\\main.cpp"));
            assert_eq!(result, empty_result!());
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn test_rewrite_paths_only_covered() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("covered.cpp".to_string(), covered_result!());
        result_map.insert("uncovered.cpp".to_string(), uncovered_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
            Some(true),
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("covered.cpp"));
            assert_eq!(rel_path, PathBuf::from("covered.cpp"));
            assert_eq!(result, covered_result!());
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn test_rewrite_paths_only_uncovered() {
        let mut result_map: CovResultMap = HashMap::new();
        result_map.insert("covered.cpp".to_string(), covered_result!());
        result_map.insert("uncovered.cpp".to_string(), uncovered_result!());
        let results = rewrite_paths(
            result_map,
            None,
            None,
            None,
            None,
            false,
            false,
            Vec::new(),
            Some(false),
        );
        let mut count = 0;
        for (abs_path, rel_path, result) in results {
            count += 1;
            assert_eq!(abs_path, PathBuf::from("uncovered.cpp"));
            assert_eq!(rel_path, PathBuf::from("uncovered.cpp"));
            assert_eq!(result, uncovered_result!());
        }
        assert_eq!(count, 1);
    }

}
