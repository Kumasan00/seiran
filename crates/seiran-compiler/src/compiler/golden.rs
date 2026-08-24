//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! fixture を組版した決定的テキストを `tests/golden/` と比較する。再生成は
//! `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で行う。検証手段の選択（layout dump golden と
//! PDF バイト比較の使い分け）は `.claude/skills/verify-typesetting/SKILL.md` が規定し、
//! 本モジュール内部のテスト分類・ヘルパの使い分けはこの doc が正典。
//!
//! # テストの分類
//!
//! golden ファイル（`tests/golden/<name>.txt`）と実際に比較するのは主入口
//! [`layout_dumps_match_golden`]（[`GOLDEN_INPUTS`] 全 fixture の回帰）だけである。これは
//! [`dump_input_via_compile`] を介して `super::compile()`（lib target の公開 facade）→
//! `compiler::dump::dump_publication`（`publication::Publication` の決定的テキストダンプ）を通す。
//! ダンプは確定座標のテキスト表現であり krilla の描画は含まないが、`Publication` の
//! メタデータ・リンク・しおりまで含むため `dump_pages`（`typeset::dump` が所有）よりカバー範囲が広い。
//!
//! 残りのテストは golden ファイルを一切読み書きせず、次の 3 通りに分かれる。
//!
//! - **2 つのダンプをテスト内で直接比較**（[`dump_input`] → `build_pages` → `dump_pages`。
//!   `assert_eq!` / `assert_ne!` で golden ファイルは介さない）:
//!   [`index_marks_are_invisible_to_layout`]・style 差分 2 種
//!   [`layout_dump_changes_with_line_height`] / [`layout_dump_changes_with_punctuation_spacing`]
//! - **`build_pages` を直接呼び、返り値の `Page` / `PlacedBlock` へ直接アサート**（ダンプ関数は
//!   一切通らない）: [`keep_with_next_prevents_heading_orphan_end_to_end`]・
//!   脚注ページ単位採番 2 種 [`per_page_footnote_numbering_restarts_on_each_page`] /
//!   [`continuous_footnote_numbering_runs_through_pages`]（共通ヘルパ [`footnote_numbers_per_page`]
//!   経由）・[`long_footnote_splits_across_pages_without_overlapping_body`]
//! - **設定オーバーライドの 2 実装（型付き版と TOML 版）の収束だけを見る**（組版は一切通らない）:
//!   [`config_overrides_typed_and_toml_stay_in_sync`]
//!
//! golden 移行は `layout_dumps_match_golden` 1 本にとどまる。`Publication` / `dump_publication` は
//! `typeset::Page` レベルの anchor・索引語行の表現を持たないため、上記のダンプ比較テストは
//! 移行していない（[`dump_input_via_compile`] の doc が言う「順次移行」方針どおり）。
//!
//! # カバレッジの注意
//!
//! 前付け（タイトルページ / 目次）・running content（ヘッダ / フッタ）・段組みは既定 config では
//! 無効で、入力名ごとのオーバーライドヘルパが有効化する（例: `toc` / `title_page`）。
//!
//! - [`apply_input_style_overrides`]（型付き `Style`）: `dump_input` / `dump_input_via_compile` の両方が共有
//! - [`apply_input_config_overrides`]（型付き `ProjectConfig`）: `dump_input` に加え、`build_pages` を
//!   直接呼ぶ [`footnote_numbers_per_page`] と
//!   [`long_footnote_splits_across_pages_without_overlapping_body`] も使う（`dump_input` 専用ではない）
//! - [`apply_input_config_overrides_toml`]（`toml::Value` 版）: `dump_input_via_compile` 専用 —
//!   処理済みの `ProjectConfig` は `Serialize` を持たないため、`compile` に渡す前の生 TOML テーブルを
//!   直接書き換える
//!
//! 型付き版と TOML 版の同期規則は各ヘルパの doc を参照。これらの経路を触ったら、該当 fixture が
//! その機能を実際に通していることを確認する。
//!
//! # 新機能に golden テストを足す
//!
//! 1. `tests/text/<name>.sei` に機能を exercise する入力を追加する
//! 2. [`GOLDEN_INPUTS`] に名前を登録する。機能が既定で無効なら [`apply_input_style_overrides`] に
//!    有効化を追記する。config レベルの上書きが必要な場合は [`apply_input_config_overrides_toml`] にも
//!    追記する — `GOLDEN_INPUTS` の全 fixture は `layout_dumps_match_golden` 経由で
//!    `dump_input_via_compile`（`compile()` 経由）を通るため、型付き `ProjectConfig` を直接書き換える
//!    [`apply_input_config_overrides`] だけでは golden の入力へ反映されない。同じ fixture 名を
//!    golden 以外の個別テスト（脚注採番テスト等、`dump_input` を経由せず `build_pages` を直接呼ぶ）でも
//!    型付き版経由で使う場合は、挙動を揃えるため両方に追記する
//! 3. `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で golden を生成し、内容を確認してコミットする
//!
//! 外部ファイルに依存する入力は対象外（前例: `figure.sei` は画像実体にレイアウトが依存するため除外）。

