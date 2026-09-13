//! 入力読込の唯一の入口 [`load`] と、その成果物 [`CompilationInputs`]
//!
//! config.toml → style.toml → 横断検証 → {文献・フォント・ソース} という順序は、前段の結果が次段の
//! 入力になることから決まる（style / references のパスは config.toml が持ち、横断検証は config × style の
//! 両方を要求する）。横断検証は検証済み版面 `PreparedGeometry` の構築でもあり、その値は
//! `CompilationInputs` が保持して組版へ渡す（#533）。横断検証まで通った後の文献・フォント・ソースは
//! 検証済み config だけを入力にして互いの成果を必要としないので、実行順（文献 → フォント → ソース）の
//! まま全部試し、失敗をこの順に集約する（#552）。この順序とエラー集約を知るのはこの module だけで、
//! 呼び出し元（`compile`）は [`load`] を 1 回呼ぶだけになる（#351）。`config_path` は facade が解決済み。
//! `resolver` も facade が 1 回構築したものを受け取る。
//!
//! CSL スタイル・ロケールはここでは読まない — 引用箇所が 1 つも無ければ `.csl` を読まない
//! という遅延は `semantics::analyze` の内側に閉じている。

use std::{sync::Arc, time::Instant};

use tracing::debug;

mod error;

use error::CompileError;

use crate::{
  failures::Failures,
  project,
  project::{
    FontData, PathResolver, ProjectPath, ProjectSource, SourceSet,
    config::{ConfigWarning, ProjectConfig},
  },
  semantics::{References, read_references},
  style,
  style::Style,
  typeset::PreparedGeometry,
};

/// 読込・個別検証・横断検証をすべて通った入力。
///
/// フィールドは非公開で、構築経路は [`load`] だけ（テスト専用のコンストラクタも持たない）。
/// 「検証を通っていない値が後段へ流れない」ことを型で保証するため、外から組み立てられる
/// コンストラクタを持たない（受け入れ条件、#351 / #522）。画像はパース後にパスが分かるため含めない。
pub(super) struct CompilationInputs {
  /// 検証済みの設定（用紙・余白・`sources`・`font_configs` 等）
  config: ProjectConfig,
  /// 検証済みのスタイル
  style: Style,
  /// config × style の横断検証を通った版面（本文・前付け・後付けの寸法）。
  /// 組版はこの確定値を受け取り、幅・ページ幾何を再計算しない（#533）
  geometry: PreparedGeometry,
  /// `\cite` の CSL 整形に使う文献データ。`semantics::analyze` へ共有参照として渡すので
  /// `Arc` で持つ
  references: Arc<References>,
  /// 読込済みの全フォントバイナリ
  font_data: FontData,
  /// ソースファイルごとの読込済みテキスト（`SourceId` で引ける）
  sources: SourceSet,
}

impl CompilationInputs {
  /// 検証済みの設定を返す。
  pub(super) fn config(&self) -> &ProjectConfig { return &self.config; }

  /// 検証済みのスタイルを返す。
  pub(super) fn style(&self) -> &Style { return &self.style; }

  /// 検証済みの版面を返す。
  pub(super) fn geometry(&self) -> &PreparedGeometry { return &self.geometry; }

  /// 文献データを返す。
  pub(super) fn references(&self) -> &Arc<References> { return &self.references; }

  /// 読込済みフォントバイナリを返す。
  pub(super) fn font_data(&self) -> &FontData { return &self.font_data; }

  /// 読込済みソース集合を返す。
  pub(super) fn sources(&self) -> &SourceSet { return &self.sources; }
}

