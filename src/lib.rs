#![cfg_attr(feature="alloc_system",feature(alloc_system))]
#[cfg(feature="alloc_system")]
extern crate alloc_system;
extern crate crypto;
#[macro_use]
extern crate serde_json;
extern crate crossbeam;
extern crate walkdir;
extern crate semver;
extern crate zip;
extern crate tempdir;
extern crate libc;
extern crate uuid;
extern crate globset;

mod defs;
pub use defs::*;

mod producer;
pub use producer::*;

mod gcov;
pub use gcov::*;

mod parser;
pub use parser::*;

mod filter;

mod path_rewriting;
pub use path_rewriting::*;

mod output;
pub use output::*;

use std::collections::{btree_map, hash_map};
use std::fs::{self, File};
use std::io::{Cursor, BufReader};
use std::path::PathBuf;
use walkdir::WalkDir;

// Merge results, without caring about duplicate lines (they will be removed at the end).
fn merge_results(result: &mut CovResult, result2: &mut CovResult) {
    for (&line_no, &execution_count) in &result2.lines {
        match result.lines.entry(line_no) {
            btree_map::Entry::Occupied(c) => {
                *c.into_mut() += execution_count;
            },
            btree_map::Entry::Vacant(v) => {
                v.insert(execution_count);
            }
        };
    }

    for (&(line_no, number), &taken) in &result2.branches {
        match result.branches.entry((line_no, number)) {
            btree_map::Entry::Occupied(c) => {
                *c.into_mut() |= taken;
            },
            btree_map::Entry::Vacant(v) => {
                v.insert(taken);
            }
        };
    }

    for (name, function) in result2.functions.drain() {
        match result.functions.entry(name) {
            hash_map::Entry::Occupied(f) => f.into_mut().executed |= function.executed,
            hash_map::Entry::Vacant(v) => {
                v.insert(function);
            }
        };
    }
}

fn add_results(mut results: Vec<(String,CovResult)>, result_map: &SyncCovResultMap, source_dir: &PathBuf) {
    let mut map = result_map.lock().unwrap();
    for mut result in results.drain(..) {
        let path = if source_dir.to_str().unwrap().is_empty() {
            result.0
        } else {
            // the goal here is to be able to merge results for paths like foo/./bar and foo/bar
            match fs::canonicalize(source_dir.join(PathBuf::from(&result.0))) {
                Ok(p) => String::from(p.to_str().unwrap()),
                Err(_) => result.0,
            }
        };

        match map.entry(path) {
            hash_map::Entry::Occupied(obj) => {
                merge_results(obj.into_mut(), &mut result.1);
            },
            hash_map::Entry::Vacant(v) => {
                v.insert(result.1);
            }
        };
    }
}

// Some versions of GCC, because of a bug, generate multiple gcov files for each
// gcno, so we have to support this case too for the time being.
#[derive(PartialEq, Eq)]
enum GcovType {
    Unknown,
    SingleFile,
    MultipleFiles,
}

macro_rules! try_parse {
    ($v:expr, $f:expr) => (match $v {
        Ok(val) => val,
        Err(err) => {
            eprintln!("Error parsing file {}:", $f);
            eprintln!("{}", err);
            std::process::exit(1);
        }
    });
}