use std::{
  fs,
  path::{Path, PathBuf},
  sync::Arc,
};

use crate::{
  compiler::build_pages,
  length::Length,
  project::{self, FilesystemProjectSource, FontData, MemoryProjectSource, ProjectPath, config::ProjectConfig},
  semantics::{References, read_references},
  style::{self, FootnoteNumbering, RunningTemplate, Style},
  typeset::{AnchorMark, Page, PlacedBlock, dump_pages},
};

/// golden 比較対象の入力名。
const GOLDEN_INPUTS: &[&str] = &[
  "align",
  "cases",
  "cite",
  "code",
  "color",
  "equation",
  "footnote",
  "footnote_per_page",
  "footnote_split",
  "gather",
  "hyperref",
  "hyphenation",
  "index",
  "itemize",
  "justify",
  "matrix",
  "multiline",
  "pagebreak",
  "quote",
  "ref",
  "split",
  "table",
  "text",
  "theorem",
  "title_page",
  "toc",
  "yakumono",
];

/// ワークスペースルートを返す。
pub(super) fn workspace_root() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート")
    .to_path_buf();
}

/// golden ファイルを置くディレクトリ（`crates/seiran-compiler/tests/golden`）を返す。
fn golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"); }

/// カレントディレクトリをワークスペースルートへ固定する（相対パス解決を実ビルドに合わせる）。
pub(super) fn enter_workspace_root() {
  std::env::set_current_dir(workspace_root()).expect("カレントディレクトリをワークスペースルートへ固定");
}

/// fixture の設定・スタイル・文献を読み込む。
///
/// テスト資産が未取得なら取得手順を案内して失敗する。
pub(super) fn load_base() -> (ProjectConfig, Style, Arc<References>) {
  assert!(
    Path::new("vendor/fonts").is_dir(),
    "golden テストの資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
  );
  let source = FilesystemProjectSource::new();
  let (config, _) =
    project::config::load(&source, Path::new("crates/seiran-compiler/tests/config/config.toml"), &workspace_root())
      .expect("fixture config.toml の読込");
  let style = style::load(&source, config.style_path.as_deref(), &workspace_root()).expect("fixture style.toml の読込");
  let references = read_references(&source, config.references_path.as_deref()).expect("fixture references の読込");
  return (config, style, Arc::new(references));
}

/// 検証対象の機能に必要な style 差分を入力ごとに適用する。
///
/// ページ余白は style が所有する（#389）ので版面を変える上書きもここに置く。config 側の上書き
/// （[`apply_input_config_overrides`] とその toml 版）は用紙寸法だけを扱う。
fn apply_input_style_overrides(name: &str, style: &mut Style) {
  match name {
    "title_page" => {
      style.title_page.enabled = true;
      style.header.left = RunningTemplate::parse("{title}");
      style.header.right = RunningTemplate::parse("{page} / {pages}");
      style.footer.center = RunningTemplate::parse("{page}");
    },
    "toc" => style.toc.enabled = true,
    "hyphenation" => {
      style.page.margin_left = Length::mm(275.0);
      style.page.margin_right = Length::mm(275.0);
    },
    // ページ単位採番が複数ページにまたがる版面にする（用紙寸法の縮小は config 側が持つ）
    "footnote_per_page" => {
      style.footnote.numbering = FootnoteNumbering::PerPage;
      style.page.margin_left = Length::mm(20.0);
      style.page.margin_right = Length::mm(20.0);
      style.page.margin_top = Length::mm(15.0);
      style.page.margin_bottom = Length::mm(15.0);
    },
    // 1 個の脚注が収まらず、繰越が連鎖する版面にする
    "footnote_split" => {
      style.page.margin_left = Length::mm(15.0);
      style.page.margin_right = Length::mm(15.0);
      style.page.margin_top = Length::mm(12.0);
      style.page.margin_bottom = Length::mm(12.0);
    },
    _ => {},
  }
}

