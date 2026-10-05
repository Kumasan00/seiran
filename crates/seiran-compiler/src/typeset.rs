//! 組版 module — 意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の [`Publication`]
//! へ変換する。
//!
//! 段順序はフォント資源の構築（解析 → メトリクス → 検証 → シェーパー）→ 画像パス収集 → 画像読込 →
//! lowering → 計測（画像の描画寸法もここで確定）→ 行分割・改ページ → 前付け・後付け → ページラベル →
//! 走り文 → outline → 確定座標の描画命令への変換。
//!
//! 組版中間型（`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系）は子 module `boxes` が
//! 所有する。
//!
//! フォント処理（OpenType 解析・検証・メトリクス・シェイピング）は子 module `font` が持つ。

mod boxes;
mod boxing;
mod breaking;
#[cfg(test)]
mod dump;
mod emit;
mod error;
mod font;
mod geometry;
mod image;
mod lowering;
mod observe;
mod pagination;
#[cfg(test)]
mod test_support;
mod warning;

use std::mem;

#[cfg(test)]
pub(crate) use boxes::{AnchorId, HBoxContent, LinkTarget, Page, PlacedBlock};
#[cfg(test)]
pub(crate) use dump::dump_pages;
pub(crate) use error::TypesetError;
use font::{FontSystem, FontWarning};
pub(crate) use geometry::{GeometryValidationError, PreparedGeometry};
// `#[cfg(test)]` を付けない — 本体コード（`compose` / `lay_out`）もこの名前を使い、条件付きの
// 再エクスポートと本体用の `use` を並べるとテストビルドで E0252（同名の重複定義）になる。
pub(crate) use pagination::LaidOutDocument;
#[cfg(test)]
pub(crate) use test_support::layout_for_test;
use tracing::{info, info_span};
pub(crate) use warning::TypesetWarning;

use crate::{
  failures::Failures,
  phase::Phase,
  project::{FontData, ProjectPath, ProjectSource, config::ProjectConfig},
  publication::Publication,
  semantics::SemanticDocument,
  style::Style,
};

/// [`compose`] の成果物 — 描画直前の出版物と、それに付随する情報。
#[derive(Debug)]
pub(crate) struct TypesetOutput {
  /// 座標と描画順が確定した文書
  pub(crate) publication: Publication,
  /// 文書が参照した画像ファイルのパス一覧（重複なし・昇順）
  pub(crate) image_paths: Vec<ProjectPath>,
}

/// 意味解析の成果物を、描画直前の [`Publication`] へ組版する。
///
/// 画像は `document` が参照しているぶんだけを `source` 経由で読み込む（読込は 1 回だけ）。
///
/// 組版を止めないがユーザーが直せる問題（フォント設定の警告・脚注のはみ出し）は [`TypesetWarning`] として
/// 組の第 2 要素で、フォント → 本体の順に返す。フォントの警告は配置が失敗しても返し、配置由来の警告は
/// 配置が成功したときだけ返す。
///
/// `geometry` は、ここで渡す `config` / `style` と同じ組から `PreparedGeometry::prepare` した値でなければ
/// ならない（型ではこの一致を強制していない）。
///
/// # Errors
///
/// フォントの解析・メトリクス取得・設定検証・シェーパー構築、画像の読込・デコード・寸法確定、
/// または脚注のページ単位採番の収束に失敗した場合に、組の第 1 要素が、その段で見つかった失敗を
/// 非空集合で持つ（フォント・画像はそれぞれ独立に検査できるので段の中では全件、段の間は早期 return する）。
pub(crate) fn compose(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  geometry: &PreparedGeometry,
  font_data: &FontData,
  document: &SemanticDocument,
) -> (Result<TypesetOutput, Failures<TypesetError>>, Vec<TypesetWarning>) {
  let (font_resources, font_warnings) = load_fonts(config, font_data);
  let mut warnings: Vec<TypesetWarning> = font_warnings.into_iter().map(TypesetWarning::Font).collect();
  let font_resources = match font_resources {
    Ok(font_resources) => font_resources,
    Err(failures) => return (Err(failures), warnings),
  };

  let phase = Phase::enter(info_span!("typeset"));
  let (mut laid_out, layout_warnings) = match lay_out(source, config, style, geometry, &font_resources, document) {
    Ok(laid_out) => laid_out,
    Err(failures) => return (Err(failures), warnings),
  };
  let image_paths = mem::take(&mut laid_out.image_paths);
  let publication = emit::emit(config, font_data, &font_resources, laid_out);
  info!(page_count = publication.pages().len(), warning_count = layout_warnings.len(), "文書を組版");
  phase.succeed();

  warnings.extend(layout_warnings);
  return (
    Ok(TypesetOutput {
      publication,
      image_paths,
    }),
    warnings,
  );
}

/// フォント資源を構築する（`font` phase）。
///
/// 検証で確定した警告は、構築が失敗しても組の第 2 要素で返す。
///
/// # Errors
///
/// フォント解析・メトリクス取得・設定検証・シェーパー初期化のいずれかに失敗した場合に、組の第 1 要素が、その段で見つかった
/// 違反を [`TypesetError::Font`] の非空集合として持つ。
fn load_fonts(
  config: &ProjectConfig,
  font_data: &FontData,
) -> (Result<FontSystem, Failures<TypesetError>>, Vec<FontWarning>) {
  let phase = Phase::enter(info_span!("font"));
  let (font_resources, font_warnings) = FontSystem::load(&config.font_configs, font_data);
  let font_resources = font_resources.map_err(|failures| return failures.map(TypesetError::from));
  if font_resources.is_ok() {
    info!(warning_count = font_warnings.len(), "フォント資源を構築");
    phase.succeed();
  }
  return (font_resources, font_warnings);
}

/// 意味解析の成果物を確定レイアウトへ組版する（`typeset` phase の前半）。
///
/// # Errors
///
/// 画像の読込・デコード・自然寸法の検証、または脚注のページ単位採番の収束に
/// 失敗した場合に、その段で見つかった失敗を非空集合で返す。
fn lay_out(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  geometry: &PreparedGeometry,
  font_resources: &FontSystem,
  document: &SemanticDocument,
) -> Result<(LaidOutDocument, Vec<TypesetWarning>), Failures<TypesetError>> {
  let image_paths = image::collect_image_paths(document.hir());
  let images = image::load_image_resources(source, &image_paths)?;
  let ctx = pagination::TypesetContext::new(config, style, geometry, font_resources);
  return pagination::paginate(&ctx, document, images, image_paths).map_err(Failures::single);
}
