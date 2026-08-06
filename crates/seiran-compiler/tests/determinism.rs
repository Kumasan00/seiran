//! `compile` の決定性 property test（#306）。
//!
//! 同じ `MemoryProjectSource` から `compile` を複数回呼んでも、常に同じ `Publication` が
//! 得られることを検証する（時刻・乱数・HashMap 反復順序等の環境依存値が紛れ込んでいないこと）。

mod common;

use common::{minimal_config_toml, read_test_font};
use seiran_compiler::{MemoryProjectSource, ProjectPath};

/// 代表的な入力の一覧（filesystem を使わず埋め込む。網羅目的の fixture 追加ではなく、
/// テキスト・装飾・見出し+ラベル+相互参照という異なるコード経路を通すための最小集合）。
///
/// 集合が小さく固定なので、ランダムサンプリングではなく全件を漏れなく走査する
/// （`prop::sample::select` は `cases` 回のうち一部しか各要素を引かない可能性があり、
/// 3 要素すべてを確実に検証するには全走査のほうが単純かつ強い）。
const REPRESENTATIVE_SOURCES: &[&str] = &[
  "Hello, Seiran!",
  r"\bold{強調されたテキスト}",
  r"\section[label=sec:intro]{見出し}本文。\ref{sec:intro}",
];

/// 同じ入力から `compile` を 2 回呼んでも同一の `Publication` が得られる。
/// 代表的な入力すべてに対して検証する。
#[test]
fn compile_is_deterministic_for_the_same_source() {
  for text in REPRESENTATIVE_SOURCES {
    // Arrange
    let font_bytes = read_test_font();
    let source = MemoryProjectSource::new()
      .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
      .with_text("/project/text.sei", *text)
      .with_bytes("/project/font.ttf", font_bytes);
    let root = ProjectPath::new("/project/config.toml");

    // Act — 同じ source から 2 回 compile する
    let first = seiran_compiler::compile(&source, &root).expect("1 回目の compile は成功するはず");
    let second = seiran_compiler::compile(&source, &root).expect("2 回目の compile は成功するはず");

    // Assert — Publication は完全に同一（PartialEq 比較）
    assert_eq!(first.publication, second.publication, "text={text:?} で決定性が崩れているはず");
  }
}
