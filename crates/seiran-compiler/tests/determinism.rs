//! `compile` の決定性 property test（#306 / #376）。
//!
//! 同じ `MemoryProjectSource` から `compile` を複数回呼んでも、常に同じ `Publication` が
//! 得られることを検証する（時刻・乱数・HashMap 反復順序等の環境依存値が紛れ込んでいないこと）。
//!
//! 失敗する入力についても、報告される診断の **code 列（件数と順序）** が毎回一致することを
//! 検証する — 並列処理（rayon）の完了順や `HashMap` の反復順が表示順へ漏れていないこと（#376）。

mod common;

use std::path::Path;

use common::{minimal_config_toml, read_test_font};
use seiran_compiler::{MemoryProjectSource, ProjectPath};

/// メモリ上のテストプロジェクトで相対パスを解決する基準ディレクトリを返す。
fn project_base_dir() -> &'static Path { return Path::new("/project"); }

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
    let first = seiran_compiler::compile(&source, &root, project_base_dir()).expect("1 回目の compile は成功するはず");
    let second = seiran_compiler::compile(&source, &root, project_base_dir()).expect("2 回目の compile は成功するはず");

    // Assert — Publication は完全に同一（PartialEq 比較）
    assert_eq!(first.publication, second.publication, "text={text:?} で決定性が崩れているはず");
  }
}

/// エラー経路の入力（意味解析の 3 種混在・重複ラベルの複数件・複数画像の読込失敗）。
///
/// フォントの失敗は `MemoryProjectSource` では 19 種すべてが同じファイルを指すため種別ごとの差を
/// 作れず、`FontType::ALL` 順は `compiler::diagnostics` の golden が固定している。
const ERROR_CASES: &[&str] = &[
  "\\section[label=dup]{A}\n\n\\section[label=dup]{B}\n\n\\cite{missing-key} と \\ref{missing-label}\n",
  "\\section[label=dup]{A}\n\n\\section[label=dup]{B}\n\n\\section[label=dup]{C}\n",
  MISSING_IMAGES,
];

/// 未登録の画像を 2 枚参照する入力（画像は `ProjectPath` をキーにするので、
/// `MemoryProjectSource` でも 2 件の別々の失敗を作れる）。
const MISSING_IMAGES: &str = concat!(
  "\\begin{figure}\n\\image[width=80mm]{/project/z-missing.png}\n\\caption{1 枚目}\n\\end{figure}\n\n",
  "\\begin{figure}\n\\image[width=80mm]{/project/a-missing.png}\n\\caption{2 枚目}\n\\end{figure}\n",
);

/// 複数の画像が欠落しているとき、正規化済みパスの昇順で全件が報告される。
#[test]
fn missing_images_are_reported_in_path_order() {
  // Arrange
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
    .with_text("/project/text.sei", MISSING_IMAGES)
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let failure =
    seiran_compiler::compile(&source, &root, project_base_dir()).expect_err("2 枚とも欠落しているので失敗するはず");

  // Assert — 1 枚目で打ち切らず、文書順ではなくパス昇順（a → z）で並ぶ
  let messages: Vec<String> = failure.diagnostics().map(|diagnostic| return diagnostic.to_string()).collect();
  assert_eq!(messages.len(), 2, "欠落した 2 枚が両方報告されるはず: {messages:?}");
  assert!(messages[0].contains("/project/a-missing.png"), "パス昇順の 1 件目が先のはず: {messages:?}");
  assert!(messages[1].contains("/project/z-missing.png"), "パス昇順の 2 件目が後のはず: {messages:?}");
}

/// 同じ入力から `compile` を繰り返し呼んでも、報告される診断の code 列と件数が一致する。
///
/// 並列処理（rayon）の完了順や `HashMap` の反復順が表示順へ漏れていれば、繰り返しのどこかで
/// 順序が変わって落ちる（#376）。
#[test]
fn error_path_is_deterministic_across_repeated_runs() {
  for text in ERROR_CASES {
    // Arrange
    let font_bytes = read_test_font();
    let source = MemoryProjectSource::new()
      .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
      .with_text("/project/text.sei", *text)
      .with_bytes("/project/font.ttf", font_bytes);
    let root = ProjectPath::new("/project/config.toml");

    // Act — 同じ入力を繰り返しコンパイルし、診断の code 列を集める
    let runs: Vec<Vec<String>> = (0..32)
      .map(|_| {
        let failure = seiran_compiler::compile(&source, &root, project_base_dir()).expect_err("この入力は失敗するはず");
        return failure
          .diagnostics()
          .map(|diagnostic| {
            return diagnostic.code().expect("leaf 診断は code を持つはず").to_string();
          })
          .collect();
      })
      .collect();

    // Assert — 全実行で code 列（件数と順序）が一致する
    let first = runs.first().expect("32 回実行しているはず");
    assert!(!first.is_empty(), "失敗時は 1 件以上の診断があるはず");
    for (index, codes) in runs.iter().enumerate() {
      assert_eq!(codes, first, "{index} 回目で診断の順序・件数が変わった: text={text:?}");
    }
  }
}

/// `config.sources` の複数ファイルが欠落しているとき、宣言順で全件が決定的に報告される。
#[test]
fn missing_sources_are_reported_in_declaration_order_on_every_run() {
  // Arrange — パス名の辞書順とは逆に宣言する
  let font_bytes = read_test_font();
  let config = format!(
    "sources = [\"/project/z.sei\", \"/project/a.sei\"]\n\n{}{}{}",
    seiran_compiler::test_support::valid_pdf_section(),
    seiran_compiler::test_support::valid_output_section("out", "/project/out"),
    seiran_compiler::test_support::make_font_sections("/project/font.ttf"),
  );
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act / Assert
  for _ in 0..32 {
    let failure = seiran_compiler::compile(&source, &root, project_base_dir())
      .expect_err("2 ソースとも欠落しているので失敗するはず");
    let messages: Vec<String> = failure.diagnostics().map(|diagnostic| return diagnostic.to_string()).collect();
    assert_eq!(messages.len(), 2, "欠落した 2 件が両方報告されるはず");
    assert!(messages[0].contains("/project/z.sei"), "宣言順の 1 件目が先のはず: {messages:?}");
    assert!(messages[1].contains("/project/a.sei"), "宣言順の 2 件目が後のはず: {messages:?}");
  }
}
