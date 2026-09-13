//! 組版 module — 意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の [`Publication`]
//! へ変換する（旧 `typeset` crate、#307 で `seiran` の非公開 module として吸収）
//!
//! 外向きの入口は [`compose`] の 1 操作だけで、フォント資源の構築（解析 → メトリクス → 検証 →
//! シェーパー）から段順序（lowering → 計測 → 画像寸法確定 → 行分割・改ページ → 前付け・後付け →
//! ページラベル → 走り文 → outline）、確定座標の描画命令への変換までを、その間に成立する
//! 不変条件（box 計測は 1 回だけ・`breaking` はフォントに触れない）とあわせてすべて実装側に
//! 閉じる（#350 / #535）。
//!
//! 組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）は本 module 非公開の
//! 子 module `boxes` が所有する（#280、#350 で `layout` から改名）。組版中間型は `typeset` の外に
//! 本体コードの消費者を持たない（#535）。
//!
//! フォント処理（OpenType 解析・検証・メトリクス・シェイピング）は子 module `font` が持つ
//! （#352）。入力の 19 種別・設定・バイト列は `project::font` の所有で、この module はそこから
//! フォント資源を組み立てて使う側になる。

use std::{mem, time::Instant};

use font::{FontResources, FontWarning};
use tracing::{info, info_span};

use crate::{
  failures::Failures,
  project::{FontData, ProjectPath, ProjectSource, config::ProjectConfig},
  publication::Publication,
  semantics::SemanticDocument,
  style::Style,
};

mod boxes;
mod boxing;
mod breaking;
mod emit;
mod error;
mod font;
mod geometry;
mod image;
mod lowering;
mod observe;
mod pagination;
mod warning;

// 確定ページ列の決定的テキストダンプ（golden 比較用）。走査対象が `boxes` の中間型なので所有は
// こちら側で、`compiler::golden` は `dump_pages` の 1 関数だけを借りる（#353）。
#[cfg(test)]
mod dump;
// 外側の module のテストが確定レイアウトを組み立てるための fixture builder（#353）。
#[cfg(test)]
pub(crate) mod test_fixtures;

// 組版中間型は `typeset` の外に本体コードの消費者を持たない（#535）。`compiler::golden` が
// 確定レイアウトへ直接アサートするためだけに、テストビルドでのみ facade へ出す
// （`compiler::project_source_equivalence` はこれらの型を使わず `Publication` にしかアサートしない）。
// テストが確定レイアウトを**組み立てる**手段は `#[cfg(test)]` の子 module `test_fixtures` が持つ
// （#353）。
#[cfg(test)]
pub(crate) use boxes::{AnchorId, AnchorMark, HBoxContent, LinkTarget, Page, PlacedBlock};
// テスト専用の例外 — `compiler::golden` が確定ページ列をダンプ比較するための関数 1 つだけを出す
// （中間型そのものは出さない。`compiler::project_source_equivalence` はここも消費しない）。
#[cfg(test)]
pub(crate) use dump::dump_pages;
// `compose` / `layout_for_test` の失敗型。`compiler` は `CompileFailure::from` の総称 impl 越しに
// 扱うだけだが、`pub(crate)` の signature に現れる名前なので facade に載せる（`compiler` 節の
// doc からも intra-doc link で指す）。
pub(crate) use error::TypesetError;
// 入口は `compose` 1 操作という原則の意図した例外（#351）。用紙・余白 × 段組みの横断制約は
// 組版の不変条件なのでここが所有するが、**呼び出しは入力読込（`compiler::input::load`）の中**で
// 行い、確定した版面 `PreparedGeometry` を `compose` の引数として受け取り直す（#533）。
pub(crate) use geometry::{LayoutValidationError, PreparedGeometry};
// 確定レイアウト。本体コードの消費者は `typeset` 自身（`lay_out` の戻り値と `emit` の入力）だけで、
// `pub(crate)` にしてあるのは `#[cfg(test)]` の出口 `layout_for_test` の戻り値型として
// `compiler` から名指しされるため（#535）。
//
// **`#[cfg(test)]` を付けてはいけない** — `typeset.rs` の本体コード（`compose` / `lay_out`）が
// この名前を使うので、条件付きの再エクスポートと本体用の `use` を並べると
// テストビルドで E0252（同名の重複定義）になる。1 本の無条件な再エクスポートで両方を賄う。
pub(crate) use pagination::LaidOutDocument;
// 組版が見つけた、ユーザーが直せる非致命的問題（#382）。フォント警告も包む（#535）。
// `compiler` が `Warnings` へ積む。
pub(crate) use warning::TypesetWarning;

