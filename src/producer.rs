use std::env;
use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read};
use zip::{self, ZipArchive};
use walkdir::WalkDir;

use defs::*;

fn dir_producer(directories: &[&String], queue: &WorkQueue, ignore_orphan_gcno: bool) -> Option<Vec<u8>> {
    let gcov_ext = Some(OsStr::new(if !ignore_orphan_gcno {
        "gcno"
    } else {
        "gcda"
    }));
    let info_ext = Some(OsStr::new("info"));
    let json_ext = Some(OsStr::new("json"));

    let mut path_mapping_file = None;

    for directory in directories {
        let is_dir_relative = PathBuf::from(directory).is_relative();
        let current_dir = env::current_dir().unwrap();

        for entry in WalkDir::new(&directory) {
            let entry = entry.expect(format!("Failed to open directory '{}'.", directory).as_str());
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension();
                let format = if ext == gcov_ext {
                    ItemFormat::GCNO
                } else if ext == info_ext {
                    ItemFormat::INFO
                } else if ext == json_ext && path.file_name().unwrap() == "linked-files-map.json" {
                    let mut buffer = Vec::new();
                    File::open(path).unwrap().read_to_end(&mut buffer).unwrap();
                    path_mapping_file = Some(buffer);
                    continue
                } else {
                    continue
                };

                let abs_path = if is_dir_relative {
                    current_dir.join(path)
                } else {
                    path.to_path_buf()
                };

                queue.push(Some(WorkItem {
                    format: format,
                    item: ItemType::Path(abs_path),
                    name: path.to_str().unwrap().to_string(),
                }));
            }
        }
    }

    path_mapping_file
}

fn open_archive(path: &str) -> ZipArchive<File> {
    let file = File::open(&path).expect(format!("Failed to open ZIP file '{}'.", path).as_str());
    ZipArchive::new(file).expect(format!("Failed to parse ZIP file: {}", path).as_str())
}

fn extract_file(zip_file: &mut zip::read::ZipFile, path: &PathBuf) {
    let mut file = File::create(&path).expect("Failed to create file");
    io::copy(zip_file, &mut file).expect("Failed to copy file from ZIP");
}

fn zip_producer(tmp_dir: &Path, zip_files: &[&String], queue: &WorkQueue, ignore_orphan_gcno: bool) -> Option<Vec<u8>> {
    let mut gcno_archive: Option<ZipArchive<File>> = None;
    let mut gcda_archives: Vec<(&String,ZipArchive<File>)> = Vec::new();
    let mut info_archives: Vec<(&String,ZipArchive<File>)> = Vec::new();

    let mut path_mapping_file = None;

    for zip_file in zip_files.iter() {
        let archive = open_archive(zip_file);
        if zip_file.contains("gcno") {
            gcno_archive = Some(archive);
        } else if zip_file.contains("gcda") {
            gcda_archives.push((zip_file, archive));
        } else if zip_file.contains("info") || zip_file.contains("grcov") || zip_file.contains("jsvm") {
            info_archives.push((zip_file, archive));
        } else {
            panic!("Unsupported archive type.");
        }
    }

    if gcno_archive.is_some() {
        assert!(!gcda_archives.is_empty());
    }
    if !gcda_archives.is_empty() {
        assert!(gcno_archive.is_some());
    }

    if let Some(mut gcno_archive) = gcno_archive {
        for i in 0..gcno_archive.len() {
            let mut gcno_file = gcno_archive.by_index(i).unwrap();
            if gcno_file.name() == "linked-files-map.json" {
                let mut buffer = Vec::new();
                gcno_file.read_to_end(&mut buffer).unwrap();
                path_mapping_file = Some(buffer);
                continue;
            }

            let gcno_path_in_zip = PathBuf::from(gcno_file.name());

            let path = tmp_dir.join(&gcno_path_in_zip);

            fs::create_dir_all(path.parent().unwrap()).expect("Failed to create directory");

            if gcno_file.name().ends_with('/') {
                fs::create_dir_all(&path).expect("Failed to create directory");
            }
            else {
                let stem = path.file_stem().unwrap().to_str().unwrap();

                let physical_gcno_path = path.with_file_name(format!("{}_{}.gcno", stem, 1));
                extract_file(&mut gcno_file, &physical_gcno_path);

                let gcda_path_in_zip = gcno_path_in_zip.with_extension("gcda");

                for (num, &mut (gcda_archive_name, ref mut gcda_archive)) in gcda_archives.iter_mut().enumerate() {
                    let gcno_path = path.with_file_name(format!("{}_{}.gcno", stem, num + 1));

                    if let Ok(mut gcda_file) = gcda_archive.by_name(&gcda_path_in_zip.to_str().unwrap().replace("\\", "/")) {
                        // Create symlinks.
                        if num != 0 {
                            fs::hard_link(&physical_gcno_path, &gcno_path).expect(format!("Failed to create hardlink {}", gcno_path.display()).as_str());
                        }

                        let gcda_path = path.with_file_name(format!("{}_{}.gcda", stem, num + 1));

                        extract_file(&mut gcda_file, &gcda_path);

                        queue.push(Some(WorkItem {
                            format: ItemFormat::GCNO,
                            item: ItemType::Path(gcno_path),
                            name: gcda_archive_name.to_string(),
                        }));
                    } else if num == 0 && !ignore_orphan_gcno {
                        queue.push(Some(WorkItem {
                            format: ItemFormat::GCNO,
                            item: ItemType::Path(gcno_path),
                            name: gcda_archive_name.to_string(),
                        }));
                    }
                }
            }
        }
    }

    for &mut (archive_name, ref mut archive) in &mut info_archives {
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();

            if file.name().ends_with('/') {
                continue;
            }

            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).unwrap();
            queue.push(Some(WorkItem {
                format: ItemFormat::INFO,
                item: ItemType::Content(buffer),
                name: archive_name.to_string(),
            }));
        }
    }

    path_mapping_file
}