/// 検証対象の機能に必要な config 差分を入力ごとに適用する。
///
/// **注意**: [`apply_input_config_overrides_toml`] と対になっている（`dump_input`＝本関数、
/// `dump_input_via_compile`＝toml 版という 2 つの golden 経路にそれぞれ渡すため）。ここへ
/// ケースを追加したら toml 版にも同じ上書きを追加すること——片方だけに追加すると
/// `layout_dumps_match_golden` が既定ジオメトリのまま golden を静かに再生成してしまい、
/// テスト失敗を経由せず座標がずれる。`config_overrides_typed_and_toml_stay_in_sync` が
/// この乖離を機械的に検査する。
fn apply_input_config_overrides(name: &str, config: &mut ProjectConfig) {
  if name == "hyphenation" {
    config.document.language = Some("en".to_string());
  }
  // ページ単位採番が複数ページにまたがる版面にする（余白側は style の上書きが担う）
  if name == "footnote_per_page" {
    config.pdf.width = Length::mm(150.0);
    config.pdf.height = Length::mm(130.0);
  }
  // 1 個の脚注が収まらず、繰越が連鎖する版面にする
  if name == "footnote_split" {
    config.pdf.width = Length::mm(120.0);
    config.pdf.height = Length::mm(85.0);
  }
}

/// `config.toml` に列挙されたフォントを実ファイルシステムから読み、`source` へ登録する。
///
/// `config.toml` のパスは実際の fixture ファイルと同じ相対パス（`vendor/fonts/...`）のままにし、
/// `compile` の `base_dir`（ワークスペースルート、`enter_workspace_root` が固定する）解決後の
/// 絶対パスと一致する位置へ実バイト列を登録する。パスを golden 用の仮想パスへ書き換えないのは、
/// `sources` / `style_path` 等の他フィールドとの対応関係を崩さないため（同じフォントファイルを
/// 複数 `font_type` が共有する場合の重複登録を避けるため `registered` で去重する）。
fn register_fonts(
  mut source: MemoryProjectSource,
  workspace_root: &Path,
  font_configs: &toml::value::Table,
) -> MemoryProjectSource {
  let mut registered: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
  for entry in font_configs.values() {
    let Some(table) = entry.as_table() else {
      continue;
    };
    let Some(real_path) = table.get("font_path").and_then(|v| return v.as_str()) else {
      continue;
    };
    let absolute_path = workspace_root.join(real_path);
    if registered.insert(absolute_path.clone()) {
      let bytes = fs::read(&absolute_path)
        .unwrap_or_else(|error| panic!("フォントを読めるはず: {}: {error}", absolute_path.display()));
      source = source.with_bytes(absolute_path, bytes);
    }
  }
  return source;
}

