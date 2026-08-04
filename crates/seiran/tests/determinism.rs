//! `compile` の決定性 property test（#306）。
//!
//! 同じ `MemoryProjectSource` から `compile` を複数回呼んでも、常に同じ `Publication` が
//! 得られることを検証する（時刻・乱数・HashMap 反復順序等の環境依存値が紛れ込んでいないこと）。

mod common;

use common::{minimal_config_toml, read_test_font};
use config::{MemoryProjectSource, ProjectPath};
use proptest::prelude::*;

/// 代表的な入力の一覧（filesystem を使わず埋め込む。網羅目的の fixture 追加ではなく、
/// テキスト・装飾・見出し+ラベル+相互参照という異なるコード経路を通すための最小集合）。
const REPRESENTATIVE_SOURCES: &[&str] = &[
  "Hello, Seiran!",
  r"\bold{強調されたテキスト}",
  r"\section[label=sec:intro]{見出し}本文。\ref{sec:intro}",
];

proptest! {
  #![proptest_config(ProptestConfig { cases: 8, ..ProptestConfig::default() })]

  /// 同じ入力から `compile` を 2 回呼んでも同一の `Publication` が得られる。
  #[test]
  fn compile_is_deterministic_for_the_same_source(
    text in prop::sample::select(REPRESENTATIVE_SOURCES),
  ) {
    // Arrange
    let font_bytes = read_test_font();
    let source = MemoryProjectSource::new()
      .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
      .with_text("/project/text.sei", text)
      .with_bytes("/project/font.ttf", font_bytes);
    let root = ProjectPath::new("/project/config.toml");

    // Act — 同じ source から 2 回 compile する
    let first = seiran::compile(&source, &root).expect("1 回目の compile は成功するはず");
    let second = seiran::compile(&source, &root).expect("2 回目の compile は成功するはず");

    // Assert — Publication は完全に同一（PartialEq 比較）
    prop_assert_eq!(first.publication, second.publication);
  }
}