pub fn producer(tmp_dir: &Path, paths: &[String], queue: &WorkQueue, ignore_orphan_gcno: bool) -> Option<Vec<u8>> {
    let mut zip_files = Vec::new();
    let mut directories = Vec::new();

    for path in paths {
        if path.ends_with(".zip") {
            zip_files.push(path);
        } else {
            directories.push(path);
        }
    }

    let ret1 = zip_producer(tmp_dir, &zip_files, queue, ignore_orphan_gcno);
    let ret2 = dir_producer(&directories, queue, ignore_orphan_gcno);

    if ret1.is_some() {
        ret1
    } else if ret2.is_some() {
        ret2
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crossbeam::sync::MsQueue;
    use serde_json::{self, Value};
    use tempdir::TempDir;

    fn check_produced(directory: PathBuf, queue: &WorkQueue, expected: Vec<(ItemFormat,bool,&str,bool)>) {
        let mut vec: Vec<Option<WorkItem>> = Vec::new();

        loop {
            let elem = queue.try_pop();
            if elem.is_none() {
                break;
            }
            vec.push(elem.unwrap());
        }

        for elem in &expected {
            assert!(vec.iter().any(|x| {
                if !x.is_some() {
                    return false;
                }

                let x = x.as_ref().unwrap();

                if x.format != elem.0 {
                    return false;
                }

                match x.item {
                    ItemType::Content(_) => {
                        !elem.1
                    },
                    ItemType::Path(ref p) => {
                        elem.1 && p.ends_with(elem.2)
                    }
                }
            }), "Missing {:?}", elem);
        }

        for v in &vec {
            let v = v.as_ref().unwrap();
            assert!(expected.iter().any(|x| {
                if v.format != x.0 {
                    return false;
                }

                match v.item {
                    ItemType::Content(_) => {
                        !x.1
                    },
                    ItemType::Path(ref p) => {
                        x.1 && p.ends_with(x.2)
                    }
                }
            }), "Unexpected {:?}", v);
        }

        // Make sure we haven't generated duplicated entries.
        assert_eq!(vec.len(), expected.len());

        // Assert file exists and file with the same name but with extension .gcda exists.
        for x in expected.iter() {
            if !x.1 {
                continue;
            }

            let p = directory.join(x.2);
            assert!(p.exists(), "{} doesn't exist", p.display());
            if x.0 == ItemFormat::GCNO {
                let gcda = p.with_file_name(format!("{}.gcda", p.file_stem().unwrap().to_str().unwrap()));
                if x.3 {
                    assert!(gcda.exists(), "{} doesn't exist", gcda.display());
                } else {
                    assert!(!gcda.exists(), "{} exists", gcda.display());
                }
            }
        }
    }

    #[test]
    fn test_dir_producer() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let mapping = dir_producer(&vec![&"test".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "test/Platform.gcno", true),
            (ItemFormat::GCNO, true, "test/sub2/RootAccessibleWrap.gcno", true),
            (ItemFormat::GCNO, true, "test/nsMaiInterfaceValue.gcno", true),
            (ItemFormat::GCNO, true, "test/sub/prova2.gcno", true),
            (ItemFormat::GCNO, true, "test/nsMaiInterfaceDocument.gcno", true),
            (ItemFormat::GCNO, true, "test/Unified_cpp_netwerk_base0.gcno", true),
            (ItemFormat::GCNO, true, "test/prova.gcno", true),
            (ItemFormat::GCNO, true, "test/nsGnomeModule.gcno", true),
            (ItemFormat::GCNO, true, "test/negative_counts.gcno", true),
            (ItemFormat::GCNO, true, "test/64bit_count.gcno", true),
            (ItemFormat::GCNO, true, "test/no_gcda/main.gcno", false),
            (ItemFormat::GCNO, true, "test/only_one_gcda/main.gcno", true),
            (ItemFormat::GCNO, true, "test/only_one_gcda/orphan.gcno", false),
            (ItemFormat::GCNO, true, "test/gcno_symlink/gcda/main.gcno", true),
            (ItemFormat::GCNO, true, "test/gcno_symlink/gcno/main.gcno", false),
            (ItemFormat::GCNO, true, "test/rust/generics_with_two_parameters.gcno", true),
            (ItemFormat::INFO, true, "test/1494603973-2977-7.info", true),
            (ItemFormat::INFO, true, "test/prova.info", true),
            (ItemFormat::INFO, true, "test/prova_fn_with_commas.info", true),
            (ItemFormat::INFO, true, "test/empty_line.info", true),
            (ItemFormat::INFO, true, "test/invalid_DA_record.info", true),
            (ItemFormat::INFO, true, "test/relative_path/relative_path.info", false),
        ];

        check_produced(PathBuf::from("."), &queue, expected);
        assert!(mapping.is_some());
        let mapping: Value = serde_json::from_slice(&mapping.unwrap()).unwrap();
        assert_eq!(mapping.get("dist/include/zlib.h").unwrap().as_str().unwrap(), "modules/zlib/src/zlib.h");
    }

    #[test]
    fn test_dir_producer_multiple_directories() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let mapping = dir_producer(&vec![&"test/sub".to_string(), &"test/sub2".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "test/sub2/RootAccessibleWrap.gcno", true),
            (ItemFormat::GCNO, true, "test/sub/prova2.gcno", true),
        ];

        check_produced(PathBuf::from("."), &queue, expected);
        assert!(mapping.is_none());
    }

    #[test]
    fn test_dir_producer_directory_with_gcno_symlinks() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let mapping = dir_producer(&vec![&"test/gcno_symlink/gcda".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "test/gcno_symlink/gcda/main.gcno", true),
        ];

        check_produced(PathBuf::from("."), &queue, expected);
        assert!(mapping.is_none());
    }

    #[test]
    fn test_dir_producer_directory_with_no_gcda() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let mapping = dir_producer(&vec![&"test/only_one_gcda".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "test/only_one_gcda/main.gcno", true),
            (ItemFormat::GCNO, true, "test/only_one_gcda/orphan.gcno", false),
        ];

        check_produced(PathBuf::from("."), &queue, expected);
        assert!(mapping.is_none());
    }

    #[test]
    fn test_dir_producer_directory_with_no_gcda_ignore_orphan_gcno() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let mapping = dir_producer(&vec![&"test/only_one_gcda".to_string()], &queue, true);

        let expected = vec![
            (ItemFormat::GCNO, true, "test/only_one_gcda/main.gcda", true),
        ];

        check_produced(PathBuf::from("."), &queue, expected);
        assert!(mapping.is_none());
    }

    // Test extracting multiple gcda archives.
    #[test]
    fn test_zip_producer_multiple_gcda_archives() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        let mapping = zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda1.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", true),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_2.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_2.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_2.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_2.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
        assert!(mapping.is_some());
        let mapping: Value = serde_json::from_slice(&mapping.unwrap()).unwrap();
        assert_eq!(mapping.get("dist/include/zlib.h").unwrap().as_str().unwrap(), "modules/zlib/src/zlib.h");
    }

    // Test extracting gcno with no path mapping.
    #[test]
    fn test_zip_producer_gcno_with_no_path_mapping() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        let mapping = zip_producer(&tmp_path, &vec![&"test/gcno_no_path_mapping.zip".to_string(), &"test/gcda1.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", true),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
        assert!(mapping.is_none());
    }

    // Test calling zip_producer with a different order of zip files.
    #[test]
    fn test_zip_producer_different_order_of_zip_files() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcda1.zip".to_string(), &"test/gcno.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", true),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_2.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_2.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_2.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_2.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting info files.
    #[test]
    fn test_zip_producer_info_files() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/info1.zip".to_string(), &"test/info2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::INFO, false, "1494603967-2977-2_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-3_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-4_0.info", true),
            (ItemFormat::INFO, false, "1494603968-2977-5_0.info", true),
            (ItemFormat::INFO, false, "1494603972-2977-6_0.info", true),
            (ItemFormat::INFO, false, "1494603973-2977-7_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-2_1.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-3_1.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-4_1.info", true),
            (ItemFormat::INFO, false, "1494603968-2977-5_1.info", true),
            (ItemFormat::INFO, false, "1494603972-2977-6_1.info", true),
            (ItemFormat::INFO, false, "1494603973-2977-7_1.info", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting both info and gcno/gcda files.
    #[test]
    fn test_zip_producer_both_info_and_gcnogcda_files() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda1.zip".to_string(), &"test/info1.zip".to_string(), &"test/info2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", true),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
            (ItemFormat::INFO, false, "1494603967-2977-2_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-3_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-4_0.info", true),
            (ItemFormat::INFO, false, "1494603968-2977-5_0.info", true),
            (ItemFormat::INFO, false, "1494603972-2977-6_0.info", true),
            (ItemFormat::INFO, false, "1494603973-2977-7_0.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-2_1.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-3_1.info", true),
            (ItemFormat::INFO, false, "1494603967-2977-4_1.info", true),
            (ItemFormat::INFO, false, "1494603968-2977-5_1.info", true),
            (ItemFormat::INFO, false, "1494603972-2977-6_1.info", true),
            (ItemFormat::INFO, false, "1494603973-2977-7_1.info", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting gcno with no associated gcda.
    #[test]
    fn test_zip_producer_gcno_with_no_associated_gcda() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        let mapping = zip_producer(&tmp_path, &vec![&"test/no_gcda/main.gcno.zip".to_string(), &"test/no_gcda/empty.gcda.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "main_1.gcno", false),
        ];

        check_produced(tmp_path, &queue, expected);
        assert!(mapping.is_none());
    }

    // Test extracting gcno with an associated gcda file in only one zip file.
    #[test]
    fn test_zip_producer_gcno_with_associated_gcda_in_only_one_archive() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        let mapping = zip_producer(&tmp_path, &vec![&"test/no_gcda/main.gcno.zip".to_string(), &"test/no_gcda/empty.gcda.zip".to_string(),  &"test/no_gcda/main.gcda.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "main_1.gcno", false),
            (ItemFormat::GCNO, true, "main_2.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
        assert!(mapping.is_none());
    }

    // Test passing a gcno archive with no gcda archive makes zip_producer fail.
    #[test]
    #[should_panic]
    fn test_zip_producer_with_gcno_archive_and_no_gcda_archive() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/no_gcda/main.gcno.zip".to_string()], &queue, false);
    }

    // Test passing a gcda archive with no gcno archive makes zip_producer fail.
    #[test]
    #[should_panic]
    fn test_zip_producer_with_gcda_archive_and_no_gcno_archive() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/no_gcda/main.gcda.zip".to_string()], &queue, false);
    }

    // Test extracting gcno/gcda archives, where a gcno file exist with no matching gcda file.
    #[test]
    fn test_zip_producer_no_matching_gcno() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", false),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", false),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting gcno/gcda archives, where a gcno file exist with no matching gcda file.
    // The gcno file should be produced only once, not twice.
    #[test]
    fn test_zip_producer_no_matching_gcno_two_gcda_archives() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda2.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, false);

        let expected = vec![
            (ItemFormat::GCNO, true, "Platform_1.gcno", false),
            (ItemFormat::GCNO, true, "sub2/RootAccessibleWrap_1.gcno", false),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_2.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_2.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_2.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_2.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting gcno/gcda archives, where a gcno file exist with no matching gcda file and ignore orphan gcno files.
    #[test]
    fn test_zip_producer_no_matching_gcno_ignore_orphan_gcno() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, true);

        let expected = vec![
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }

    // Test extracting gcno/gcda archives, where a gcno file exist with no matching gcda file and ignore orphan gcno files.
    #[test]
    fn test_zip_producer_no_matching_gcno_two_gcda_archives_ignore_orphan_gcno() {
        let queue: Arc<WorkQueue> = Arc::new(MsQueue::new());

        let tmp_dir = TempDir::new("grcov").expect("Failed to create temporary directory");
        let tmp_path = tmp_dir.path().to_owned();
        zip_producer(&tmp_path, &vec![&"test/gcno.zip".to_string(), &"test/gcda2.zip".to_string(), &"test/gcda2.zip".to_string()], &queue, true);

        let expected = vec![
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceValue_2.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_1.gcno", true),
            (ItemFormat::GCNO, true, "sub/prova2_2.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_1.gcno", true),
            (ItemFormat::GCNO, true, "nsMaiInterfaceDocument_2.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_1.gcno", true),
            (ItemFormat::GCNO, true, "nsGnomeModule_2.gcno", true),
        ];

        check_produced(tmp_path, &queue, expected);
    }
}