/// `apply_input_config_overrides`（型付き `ProjectConfig` 版）の `toml::Table` 版。
///
/// `ProjectConfig`（処理済み構造体）は `Serialize` を持たないため、`compile` に渡す前の生の TOML
/// テーブルを直接書き換える。上書き内容は型付き版と同じにする（golden の座標は同一のはず）。
///
/// **注意**: 型付き版 [`apply_input_config_overrides`] と対になっている。ここへケースを
/// 追加したら型付き版にも同じ上書きを追加すること——片方だけに追加すると
/// `layout_dumps_match_golden`（本関数を使う `dump_input_via_compile` 経由）が既定ジオメトリの
/// まま golden を静かに再生成してしまい、テスト失敗を経由せず座標がずれる。
/// `config_overrides_typed_and_toml_stay_in_sync` がこの乖離を機械的に検査する。
fn apply_input_config_overrides_toml(name: &str, table: &mut toml::value::Table) {
  fn set_pdf_field(table: &mut toml::value::Table, key: &str, value: &str) {
    let pdf = table.entry("pdf").or_insert_with(|| return toml::Value::Table(toml::value::Table::new()));
    pdf
      .as_table_mut()
      .expect("[pdf] はテーブルのはず")
      .insert(key.to_string(), toml::Value::String(value.to_string()));
  }
  if name == "hyphenation" {
    let document = table.entry("document").or_insert_with(|| return toml::Value::Table(toml::value::Table::new()));
    document
      .as_table_mut()
      .expect("[document] はテーブルのはず")
      .insert("language".to_string(), toml::Value::String("en".to_string()));
  }
  if name == "footnote_per_page" {
    set_pdf_field(table, "width", "150mm");
    set_pdf_field(table, "height", "130mm");
  }
  if name == "footnote_split" {
    set_pdf_field(table, "width", "120mm");
    set_pdf_field(table, "height", "85mm");
  }
}

/// 指定 fixture 用の `MemoryProjectSource` と `ProjectPath`（config.toml の入口）を組み立てる。
///
/// 実ファイルシステムからの読込は `fs::read` / `fs::read_to_string` のみ（fixture の config.toml /
/// style.toml / references.toml / `.sei` / フォント / CSL 資産）で、それ以外は一切実ディスクに
/// 触れない。`compile` はカレントディレクトリ（`enter_workspace_root` がワークスペースルートへ
/// 固定済み）を `base_dir` として相対パスを解決するため、`MemoryProjectSource` へは
/// **`base_dir` 解決後と同じ絶対パス**で登録する（golden 用の仮想パスは使わない。
/// `style.reference.csl_path` / `locale_path` は `load_base` の時点で既に絶対パスへ解決済みなので
/// そのまま同じ絶対パスに実バイト列を重ねればよい）。
fn memory_source_for_golden_fixture(name: &str) -> (MemoryProjectSource, ProjectPath) {
  let workspace_root = workspace_root();
  let config_path = workspace_root.join("crates/seiran-compiler/tests/config/config.toml");
  let references_path = workspace_root.join("crates/seiran-compiler/tests/config/references.toml");
  let style_path = workspace_root.join("crates/seiran-compiler/tests/config/style.toml");
  let sei_path = workspace_root.join(format!("tests/text/{name}.sei"));

  let base_config_text = fs::read_to_string(&config_path).expect("fixture config.toml を読めるはず");
  // `toml::Value::from_str` は単一の値式しかパースできず、複数行のドキュメントは
  // `toml::Table::from_str`（内部で `toml::from_str` を呼ぶ）でパースする必要がある。
  let mut table: toml::Table = base_config_text.parse().expect("fixture config.toml をパースできるはず");
  table.insert(
    "sources".to_string(),
    toml::Value::Array(vec![toml::Value::String(format!("tests/text/{name}.sei"))]),
  );
  apply_input_config_overrides_toml(name, &mut table);
  let config_text = toml::to_string(&table).expect("config.toml を再直列化できるはず");

  let font_configs = table
    .get("font_configs")
    .and_then(|v| return v.as_table())
    .expect("config.toml に [font_configs.*] があるはず")
    .clone();

  let (_, base_style, _) = load_base();
  let mut style = base_style;
  apply_input_style_overrides(name, &mut style);
  let style_text = toml::to_string(&style).expect("Style を TOML へ再直列化できるはず");

  let references_text = fs::read_to_string(&references_path).expect("fixture references.toml を読めるはず");
  let sei_text = fs::read_to_string(&sei_path).expect("fixture .sei を読めるはず");

  let mut source = MemoryProjectSource::new()
    .with_text(&config_path, config_text)
    .with_text(&style_path, style_text)
    .with_text(&references_path, references_text)
    .with_text(&sei_path, sei_text);

  source = register_fonts(source, &workspace_root, &font_configs);
  if let Some(csl_path) = &style.reference.csl_path {
    let bytes = fs::read(csl_path).unwrap_or_else(|error| panic!("CSL を読めるはず: {}: {error}", csl_path.display()));
    source = source.with_bytes(csl_path, bytes);
  }
  if let Some(locale_path) = &style.reference.locale_path {
    let bytes = fs::read(locale_path)
      .unwrap_or_else(|error| panic!("CSL ロケールを読めるはず: {}: {error}", locale_path.display()));
    source = source.with_bytes(locale_path, bytes);
  }

  return (source, ProjectPath::new(&config_path));
}