/// 設定・スタイル・文献・フォント・ソースを読み込み、検証済みの入力を組み立てる。
///
/// `source` は呼び出し元が 1 回だけ構築したものを受け取り、ここでは構築しない。
///
/// 戻り値は読込の成否と、config.toml の読込で確定した警告（`sources` の宣言順）の組。警告は後段
/// （style・横断検証・文献・フォント・ソース）が失敗しても、config 自身の検証が失敗しても返す（#550）。
///
/// # Errors
///
/// 設定・スタイルの読込または検証、両者の横断検証、文献・フォント・ソースの読込のいずれかに
/// 失敗した場合に、組の第 1 要素がエラーになる。
///
/// **後段の入力を構築できない境界だけ早期 return する** — config が読めなければ style path が決まらず、
/// style が無ければ横断検証ができないので、config → style → 横断検証の間は跨いで集約しない（#376）。
/// 横断検証まで通った後の文献・フォント・ソースの読込は互いに独立なので、1 件目で打ち切らず
/// 文献 → フォント → ソースの順に全件を集約する（#552）。種類の中で独立に検査できるもの
/// （複数フォントパス・複数ソース）も従来どおり全件を集約する。
pub(super) fn load(
  source: &dyn ProjectSource,
  config_path: &ProjectPath,
  resolver: &PathResolver,
) -> (Result<CompilationInputs, Failures<CompileError>>, Vec<ConfigWarning>) {
  let (config, config_warnings) = project::config::load(source, config_path, resolver);
  let inputs = config.map_err(lift).and_then(|config| return load_after_config(source, config, resolver));
  return (inputs, config_warnings);
}

/// 検証済みの設定から、残りの入力（スタイル・版面・文献・フォント・ソース）を読み込む。
///
/// # Errors
///
/// スタイルの読込・検証、横断検証、文献・フォント・ソースの読込のいずれかに失敗した場合にエラーを返す。
fn load_after_config(
  source: &dyn ProjectSource,
  config: ProjectConfig,
  resolver: &PathResolver,
) -> Result<CompilationInputs, Failures<CompileError>> {
  let style = style::load(source, config.style_path.as_ref(), resolver).map_err(lift)?;
  let geometry = PreparedGeometry::prepare(&config, &style).map_err(lift)?;
  let (references, font_data, sources) = read_independent_inputs(source, &config)?;

  return Ok(CompilationInputs {
    config,
    style,
    geometry,
    references,
    font_data,
    sources,
  });
}

/// 検証済みの設定だけを入力にする 3 つの読込（文献・フォント・ソース）を実行する。
///
/// 3 つは互いの成果を必要としないので、1 件目で打ち切らず実行順（文献 → フォント → ソース）のまま
/// 全部試し、失敗をこの順に 1 つの集合へ連結する（#552）。種類の中の順序は各読込が決める
/// （フォントはパスの昇順、ソースは `sources` の宣言順）。
///
/// # Errors
///
/// 3 つのうち 1 つでも失敗すれば、失敗したもの全部の診断をこの順に返す。
fn read_independent_inputs(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
) -> Result<(Arc<References>, FontData, SourceSet), Failures<CompileError>> {
  let references = read_references(source, config.references_path.as_ref()).map(Arc::new).map_err(single);

  let stage_start = Instant::now();
  let font_data = FontData::load(source, &config.font_configs).map_err(lift);
  if font_data.is_ok() {
    debug!(elapsed = ?stage_start.elapsed(), "フォントファイルを読込");
  }

  let sources = read_sources(source, &config.sources);

  return match (references, font_data, sources) {
    (Ok(references), Ok(font_data), Ok(sources)) => Ok((references, font_data, sources)),
    (references, font_data, sources) => {
      let errors: Vec<CompileError> =
        [references.err(), font_data.err(), sources.err()].into_iter().flatten().flatten().collect();
      let Some(failures) = Failures::from_vec(errors) else {
        unreachable!("この arm は 3 つの読込のうち少なくとも 1 つが Err のときにだけ入る")
      };
      Err(failures)
    },
  };
}

/// 段まるごとの失敗（後続の入力を構築できないもの）を 1 件の非空集合へ包む。
fn single<E: Into<CompileError>>(error: E) -> Failures<CompileError> { return Failures::single(error.into()); }

/// 段が集めた非空集合を、そのまま `CompileError` の非空集合へ持ち上げる。
fn lift<E: Into<CompileError>>(failures: Failures<E>) -> Failures<CompileError> { return failures.map(Into::into); }