pub fn consumer(working_dir: &PathBuf, source_dir: &PathBuf, result_map: &SyncCovResultMap, queue: &WorkQueue, is_llvm: bool, branch_enabled: bool) {
    let mut gcov_type = GcovType::Unknown;

    while let Some(work_item) = queue.pop() {
        let new_results = match work_item.format {
            ItemFormat::GCNO => {
                let gcno_path = work_item.path();

                if !is_llvm {
                    run_gcov(gcno_path, branch_enabled, working_dir);
                } else {
                    call_parse_llvm_gcno(working_dir.to_str().unwrap(), gcno_path.parent().unwrap().join(gcno_path.file_stem().unwrap()).to_str().unwrap(), branch_enabled);
                }

                let gcov_path = working_dir.join(gcno_path.file_name().unwrap().to_str().unwrap().to_string() + ".gcov");
                if gcov_type == GcovType::Unknown {
                    gcov_type = if gcov_path.exists() {
                        GcovType::SingleFile
                    } else {
                        GcovType::MultipleFiles
                    };
                }

                if gcov_type == GcovType::SingleFile {
                    let new_results = try_parse!(parse_gcov(&gcov_path), work_item.name);
                    fs::remove_file(gcov_path).unwrap();
                    new_results
                } else {
                    let mut new_results: Vec<(String,CovResult)> = Vec::new();

                    for entry in WalkDir::new(&working_dir).min_depth(1) {
                        let gcov_path = entry.unwrap();
                        let gcov_path = gcov_path.path();

                        new_results.append(&mut try_parse!(parse_gcov(&gcov_path), work_item.name));

                        fs::remove_file(gcov_path).unwrap();
                    }

                    new_results
                }
            },
            ItemFormat::INFO => {
                match work_item.item {
                    ItemType::Path(info_path) => {
                        let f = File::open(&info_path).expect("Failed to open lcov file");
                        let file = BufReader::new(&f);
                        try_parse!(parse_lcov(file, branch_enabled), work_item.name)
                    },
                    ItemType::Content(info_content) => {
                        let buffer = BufReader::new(Cursor::new(info_content));
                        try_parse!(parse_lcov(buffer, branch_enabled), work_item.name)
                    }
                }
            }
        };

        add_results(new_results, result_map, source_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_merge_results() {
        let mut functions1: HashMap<String,Function> = HashMap::new();
        functions1.insert("f1".to_string(), Function {
            start: 1,
            executed: false,
        });
        functions1.insert("f2".to_string(), Function {
            start: 2,
            executed: false,
        });
        let mut result = CovResult {
            lines: [(1, 21),(2, 7),(7,0)].iter().cloned().collect(),
            branches: [((1, 0), false), ((1, 1), false), ((2, 0), false), ((2, 1), true), ((4, 0), true)].iter().cloned().collect(),
            functions: functions1,
        };
        let mut functions2: HashMap<String,Function> = HashMap::new();
        functions2.insert("f1".to_string(), Function {
            start: 1,
            executed: false,
        });
        functions2.insert("f2".to_string(), Function {
            start: 2,
            executed: true,
        });
        let mut result2 = CovResult {
            lines: [(1,21),(3,42),(4,7),(2,0),(8,0)].iter().cloned().collect(),
            branches: [((1, 0), false), ((1, 1), false), ((2, 0), true), ((2, 1), false), ((3, 0), true)].iter().cloned().collect(),
            functions: functions2,
        };

        merge_results(&mut result, &mut result2);
        assert_eq!(result.lines, [(1,42),(2,7),(3,42),(4,7),(7,0),(8,0)].iter().cloned().collect());
        assert_eq!(result.branches, [((1, 0), false), ((1, 1), false), ((2, 0), true), ((2, 1), true), ((3, 0), true), ((4, 0), true)].iter().cloned().collect());
        assert!(result.functions.contains_key("f1"));
        assert!(result.functions.contains_key("f2"));
        let mut func = result.functions.get("f1").unwrap();
        assert_eq!(func.start, 1);
        assert_eq!(func.executed, false);
        func = result.functions.get("f2").unwrap();
        assert_eq!(func.start, 2);
        assert_eq!(func.executed, true);
    }

    #[test]
    fn test_merge_relative_path() {
        let f = File::open("./test/relative_path/relative_path.info").expect("Failed to open lcov file");
        let file = BufReader::new(&f);
        let results = parse_lcov(file, false).unwrap();
        let result_map: Arc<SyncCovResultMap> = Arc::new(Mutex::new(HashMap::with_capacity(1)));
        add_results(results, &result_map, &PathBuf::from("./test/relative_path"));
        let result_map = Arc::try_unwrap(result_map).unwrap().into_inner().unwrap();

        assert!(result_map.len() == 1);

        let cpp_file = fs::canonicalize(PathBuf::from("./test/relative_path/foo/bar/oof.cpp")).unwrap();
        let cpp_file = cpp_file.to_str().unwrap();
        let cov_result = result_map.get(cpp_file).unwrap();

        assert_eq!(cov_result.lines, [(1,63), (2,63), (3,84), (4,42)].iter().cloned().collect());
        assert!(cov_result.functions.contains_key("myfun"));
    }
}