/// `compile()` を入口として fixture を組版し、`Publication` のダンプを返す（golden 移行版）。
///
/// `layout_dumps_match_golden` 専用。他の golden.rs テストは引き続き `dump_input`
/// （`build_pages` 経由）を使う——`crate::typeset::Page` レベルの anchor/index 行に依存する
/// `index_marks_are_invisible_to_layout` 等は、`Publication` には対応する表現が無いため
/// 今回は移行しない（issue の「順次移行」方針どおり）。
fn dump_input_via_compile(name: &str) -> String {
  let (source, root) = memory_source_for_golden_fixture(name);
  let compilation = super::compile(&source, &root, &workspace_root())
    .unwrap_or_else(|failure| panic!("fixture {name} の compile は成功するはず: {:?}", failure.into_report()));
  return super::dump::dump_publication(&compilation.publication);
}

/// 指定入力を組版し、確定ページ列のダンプを返す。
fn dump_input(base_config: &ProjectConfig, style: &Style, references: &Arc<References>, name: &str) -> String {
  let mut config = base_config.clone();
  config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
  let mut style = style.clone();
  apply_input_style_overrides(name, &mut style);
  apply_input_config_overrides(name, &mut config);
  let source = FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");
  let laid_out = build_pages(&config, &style, references, &font_data).expect("build_pages の実行");
  return dump_pages(&laid_out.pages);
}

#[test]
fn layout_dumps_match_golden() {
  // Arrange
  enter_workspace_root();
  let update = std::env::var_os("UPDATE_GOLDEN").is_some();
  if update {
    fs::create_dir_all(golden_dir()).expect("golden ディレクトリの作成");
  }

  // Act / Assert — 各入力を compile() 経由で組版し、Publication のダンプを golden と比較
  let mut mismatches = Vec::new();
  for name in GOLDEN_INPUTS {
    let dump = dump_input_via_compile(name);
    let golden_path = golden_dir().join(format!("{name}.txt"));
    if update {
      fs::write(&golden_path, &dump).expect("golden の書き出し");
    } else {
      let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!("golden が未生成です: {} ({error})。UPDATE_GOLDEN=1 で生成してください", golden_path.display())
      });
      if dump != expected {
        mismatches.push(*name);
      }
    }
  }

  assert!(
    mismatches.is_empty(),
    "レイアウトダンプが golden と一致しません: {mismatches:?}（意図した変更なら UPDATE_GOLDEN=1 で再生成し git diff で確認）"
  );
}