/// `config.sources` を読み込み、失敗を位置付き診断へ組み替える。
///
/// `project::SourceSet` はどのパスがどう失敗したかだけを返し、役割（テキストファイル）と
/// パスを含む leaf diagnostic を組み立てるのはここ。seam の `SourceReadError` は
/// `Diagnostic` を実装しない低水準 cause なので、そのまま `#[source]` に載せても
/// 入れ子の診断ブロックにはならない（#377）。
fn read_sources(source: &dyn ProjectSource, sources: &[ProjectPath]) -> Result<SourceSet, Failures<CompileError>> {
  return SourceSet::read(source, sources).map_err(|failures| {
    return failures.map(|error| {
      return CompileError::ReadTextFile {
        path: error.path,
        source: error.source,
      };
    });
  });
}

#[cfg(test)]
mod tests {
  use std::{path::Path, sync::Arc};

  use miette::Diagnostic;

  use super::{CompileError, load, read_sources};
  use crate::project::{
    MemoryProjectSource, PathResolver, ProjectPath, ProjectSource, SourceReadError,
    config::test_support::{make_font_sections, valid_output_section, valid_pdf_section},
  };

  /// `project::SourceSet` の素のエラーを、移設前と同じ位置付き診断へ組み替えることを固定する。
  ///
  /// `code` と役割・パスを含むメッセージの組み立ては `project` ではなくここの責務なので、
  /// `SourceSet::read` 側のテストではこの層を通らない（#351）。
  #[test]
  fn read_sources_maps_missing_file_to_read_text_file_diagnostic() {
    // Arrange — 存在するソースと存在しないソースを混ぜる
    let source = MemoryProjectSource::new().with_text("/project/a.sei", "content-a");
    let sources = vec![
      ProjectPath::new("/project/a.sei"),
      ProjectPath::new("/project/missing.sei"),
    ];

    // Act
    let result = read_sources(&source, &sources);

    // Assert
    let Err(failures) = result else {
      panic!("ReadTextFile を期待");
    };
    let CompileError::ReadTextFile {
      path,
      source: read_error,
    } = failures.first()
    else {
      panic!("ReadTextFile を期待");
    };
    assert_eq!(path, "/project/missing.sei");
    assert!(
      matches!(read_error, SourceReadError::NotFound),
      "seam のエラーは cause として保たれるはず: {read_error:?}"
    );
  }

  #[test]
  fn read_sources_reports_every_missing_file_in_declaration_order() {
    // Arrange — 2 つの欠落を宣言順とは逆のパス名で並べる（宣言順で報告されることを見る）
    let source = MemoryProjectSource::new();
    let sources = vec![
      ProjectPath::new("/project/z-missing.sei"),
      ProjectPath::new("/project/a-missing.sei"),
    ];

    // Act
    let Err(failures) = read_sources(&source, &sources) else {
      panic!("2 件とも失敗するはず");
    };

    // Assert — パス名の辞書順ではなく config.sources の宣言順
    let paths: Vec<&str> = failures
      .iter()
      .map(|error| {
        let CompileError::ReadTextFile { path, .. } = error else {
          panic!("ReadTextFile を期待");
        };
        return path.as_str();
      })
      .collect();
    assert_eq!(paths, vec!["/project/z-missing.sei", "/project/a-missing.sei"]);
  }

  /// 登録済みのパスのうち `unreadable` に挙げたものだけ「存在はするが読めない」`ProjectSource`。
  ///
  /// config の検証（`source.exists`）は通り、読込段で初めて失敗する入力を作るためのテスト用 double。
  /// `MemoryProjectSource` は登録済みのパスを必ず読めるので、フォントの読込失敗はこれでしか起こせない。
  struct UnreadablePaths {
    /// 実体
    inner: MemoryProjectSource,
    /// 読込だけを失敗させるパス
    unreadable: Vec<ProjectPath>,
  }

  impl UnreadablePaths {
    /// `path` が読めないパスなら権限エラーを返す。
    fn check(&self, path: &ProjectPath) -> Result<(), SourceReadError> {
      if self.unreadable.contains(path) {
        return Err(SourceReadError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
      }
      return Ok(());
    }
  }

