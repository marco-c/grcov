use crossbeam::queue::MsQueue;
use libc;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub start: u32,
    pub executed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CovResult {
    pub lines: BTreeMap<u32, u64>,
    pub branches: BTreeMap<(u32, u32), bool>,
    pub functions: HashMap<String, Function>,
}

#[derive(Debug)]
#[repr(C)]
pub struct GCOVResult {
    pub ptr: *mut libc::c_void,
    pub len: libc::size_t,
    pub capacity: libc::size_t,
    pub branch_number: libc::uint32_t,
}

#[derive(Debug, PartialEq, Copy, Clone)]
#[allow(non_camel_case_types)]
pub enum ItemFormat {
    GCNO,
    INFO,
    JACOCO_XML,
}

#[derive(Debug)]
pub struct GcnoBuffers {
    pub stem: String,
    pub gcno_buf: Arc<Vec<u8>>,
    pub gcda_buf: Vec<u8>,
}

#[derive(Debug)]
pub enum ItemType {
    Path(PathBuf),
    Content(Vec<u8>),
    Buffers(GcnoBuffers),
}

#[derive(Debug)]
pub struct WorkItem {
    pub format: ItemFormat,
    pub item: ItemType,
    pub name: String,
}

pub type WorkQueue = MsQueue<Option<WorkItem>>;

pub type CovResultMap = HashMap<String, CovResult>;
pub type SyncCovResultMap = Mutex<CovResultMap>;
pub type CovResultIter = Box<Iterator<Item = (PathBuf, PathBuf, CovResult)>>;
