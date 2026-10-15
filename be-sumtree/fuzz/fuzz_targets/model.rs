#![no_main]

use be_sumtree::model::{Op, run};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|ops: Vec<Op>| run(&ops));