/// [`apply_input_config_overrides`]（型付き版）と [`apply_input_config_overrides_toml`]（toml 版）が
/// 両方とも扱う 3 fixture について、同じ座標へ収束することを確認する回帰テスト。
///
/// この 2 関数は golden の入口が 2 経路（`dump_input`＝`build_pages` 経由と
/// `dump_input_via_compile`＝`compile()` 経由）に分かれているために存在する並行実装で、
/// 将来どちらか片方だけにケースを追加すると `layout_dumps_match_golden` が既定ジオメトリの
/// まま golden を静かに再生成してしまう（テスト失敗を経由しない完全に静かな回帰）。
/// このテストは両関数が触れうるフィールド（`pdf.width` / `pdf.height` / `document.language`）
/// だけを比較する最小限のクロスチェックで、そのフィールドが片方だけ変わればここが失敗する。
///
/// ページ余白は style が所有するようになったので（#389）並行実装が無く、代わりに
/// 「型付き `Style` への上書き」と「TOML へ再直列化して読み直した `Style`」の
/// `page.margin_*` が一致すること（`Length` の serde 往復）を同じループで押さえる。
#[test]
fn config_overrides_typed_and_toml_stay_in_sync() {
  // Arrange
  enter_workspace_root();
  let (base_config, _, _) = load_base();
  let workspace_root = workspace_root();

  for name in ["hyphenation", "footnote_per_page", "footnote_split"] {
    // Act — 型付き版（`dump_input` と同じ適用順）
    let mut typed_config = base_config.clone();
    apply_input_config_overrides(name, &mut typed_config);

    // Act — toml 版（`dump_input_via_compile` と同じ経路で実際に `load` を通す）
    let (source, root) = memory_source_for_golden_fixture(name);
    let (toml_config, _) = project::config::load(&source, root.as_ref(), &workspace_root)
      .unwrap_or_else(|error| panic!("fixture {name} の toml 版 config 読込は成功するはず: {error}"));

    // Assert — 両関数が触れうるフィールドが一致する
    assert_eq!(
      typed_config.pdf.width, toml_config.pdf.width,
      "{name}: pdf.width が型付き版と toml 版で食い違っている"
    );
    assert_eq!(
      typed_config.pdf.height, toml_config.pdf.height,
      "{name}: pdf.height が型付き版と toml 版で食い違っている"
    );
    assert_eq!(
      typed_config.document.language, toml_config.document.language,
      "{name}: document.language が型付き版と toml 版で食い違っている"
    );

    // Assert — 余白は style が所有する。型付き上書きと TOML 往復後の値が一致する
    let (_, base_style, _) = load_base();
    let mut typed_style = base_style.clone();
    apply_input_style_overrides(name, &mut typed_style);
    let style_text = toml::to_string(&typed_style).expect("Style を TOML へ再直列化できるはず");
    let toml_style = style::parse(&style_text, "style.toml")
      .unwrap_or_else(|failures| panic!("fixture {name} の style 読み直しは成功するはず: {failures}"));
    assert_eq!(
      typed_style.page.margin_left, toml_style.page.margin_left,
      "{name}: page.margin_left が TOML 往復で変わっている"
    );
    assert_eq!(
      typed_style.page.margin_right, toml_style.page.margin_right,
      "{name}: page.margin_right が TOML 往復で変わっている"
    );
    assert_eq!(
      typed_style.page.margin_top, toml_style.page.margin_top,
      "{name}: page.margin_top が TOML 往復で変わっている"
    );
    assert_eq!(
      typed_style.page.margin_bottom, toml_style.page.margin_bottom,
      "{name}: page.margin_bottom が TOML 往復で変わっている"
    );
  }
}

/// 索引マーカーを除けば本文レイアウトが変わらないことを確認する。
#[test]
fn index_marks_are_invisible_to_layout() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();

  // Act
  let with_index = dump_input(&base_config, &style, &references, "index");
  let without_index = dump_input(&base_config, &style, &references, "index_baseline");
  let body_page_count = without_index.lines().filter(|line| return line.starts_with("=== page ")).count();
  let mut page_index: isize = -1;
  let stripped_lines: Vec<&str> = with_index
    .lines()
    .filter(|line| {
      if line.starts_with("=== page ") {
        page_index += 1;
      }
      return usize::try_from(page_index).is_ok_and(|index| return index < body_page_count)
        && !line.starts_with("index ")
        && !line.starts_with("anchor mark=Label(\"index-page:");
    })
    .collect();
  let stripped = stripped_lines.iter().fold(String::new(), |mut acc, line| {
    acc.push_str(line);
    acc.push('\n');
    return acc;
  });

  // Assert — 索引ページを除いた本文だけを比較する
  assert_eq!(stripped, without_index, "\\index の有無で本文のレイアウトが変わってはならない");
}

/// ページ末尾のブロックが見出し行かを返す。
fn page_ends_with_heading(page: &Page) -> bool {
  let Some(PlacedBlock::Line { line, baseline_y }) = page.blocks.last() else {
    return false;
  };
  let top = *baseline_y - line.height;
  return page.anchors.iter().any(|anchor| {
    return matches!(anchor.mark, AnchorMark::Heading { .. }) && (anchor.y - top).abs() < Length::pt(0.5);
  });
}

