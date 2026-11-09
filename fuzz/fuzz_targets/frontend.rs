//! frontend（字句解析・構文解析・評価）全体を探索する fuzz target。
//!
//! 非 UTF-8 の入力は対象外として `Corpus::Reject` で corpus に残さない（`&str` 受けで捨てるだけだと、UTF-8 検査の
//! 分岐の被覆を理由に libFuzzer が非 UTF-8 の入力を corpus へ溜める）。

#![no_main]

use std::str;

use libfuzzer_sys::{Corpus, fuzz_target};

fuzz_target!(|data: &[u8]| -> Corpus {
  let Ok(source) = str::from_utf8(data) else {
    return Corpus::Reject;
  };
  assert!(source != "seiran-fuzz-self-test", "失敗経路の確認用");
  let _ = seiran_compiler::fuzzing::check_source(source);
  return Corpus::Keep;
});