/// [`compose`] の成果物 — 描画直前の出版物と、それに付随する情報。
///
/// 組版中間型（`Page` / `LaidOutDocument`）は含まない。呼び出し元が要るのは
/// 「描画できる文書」「読んだ画像のパス」の 2 つで、ユーザーに見せる警告は成否と独立に
/// [`compose`] の組の第 2 要素で返す。フォント資源と配置済みページの組を引き回す知識は
/// `typeset` の内側に閉じる（#535 / #550）。
#[derive(Debug)]
pub(crate) struct TypesetOutput {
  /// 座標と描画順が確定した文書
  pub(crate) publication: Publication,
  /// 文書が参照した画像ファイルのパス一覧（重複なし・昇順。`DependencyManifest` 用）
  pub(crate) image_paths: Vec<ProjectPath>,
}

/// 意味解析の成果物を、描画直前の [`Publication`] へ組版する。
///
/// フォント資源の構築（解析 → メトリクス → 検証 → シェーパー）から確定座標の描画命令への
/// 変換までをこの操作 1 つに閉じる。フォントバイト列を借りるのはこの関数の中だけで、
/// 呼び出し元は資源の借用期間を知らない（#535）。
///
/// 画像は `document` が参照しているぶんだけを `source` 経由で読み込み、自然寸法から表示寸法を
/// 確定して描画資源へ載せる（読込は 1 回だけ）。
///
/// 組版を止めないがユーザーが直せる問題（フォント設定の警告・脚注のはみ出し）は
/// [`TypesetWarning`] として組の第 2 要素で返す。順序はフォント → 本体で、これが `compiler::Warnings` に
/// 現れる順序になる（#382 / #535）。**失敗しても確定した警告は返す** — フォント資源の構築で確定した警告は
/// 後の配置が失敗しても残す。配置由来の警告（脚注のはみ出し）は配置が成功したときにしか確定しない
/// （脚注のページ単位採番の反復で採用されなかった配置の警告を残さない）ので、配置が失敗した実行では
/// 返さない（#550）。
///
/// 版面（`geometry`）は入力読込が検証済みの値として渡すもので、この中で config / style から
/// 幅・ページ幾何を組み立て直すことはしない（#533）。`geometry` は必ず、ここで渡す `config` /
/// `style` と同じ組から `PreparedGeometry::prepare` した値でなければなりません — 引数はいずれも
/// 同じ `CompilationInputs` から読むもので、型としてはこの一致を強制していません。
///
/// `tracing` の phase span（`font` / `typeset`）と各段の完了 event はこの操作の内側
/// （[`load_fonts`] / [`compose`]）が持つ（#500 の工程表示を変えないため、span 名と
/// event のメッセージ・フィールドは #535 の前後で同一）。
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

  let _phase = info_span!("typeset").entered();
  let stage_start = Instant::now();
  let (mut laid_out, layout_warnings) = match lay_out(source, config, style, geometry, &font_resources, document) {
    Ok(laid_out) => laid_out,
    Err(failures) => return (Err(failures), warnings),
  };
  let image_paths = mem::take(&mut laid_out.image_paths);
  let publication = emit::emit(config, font_data, &font_resources, laid_out);
  info!(
    page_count = publication.pages().len(),
    warning_count = layout_warnings.len(),
    elapsed = ?stage_start.elapsed(),
    "文書を組版"
  );

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
/// span と完了 event をここが持ち、構築順序（解析 → メトリクス → 検証 → シェーパー）は
/// `font` module に閉じる（#352）。検証で確定した警告は、構築が失敗しても組の第 2 要素で返す。
///
/// # Errors
///
/// フォント解析・メトリクス取得・設定検証のいずれかに失敗した場合に、組の第 1 要素が、その段で見つかった
/// 違反を [`TypesetError::Font`] の非空集合として持つ（`FontSystemError` を transparent に包むだけなので
/// 診断の出方は変わらない）。
fn load_fonts<'a>(
  config: &'a ProjectConfig,
  font_data: &'a FontData,
) -> (Result<FontResources<'a>, Failures<TypesetError>>, Vec<FontWarning>) {
  let _phase = info_span!("font").entered();
  let stage_start = Instant::now();
  let (font_resources, font_warnings) = FontResources::load(&config.font_configs, font_data);
  let font_resources = font_resources.map_err(|failures| return failures.map(TypesetError::from));
  if font_resources.is_ok() {
    info!(
      warning_count = font_warnings.len(),
      elapsed = ?stage_start.elapsed(),
      "フォント資源を構築"
    );
  }
  return (font_resources, font_warnings);
}