  impl ProjectSource for UnreadablePaths {
    fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError> {
      self.check(path)?;
      return self.inner.read_text(path);
    }

    fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> {
      self.check(path)?;
      return self.inner.read_bytes(path);
    }

    fn exists(&self, path: &ProjectPath) -> bool { return self.inner.exists(path); }
  }

  /// 文献・フォント 2 ファイル（serif だけ `fonts/broken.ttf`）・ソース 2 本を宣言する `config.toml`。
  ///
  /// `style_path` を渡すと `style_path = "..."` を足す。
  fn config_toml(style_path: Option<&str>) -> String {
    let fonts = make_font_sections("fonts/ok.ttf").replacen(
      "font_path = \"fonts/ok.ttf\"",
      "font_path = \"fonts/broken.ttf\"",
      1,
    );
    let style = style_path.map_or_else(String::new, |path| return format!("style_path = \"{path}\"\n"));
    return format!(
      "sources = [\"a.sei\", \"broken.sei\"]\nreferences_path = \"refs.toml\"\n{style}\n{}{}{fonts}",
      valid_output_section("test", "out"),
      valid_pdf_section(),
    );
  }

  /// `config` とそれが参照するファイルをすべて `/project` 配下に登録した `MemoryProjectSource`。
  fn registered_project(config: &str) -> MemoryProjectSource {
    return MemoryProjectSource::new()
      .with_text("/project/config.toml", config)
      .with_text("/project/refs.toml", "")
      .with_bytes("/project/fonts/ok.ttf", Vec::new())
      .with_bytes("/project/fonts/broken.ttf", Vec::new())
      .with_text("/project/a.sei", "本文")
      .with_text("/project/broken.sei", "本文");
  }

  #[test]
  fn load_reports_references_font_and_source_read_failures_together_in_input_order() {
    // Arrange — 3 種とも config の検証（存在確認）は通り、読込段で初めて失敗する
    let source = UnreadablePaths {
      inner: registered_project(&config_toml(None)),
      unreadable: vec![
        ProjectPath::new("/project/broken.sei"),
        ProjectPath::new("/project/fonts/broken.ttf"),
        ProjectPath::new("/project/refs.toml"),
      ],
    };

    // Act
    let (result, _) =
      load(&source, &ProjectPath::new("/project/config.toml"), &PathResolver::new(Path::new("/project")));

    // Assert — 1 件目で打ち切らず、文献 → フォント → ソースの順に 1 度で並ぶ
    let Err(failures) = result else {
      panic!("3 種の読込失敗を期待");
    };
    let codes: Vec<String> = failures
      .iter()
      .map(|error| return error.code().expect("leaf の code を持つはず").to_string())
      .collect();
    assert_eq!(
      codes,
      vec![
        "semantics::citation::references::read_file".to_string(),
        "project::font::read".to_string(),
        "compiler::read_text_file".to_string(),
      ]
    );
  }

  #[test]
  fn load_does_not_read_references_fonts_or_sources_when_the_style_fails() {
    // Arrange — style.toml だけが TOML として壊れている
    let source = registered_project(&config_toml(Some("style.toml"))).with_text("/project/style.toml", "x = \n");

    // Act
    let (result, _) =
      load(&source, &ProjectPath::new("/project/config.toml"), &PathResolver::new(Path::new("/project")));

    // Assert — style で早期 return し、後段の 3 種は 1 度も読まれない
    let Err(failures) = result else {
      panic!("style の解析失敗を期待");
    };
    assert_eq!(failures.first().code().expect("leaf の code を持つはず").to_string(), "style::parse_toml");
    for path in [
      "/project/refs.toml",
      "/project/fonts/ok.ttf",
      "/project/fonts/broken.ttf",
      "/project/a.sei",
      "/project/broken.sei",
    ] {
      assert_eq!(source.read_count(path), 0, "{path} は読まれないはず");
    }
  }
}