#[test]
fn keep_with_next_prevents_heading_orphan_end_to_end() {
  // Arrange — 版面を小さくして見出しがページ境界に当たりやすくする（見出し + 本文数行は空ページに
  // 収まる大きさ）。keepwithnext.sei は見出し直前を filler で埋め、見出しがページ末尾に来る配置。
  enter_workspace_root();
  let (mut config, mut style, references) = load_base();
  config.pdf.height = Length::mm(45.0);
  style.page.margin_top = Length::mm(10.0);
  style.page.margin_bottom = Length::mm(10.0);
  config.sources = vec![PathBuf::from("tests/text/keepwithnext.sei")];
  let source = FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");

  // Act
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");

  // Assert — 見出しがページ末尾に孤立せず、テストが複数ページを使っている
  assert!(laid_out.pages.len() >= 2, "複数ページに分かれるはず: {} ページ", laid_out.pages.len());
  for (index, page) in laid_out.pages.iter().enumerate() {
    assert!(!page_ends_with_heading(page), "page {index} が見出しで終わっている（孤立）: {:#?}", page.blocks);
  }
}

/// `footnote_per_page.sei` を指定の採番方式で組版し、ページごとの脚注番号列を返すテストヘルパ
fn footnote_numbers_per_page(numbering: FootnoteNumbering) -> Vec<Vec<u32>> {
  enter_workspace_root();
  let (mut config, mut style, references) = load_base();
  config.sources = vec![PathBuf::from("tests/text/footnote_per_page.sei")];
  apply_input_config_overrides("footnote_per_page", &mut config);
  apply_input_style_overrides("footnote_per_page", &mut style);
  // 採番方式はこのヘルパの引数が決める（style 上書きの既定値を上書きする）
  style.footnote.numbering = numbering;
  let source = FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");
  return laid_out
    .pages
    .iter()
    .map(|page| return page.footnotes.iter().map(|footnote| return footnote.number).collect())
    .collect();
}

#[test]
fn per_page_footnote_numbering_restarts_on_each_page() {
  // Act
  let per_page = footnote_numbers_per_page(FootnoteNumbering::PerPage);

  // Assert — 脚注を持つページが 2 つ以上あり（空振りでないこと）、どのページも 1 から始まる連番。
  // 入力は 1 ページ目に 10 個置くので、2 ページ目は通し番号なら 11 以降＝マーカーが 2 桁になる。
  // ページ単位採番では 1 桁に縮み、その幅の変化が行分割へ跳ね返る循環を踏んだうえで収束している。
  let pages_with_footnotes: Vec<&Vec<u32>> = per_page.iter().filter(|numbers| return !numbers.is_empty()).collect();
  assert!(pages_with_footnotes.len() >= 2, "脚注が 2 ページ以上に分かれるはず: {per_page:?}");
  for numbers in pages_with_footnotes {
    let expected: Vec<u32> = (1..=u32::try_from(numbers.len()).expect("脚注数は u32 に収まる")).collect();
    assert_eq!(*numbers, expected, "各ページの脚注番号は 1 からの連番のはず: {per_page:?}");
  }
}

#[test]
fn long_footnote_splits_across_pages_without_overlapping_body() {
  // Arrange
  enter_workspace_root();
  let (mut config, mut style, references) = load_base();
  config.sources = vec![PathBuf::from("tests/text/footnote_split.sei")];
  apply_input_config_overrides("footnote_split", &mut config);
  apply_input_style_overrides("footnote_split", &mut style);
  let source = FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");

  // Act
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");

  // Assert — 脚注 1 の続きが次ページへ繰り越される
  let fragments: Vec<Vec<(u32, bool)>> = laid_out
    .pages
    .iter()
    .map(|page| return page.footnotes.iter().map(|f| return (f.number, f.continued)).collect())
    .collect();
  let carried = fragments
    .iter()
    .position(|page| return page.iter().any(|(_, continued)| return *continued))
    .unwrap_or_else(|| panic!("脚注が分割されて繰越が生じるはず（空振り検知）: {fragments:?}"));
  assert!(carried > 0, "繰越は 2 ページ目以降に現れるはず: {fragments:?}");
  // 繰越はそのページの脚注領域の先頭（自前の脚注より前）に置かれる
  assert_eq!(fragments[carried].first(), Some(&(1, true)), "繰越が脚注領域の先頭のはず: {fragments:?}");
  assert!(
    fragments[carried].iter().any(|(number, continued)| return *number == 2 && !continued),
    "繰越先ページの自前の脚注が繰越の後ろに積まれるはず: {fragments:?}"
  );
  // 本文と脚注が重ならない（繰越ページも含めて）
  for (index, page) in laid_out.pages.iter().enumerate() {
    let Some(body_bottom) = page.blocks.iter().filter_map(block_bottom).reduce(Length::max) else {
      continue;
    };
    let Some(footnote_top) =
      page.footnotes.iter().flat_map(|f| return &f.blocks).filter_map(block_top).reduce(Length::min)
    else {
      continue;
    };
    assert!(
      footnote_top >= body_bottom,
      "page {index}: 本文の下端 {} と脚注の上端 {} が重なっている",
      body_bottom.to_pt(),
      footnote_top.to_pt()
    );
  }
}