/// 意味解析の成果物を確定レイアウトへ組版する（`typeset` phase の前半）。
///
/// 段順序（シェーパー構築 → 画像パス収集 → 画像読込 → lowering → 計測 → 行分割・改ページ →
/// 前付け・後付け → ページラベル → 走り文 → outline）はここから先の実装に閉じる。
///
/// # Errors
///
/// シェーパーの構築、画像の読込・デコード・寸法確定、または脚注のページ単位採番の収束に
/// 失敗した場合に、その段で見つかった失敗を非空集合で返す。
fn lay_out(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  geometry: &PreparedGeometry,
  font_resources: &FontResources<'_>,
  document: &SemanticDocument,
) -> Result<(LaidOutDocument, Vec<TypesetWarning>), Failures<TypesetError>> {
  // シェーパー構築は画像読込より前に置く — 両方が失敗する入力では、常にフォント側の
  // エラーを報告するため。
  let font_system = font_resources.system().map_err(|failures| return failures.map(TypesetError::from))?;
  let image_paths = image::collect_image_paths(document.hir());
  let images = image::load_image_resources(source, &image_paths)?;
  let ctx = pagination::TypesetContext::new(config, style, geometry, &font_system);
  return pagination::paginate(&ctx, document, images, image_paths).map_err(Failures::single);
}

/// [`compose`] と同じ経路で組版し、確定レイアウトを取り出すテスト専用の出口。
///
/// `Publication` へ変換すると失われる情報（anchor・索引語のページ帰属・脚注 fragment・
/// `PlacedBlock` の幾何）を検査するテストだけが使う。[`compose`] と同じ [`load_fonts`] /
/// [`lay_out`] を通るので、フォント資源の構築順序や組版の段順序を迂回できない（#522 / #535）。
///
/// [`compose`] と異なり `info_span!("typeset")` には入らない（`font` span は [`load_fonts`] が
/// 開くのでそのまま残る）。テスト専用の出口なので tracing の出方を production と揃える必要は
/// なく、意図的にこのままにしてある。
///
/// # Errors
///
/// [`compose`] と同じ条件で失敗する。
#[cfg(test)]
pub(crate) fn layout_for_test(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  geometry: &PreparedGeometry,
  font_data: &FontData,
  document: &SemanticDocument,
) -> Result<LaidOutDocument, Failures<TypesetError>> {
  let (font_resources, _font_warnings) = load_fonts(config, font_data);
  let font_resources = font_resources?;
  let (laid_out, _layout_warnings) = lay_out(source, config, style, geometry, &font_resources, document)?;
  return Ok(laid_out);
}