/// 配置済みブロックの上端（脚注領域の重なり判定に使う。行・罫線のみを見る）
fn block_top(block: &PlacedBlock) -> Option<Length> {
  return match block {
    PlacedBlock::Line { line, baseline_y } => Some(*baseline_y - line.height),
    PlacedBlock::Rule { y, .. } => Some(*y),
    _ => None,
  };
}

/// 配置済みブロックの下端（本文の重なり判定に使う。行のみを見る）
fn block_bottom(block: &PlacedBlock) -> Option<Length> {
  return match block {
    PlacedBlock::Line { line, baseline_y } => Some(*baseline_y + line.depth),
    _ => None,
  };
}

#[test]
fn continuous_footnote_numbering_runs_through_pages() {
  // Act — 同じ入力を既定（通し）で組む
  let continuous = footnote_numbers_per_page(FootnoteNumbering::Continuous);

  // Assert — ページをまたいでも 1 からの通し連番のまま（ページ単位採番の導入で既定が変わっていない）
  let flattened: Vec<u32> = continuous.iter().flatten().copied().collect();
  let expected: Vec<u32> = (1..=u32::try_from(flattened.len()).expect("脚注数は u32 に収まる")).collect();
  assert_eq!(flattened, expected, "通し採番はページをまたいで連番のはず: {continuous:?}");
}

#[test]
fn layout_dump_changes_with_line_height() {
  // Arrange — 行送り（line_height_factor）だけを変えた 2 スタイル。行送りは 2 行目以降の
  // ベースライン送りに効くため、複数行が縦に並ぶ入力（itemize）を対象にする。
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut taller = style.clone();
  taller.text.line_height_factor += 0.5;

  // Act
  let base_dump = dump_input(&base_config, &style, &references, "itemize");
  let taller_dump = dump_input(&base_config, &taller, &references, "itemize");

  // Assert — レイアウトに影響する定数変更はダンプの差分として現れる
  assert_ne!(base_dump, taller_dump);
}

#[test]
fn layout_dump_changes_with_punctuation_spacing() {
  // Arrange — 和文約物アキ調整（JIS X 4051）の on/off だけを変えた 2 スタイル。
  // 約物が密な入力（yakumono）で連続約物の詰め・約物の収縮点化が座標差として現れる。
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut disabled = style.clone();
  disabled.text.punctuation_spacing = false;

  // Act — 既定（有効）と無効（フォントの送り幅そのまま）を組版してダンプする
  let enabled_dump = dump_input(&base_config, &style, &references, "yakumono");
  let disabled_dump = dump_input(&base_config, &disabled, &references, "yakumono");

  // Assert — 約物アキ調整はレイアウトを変える（無効化で従来出力へ戻せる）
  assert_ne!(enabled_dump, disabled_dump);
}

/// コードブロックの空行が 1 行ぶんの高さを保つことを確認する。
///
/// 空行は内容が空の Atom 1 つになるので、素朴に組むと行の高さ・深さが 0 になり、行送りが
/// `leading.max(前の行の深さ + この行の高さ)` の leading まで縮む（他の行より詰まる）。
/// `typeset::block` の strut がこれを防いでいる。
#[test]
fn blank_code_line_keeps_a_full_line_height() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();

  // Act — code.sei は空行を含むコードブロックを 2 つ持つ
  let dump = dump_input(&base_config, &style, &references, "code");

  // Assert
  assert!(!dump.contains("height=0.00"), "高さ 0 の行は出ないはず（空行も 1 行ぶんの extent を持つ）");
}
